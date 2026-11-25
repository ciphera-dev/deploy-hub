#!/usr/bin/env python3
"""GitHub simule du parcours golden-path (install/tests/golden-path/setup.sh).

Deux ecoutes :

- API et pages web (HTTP, boucle locale ; CIPHERA_GITHUB_API_URL du plan de
  controle, et cible du detournement de https://github.com/ par le navigateur
  du test) : flux « manifest » (formulaire /settings/apps/new, conversion du
  code), installation de l'App, jetons d'installation, depots de
  l'installation, depot (id), commit en tete de branche ; plus POST /_e2e/advance (le test
  avance main sur le commit suivant) et GET /_e2e/health ;
- depot Git (HTTPS, nom github.com via /etc/hosts) : `git http-backend` en
  lecture seule (git-upload-pack), authentification Basic
  `x-access-token:<jeton>` avec un jeton emis par l'API, comme un depot prive.

Le JWT de l'App est verifie (RS256, cle publique de la cle remise a la
conversion, `iss`, `exp`). Donnees de test seulement : aucun secret reel,
aucune requete vers github.com. Aucun shell : sous-processus en argv.

    fake_github.py --root <dossier> --api 127.0.0.1:9480 --git 127.0.0.2:443 \
        --cert <certificat github.com> --key <cle>

<dossier> contient app-key.pem (cle RSA de l'App), app-key.pub.pem et le
depot nu repos/golden/app.git (refs/heads/main, refs/e2e/next).
"""

import argparse
import base64
import binascii
import datetime
import html
import json
import os
import re
import secrets
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

OWNER = "golden"
REPO = "app"
FULL_NAME = f"{OWNER}/{REPO}"
APP_ID = 424242
INSTALLATION_ID = 4242
TOKEN_USER = "x-access-token"
MAX_BODY = 1024 * 1024

BRANCH_RE = re.compile(r"^[A-Za-z0-9._-]{1,100}$")
CODE_RE = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
SLUG_RE = re.compile(r"^[a-z0-9-]{1,100}$")
GIT_PATH_RE = re.compile(r"^/golden/app(?:\.git)?(/info/refs|/git-upload-pack)$")


class State:
    """Etat partage des deux ecoutes (verrou unique)."""

    def __init__(self, root):
        self.lock = threading.Lock()
        self.root = root
        self.repos = os.path.join(root, "repos")
        self.bare = os.path.join(self.repos, OWNER, REPO + ".git")
        with open(os.path.join(root, "app-key.pem"), encoding="ascii") as f:
            self.pem = f.read()
        self.public_key = os.path.join(root, "app-key.pub.pem")
        self.manifests = {}  # code -> (manifest, state)
        self.app = None
        self.installed = False
        self.tokens = set()

    def git(self, *args):
        return subprocess.run(
            ["git", "--git-dir", self.bare, *args],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()


def b64url_decode(value):
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


def utc_iso(offset_secs=0):
    t = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=offset_secs)
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def page(title, body):
    return (
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
        f"<title>{html.escape(title)}</title></head><body>{body}</body></html>"
    ).encode("utf-8")


class Base(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "fake-github"
    sys_version = ""

    @property
    def state(self):
        return self.server.state

    def log_message(self, fmt, *args):
        # Ligne de requete et statut seulement (jamais d'en-tete Authorization).
        sys.stderr.write("%s %s\n" % (self.server.label, fmt % args))

    def reply(self, status, body, content_type, extra=None):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        for name, value in (extra or {}).items():
            self.send_header(name, value)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def json(self, status, value):
        self.reply(status, json.dumps(value).encode("utf-8"), "application/json; charset=utf-8")

    def html(self, title, body):
        self.reply(200, page(title, body), "text/html; charset=utf-8")

    def not_found(self):
        self.json(404, {"message": "Not Found"})

    def read_body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            chunks = []
            total = 0
            while True:
                size = int(self.rfile.readline().split(b";", 1)[0].strip(), 16)
                if size == 0:
                    while self.rfile.readline() not in (b"\r\n", b"\n", b""):
                        pass
                    break
                total += size
                if total > MAX_BODY:
                    raise ValueError("corps trop grand")
                chunks.append(self.rfile.read(size))
                self.rfile.readline()
            return b"".join(chunks)
        length = int(self.headers.get("Content-Length") or 0)
        if length > MAX_BODY:
            raise ValueError("corps trop grand")
        return self.rfile.read(length) if length else b""

    def bearer(self):
        value = self.headers.get("Authorization", "")
        for scheme in ("Bearer ", "token "):
            if value.startswith(scheme):
                return value[len(scheme):].strip()
        return None

    def jwt_ok(self):
        """JWT RS256 de l'App : signature (cle publique), `iss`, `exp`."""
        token = self.bearer()
        if not token or token.count(".") != 2:
            return False
        header_b64, claims_b64, signature_b64 = token.split(".")
        try:
            header = json.loads(b64url_decode(header_b64))
            claims = json.loads(b64url_decode(claims_b64))
            signature = b64url_decode(signature_b64)
        except (ValueError, binascii.Error):
            return False
        if header.get("alg") != "RS256" or claims.get("iss") != str(APP_ID):
            return False
        if not isinstance(claims.get("exp"), int) or claims["exp"] < time.time():
            return False
        with tempfile.TemporaryDirectory() as d:
            data = os.path.join(d, "data")
            sig = os.path.join(d, "sig")
            with open(data, "wb") as f:
                f.write(f"{header_b64}.{claims_b64}".encode("ascii"))
            with open(sig, "wb") as f:
                f.write(signature)
            check = subprocess.run(
                ["openssl", "dgst", "-sha256", "-verify", self.state.public_key, "-signature", sig, data],
                capture_output=True,
                check=False,
            )
        return check.returncode == 0

    def installation_token_ok(self):
        token = self.bearer()
        with self.state.lock:
            return token is not None and token in self.state.tokens


class ApiHandler(Base):
    """API REST de GitHub (champs lus par le plan de controle) et pages web."""

    def repo_json(self):
        return {
            "id": 1,
            "name": REPO,
            "full_name": FULL_NAME,
            "owner": {"login": OWNER},
            "private": True,
            "default_branch": "main",
            "updated_at": utc_iso(),
        }

    def do_GET(self):
        url = urllib.parse.urlsplit(self.path)
        path = url.path
        if path == "/_e2e/health":
            return self.json(200, {"ok": True})
        m = re.match(r"^/apps/([^/]+)/installations/new$", path)
        if m:
            return self.install_page(m.group(1))
        if path == "/app/installations":
            if not self.jwt_ok():
                return self.json(401, {"message": "Bad credentials"})
            with self.state.lock:
                installed = self.state.installed
            body = [{"id": INSTALLATION_ID, "account": {"login": OWNER}}] if installed else []
            return self.json(200, body)
        if path == "/installation/repositories":
            if not self.installation_token_ok():
                return self.json(401, {"message": "Bad credentials"})
            return self.json(200, {"total_count": 1, "repositories": [self.repo_json()]})
        if path == f"/repos/{FULL_NAME}/installation":
            if not self.jwt_ok():
                return self.json(401, {"message": "Bad credentials"})
            with self.state.lock:
                installed = self.state.installed
            return self.json(200, {"id": INSTALLATION_ID}) if installed else self.not_found()
        if path == f"/repos/{FULL_NAME}":
            if not self.installation_token_ok():
                return self.not_found()
            return self.json(200, self.repo_json())
        m = re.match(rf"^/repos/{FULL_NAME}/commits/(.+)$", path)
        if m:
            if not self.installation_token_ok():
                return self.not_found()
            branch = m.group(1)
            if not BRANCH_RE.match(branch):
                return self.not_found()
            try:
                sha = self.state.git("rev-parse", "--verify", f"refs/heads/{branch}")
            except subprocess.CalledProcessError:
                return self.not_found()
            return self.json(200, {"sha": sha})
        return self.not_found()

    def do_POST(self):
        url = urllib.parse.urlsplit(self.path)
        path = url.path
        try:
            body = self.read_body()
        except ValueError:
            return self.json(413, {"message": "Payload too large"})
        if path == "/settings/apps/new":
            return self.manifest_form(url.query, body)
        m = re.match(r"^/app-manifests/([^/]+)/conversions$", path)
        if m:
            return self.convert(m.group(1))
        m = re.match(r"^/app/installations/(\d+)/access_tokens$", path)
        if m:
            if not self.jwt_ok():
                return self.json(401, {"message": "Bad credentials"})
            # Le jeton se limite au depot (nom ou id, lecture du contenu) ou,
            # pour la liste des depots, aux metadonnees en lecture.
            if body:
                try:
                    wanted = json.loads(body)
                except ValueError:
                    return self.json(422, {"message": "Invalid request"})
                scoped = {"permissions": {"contents": "read"}}
                allowed = [
                    {**scoped, "repositories": [REPO]},
                    {**scoped, "repository_ids": [1]},
                    {"permissions": {"metadata": "read"}},
                ]
                if wanted not in allowed:
                    return self.json(422, {"message": "Invalid request"})
            with self.state.lock:
                if int(m.group(1)) != INSTALLATION_ID or not self.state.installed:
                    return self.not_found()
                token = "ghs_" + secrets.token_hex(18)
                self.state.tokens.add(token)
            return self.json(201, {"token": token, "expires_at": utc_iso(3600)})
        if path == "/_e2e/advance":
            with self.state.lock:
                self.state.git("update-ref", "refs/heads/main", "refs/e2e/next")
                sha = self.state.git("rev-parse", "refs/heads/main")
            return self.json(200, {"sha": sha})
        return self.not_found()

    def do_PATCH(self):
        if urllib.parse.urlsplit(self.path).path != "/app/hook/config":
            return self.not_found()
        if not self.jwt_ok():
            return self.json(401, {"message": "Bad credentials"})
        try:
            url = json.loads(self.read_body() or b"{}").get("url")
        except ValueError:
            return self.json(422, {"message": "Invalid request"})
        with self.state.lock:
            if self.state.app is None:
                return self.not_found()
            self.state.app["hook_url"] = url
        return self.json(200, {"content_type": "json", "insecure_ssl": "0", "url": url})

    def manifest_form(self, query, body):
        """Formulaire de creation d'App : page avec le lien de retour vers le panneau."""
        state = urllib.parse.parse_qs(query).get("state", [""])[0]
        form = urllib.parse.parse_qs(body.decode("utf-8", "replace"))
        try:
            manifest = json.loads(form["manifest"][0])
            redirect = manifest["redirect_url"]
        except (KeyError, IndexError, ValueError, TypeError):
            return self.json(422, {"message": "Invalid manifest"})
        if not CODE_RE.match(state) or not isinstance(redirect, str) or not redirect.startswith("https://"):
            return self.json(422, {"message": "Invalid manifest"})
        code = secrets.token_hex(10)
        with self.state.lock:
            self.state.manifests[code] = manifest
        back = redirect + "?" + urllib.parse.urlencode({"code": code, "state": state})
        self.html(
            "Create GitHub App",
            f"<h1>Create GitHub App for {html.escape(OWNER)}</h1>"
            f"<p>{html.escape(str(manifest.get('name', '')))}</p>"
            f"<a href=\"{html.escape(back)}\">Create GitHub App for {html.escape(OWNER)}</a>",
        )

    def convert(self, code):
        """POST /app-manifests/{code}/conversions : code a usage unique."""
        if not CODE_RE.match(code):
            return self.not_found()
        with self.state.lock:
            manifest = self.state.manifests.pop(code, None)
            if manifest is None:
                return self.not_found()
            name = str(manifest.get("name") or "Ciphera")
            slug = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-") or "ciphera"
            secret = secrets.token_hex(20)
            self.state.app = {
                "slug": slug,
                "setup_url": manifest.get("setup_url") or manifest["redirect_url"],
                "hook_url": (manifest.get("hook_attributes") or {}).get("url"),
            }
            self.state.installed = False
            pem = self.state.pem
        return self.json(
            201,
            {
                "id": APP_ID,
                "slug": slug,
                "node_id": "A_golden",
                "name": name,
                "owner": {"login": OWNER},
                "html_url": f"https://github.com/apps/{slug}",
                "client_id": "Iv1.golden",
                "pem": pem,
                "webhook_secret": secret,
            },
        )

    def install_page(self, slug):
        """Page d'installation : l'App est installee sur golden/app."""
        if not SLUG_RE.match(slug):
            return self.not_found()
        with self.state.lock:
            app = self.state.app
            if app is None or app["slug"] != slug:
                return self.not_found()
            self.state.installed = True
            setup_url = app["setup_url"]
        back = setup_url + "?" + urllib.parse.urlencode(
            {"installation_id": INSTALLATION_ID, "setup_action": "install"}
        )
        self.html(
            "Install GitHub App",
            f"<h1>Install {html.escape(slug)}</h1>"
            f"<p>Repository access: {html.escape(FULL_NAME)}</p>"
            f"<a href=\"{html.escape(back)}\">Install</a>",
        )


class GitHandler(Base):
    """Depot Git en HTTP « smart » (git-upload-pack seulement) par git http-backend."""

    def do_GET(self):
        self.backend()

    def do_POST(self):
        self.backend()

    def authorized(self):
        value = self.headers.get("Authorization", "")
        if not value.startswith("Basic "):
            return False
        try:
            user, _, password = base64.b64decode(value[6:], validate=True).decode("utf-8").partition(":")
        except (ValueError, binascii.Error):
            return False
        with self.state.lock:
            return user == TOKEN_USER and password in self.state.tokens

    def backend(self):
        url = urllib.parse.urlsplit(self.path)
        m = GIT_PATH_RE.match(url.path)
        if not m:
            return self.reply(404, b"Not Found\n", "text/plain")
        service = urllib.parse.parse_qs(url.query).get("service")
        if m.group(1) == "/info/refs" and service != ["git-upload-pack"]:
            return self.reply(403, b"Forbidden\n", "text/plain")
        if m.group(1) == "/git-upload-pack" and self.command != "POST":
            return self.reply(405, b"Method Not Allowed\n", "text/plain")
        try:
            body = self.read_body()
        except ValueError:
            return self.reply(413, b"Payload Too Large\n", "text/plain")
        if not self.authorized():
            return self.reply(
                401, b"Authentication required\n", "text/plain", {"WWW-Authenticate": 'Basic realm="GitHub"'}
            )
        env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "GIT_PROJECT_ROOT": self.state.repos,
            "GIT_HTTP_EXPORT_ALL": "1",
            "PATH_INFO": f"/{OWNER}/{REPO}.git{m.group(1)}",
            "QUERY_STRING": url.query,
            "REQUEST_METHOD": self.command,
            "CONTENT_TYPE": self.headers.get("Content-Type", ""),
            "CONTENT_LENGTH": str(len(body)),
            "REMOTE_USER": TOKEN_USER,
            "REMOTE_ADDR": self.client_address[0],
        }
        if self.headers.get("Git-Protocol"):
            env["GIT_PROTOCOL"] = self.headers["Git-Protocol"]
        if self.headers.get("Content-Encoding"):
            env["HTTP_CONTENT_ENCODING"] = self.headers["Content-Encoding"]
        out = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True, check=False)
        if out.returncode != 0:
            sys.stderr.write("git http-backend : code %d : %s\n" % (out.returncode, out.stderr.decode("utf-8", "replace")))
            return self.reply(500, b"Internal Server Error\n", "text/plain")
        head, sep, payload = out.stdout.partition(b"\r\n\r\n")
        if not sep:
            head, sep, payload = out.stdout.partition(b"\n\n")
        status = 200
        headers = []
        for line in head.decode("latin-1").splitlines():
            name, _, value = line.partition(":")
            if name.strip().lower() == "status":
                status = int(value.strip().split()[0])
            elif name.strip().lower() not in ("content-length", "transfer-encoding", "connection"):
                headers.append((name.strip(), value.strip()))
        self.send_response(status)
        for name, value in headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def address(value):
    host, _, port = value.rpartition(":")
    return host, int(port)


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        # Client parti en cours de route (fin de git, poignee TLS refusee) : une ligne.
        sys.stderr.write("%s : connexion interrompue (%s)\n" % (self.label, sys.exc_info()[1]))


def server(handler, addr, state, label):
    srv = Server(address(addr), handler)
    srv.state = state
    srv.label = label
    return srv


def main():
    parser = argparse.ArgumentParser(description="GitHub simule du parcours golden-path")
    parser.add_argument("--root", required=True)
    parser.add_argument("--api", required=True, help="hote:port de l'API (HTTP)")
    parser.add_argument("--git", required=True, help="hote:port du depot (HTTPS)")
    parser.add_argument("--cert", required=True)
    parser.add_argument("--key", required=True)
    args = parser.parse_args()

    state = State(args.root)
    api = server(ApiHandler, args.api, state, "api")
    git = server(GitHandler, args.git, state, "git")
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    ctx.load_cert_chain(args.cert, args.key)
    git.socket = ctx.wrap_socket(git.socket, server_side=True)
    threading.Thread(target=git.serve_forever, daemon=True).start()
    sys.stderr.write("GitHub simule : API http://%s, depot https://%s\n" % (args.api, args.git))
    api.serve_forever()


if __name__ == "__main__":
    main()
