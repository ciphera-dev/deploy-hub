#!/usr/bin/env bash
# Prepare la machine du parcours golden-path (web/e2e/golden-path.spec.ts) :
# instance Ciphera installee par install.sh sur la machine elle-meme (chemin
# de production : systemd, Docker, PostgreSQL et buildkitd lances par
# l'agent), Pebble (serveur ACME de test) et son DNS, GitHub simule
# (fake_github.py : API, pages du flux manifest, depot Git prive).
#
#   sudo CIPHERA_DESTRUCTIVE_HOST_TEST=1 install/tests/golden-path/setup.sh <dossier>
#
# ATTENTION : installe Ciphera et modifie la machine (/etc/hosts, magasin de
# certificats du systeme, unites systemd, conteneurs Docker). Reserve a une
# machine jetable (job CI e2e-golden-path) : refuse sans
# CIPHERA_DESTRUCTIVE_HOST_TEST=1.
#
# Exige : root, systemd, Docker, minisign, openssl, python3, git, curl, tar,
# sha256sum, ss, update-ca-certificates. CIPHERA_TEST_BIN_DIR : binaires musl
# (defaut target/<cible musl>/release), ciphera-cli construit avec
# `--features install-test` (job CI e2e-golden-path). web/dist doit avoir
# ete construit avant les binaires (interface embarquee).
#
# Reseau de test (aucune requete vers github.com apres install.sh, qui y
# telecharge les outils de build) :
#   - API GitHub : http://127.0.0.1:9480 (CIPHERA_GITHUB_API_URL du plan de
#     controle) ; le navigateur du test y detourne https://github.com/ ;
#   - depot Git : github.com -> 127.0.0.2 (/etc/hosts), servi en HTTPS sur
#     127.0.0.2:443 avec un certificat signe par une AC jetable ajoutee au
#     magasin du systeme (git lance par l'agent) ; le proxy Ciphera n'ecoute
#     donc que sur 127.0.0.1 (CIPHERA_PROXY_HTTP / CIPHERA_PROXY_HTTPS) ;
#   - domaine du projet : golden.ciphera-e2e.org -> 127.0.0.1 (/etc/hosts,
#     verification DNS du plan de controle) ; Pebble resout tout nom vers
#     127.0.0.1 (pebble-challtestsrv) et valide HTTP-01 sur le port 5002, ou
#     le proxy ecoute aussi (meme reglage que le job acme-pebble).
#
# Ecrit <dossier>/e2e.env (variables CIPHERA_E2E_* lues par le test) ; le
# jeton d'installation est masque dans le journal GitHub Actions.

# Les commandes entre apostrophes sont evaluees par sh -c.
# shellcheck disable=SC2016
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)

die() {
	printf 'golden-path/setup.sh : %s\n' "$*" >&2
	exit 1
}

[ "${CIPHERA_DESTRUCTIVE_HOST_TEST:-}" = 1 ] ||
	die "modifie la machine : relancer avec CIPHERA_DESTRUCTIVE_HOST_TEST=1 (machine jetable seulement)"
[ "$(id -u)" -eq 0 ] || die "droits root requis"
[ "$#" -eq 1 ] || die "usage : setup.sh <dossier de travail>"
for t in docker systemctl systemd-run minisign openssl python3 git curl tar sha256sum ss update-ca-certificates; do
	command -v "$t" >/dev/null || die "outil requis absent : $t"
done
docker info >/dev/null 2>&1 || die "Docker injoignable"
[ ! -e /etc/ciphera ] || die "/etc/ciphera existe deja : machine non vierge"

case "$(uname -m)" in
x86_64 | amd64) target=x86_64-unknown-linux-musl ;;
aarch64 | arm64) target=aarch64-unknown-linux-musl ;;
*) die "architecture non geree : $(uname -m)" ;;
esac
bin_dir=${CIPHERA_TEST_BIN_DIR:-$root/target/$target/release}
installer="$root/install/install.sh"
version=$(sed -n "s/^DEFAULT_VERSION='\(v[0-9.]*\)'\$/\1/p" "$installer")
[ -n "$version" ] || die "DEFAULT_VERSION introuvable dans install.sh"

work=$1
mkdir -p "$work"
work=$(cd "$work" && pwd)
chmod 0755 "$work"

domain=golden.ciphera-e2e.org
api_addr=127.0.0.1:9480
git_ip=127.0.0.2
panel=https://127.0.0.1:8443
pebble_challtestsrv=ghcr.io/letsencrypt/pebble-challtestsrv@sha256:0152c8cb506e2ecba2900ed47d23932e27e3e6575a7d2c8ef93c7719ee3dea4c
pebble_image=ghcr.io/letsencrypt/pebble@sha256:0c81f05db371f436c97fb894bd6d2ab8fc9c8e2ce40f8bef8a8e15941bcd75ef

# wait_for <secondes> <commande sh>
wait_for() {
	local i
	for i in $(seq 1 "$1"); do
		if sh -c "$2" >/dev/null 2>&1; then
			return 0
		fi
		[ "$i" -lt "$1" ] && sleep 1
	done
	return 1
}

diagnose() {
	journalctl -u ciphera -u ciphera-agent -u ciphera-proxy -u ciphera-e2e-github --no-pager -n 80 || true
	docker ps -a || true
}

server_pid=
cleanup() {
	if [ -n "$server_pid" ]; then
		kill "$server_pid" 2>/dev/null || true
	fi
}
trap cleanup EXIT

# --- 1. installation (release de test signee par une cle jetable) -------------
printf '== installation de Ciphera\n'
port=9443
archive="ciphera-$version-$target.tar.gz"
release="$work/release/srv/good/releases/$version"
mkdir -p "$work/release/keys" "$release"
minisign -G -W -p "$work/release/keys/test.pub" -s "$work/release/keys/test.key" >/dev/null
pubkey=$(sed -n 2p "$work/release/keys/test.pub")
sh "$root/install/package.sh" "$version" "$target" "$bin_dir" "$release"
(cd "$release" && sha256sum "$archive" >SHA256SUMS)
minisign -S -s "$work/release/keys/test.key" -m "$release/SHA256SUMS" >/dev/null
rm -f "$work/release/keys/test.key"

sh "$root/install/tests/serve-release.sh" "$work/release/srv" "$port" "$work/release/tls" &
server_pid=$!
wait_for 30 "test -e '$work/release/tls/ready'" || die "serveur de release non demarre"

if ! env CIPHERA_INSTALL_TEST=1 \
	"CIPHERA_RELEASE_BASE_URL=https://localhost:$port/good" \
	"CIPHERA_TEST_MINISIGN_PUBKEY=$pubkey" \
	"CURL_CA_BUNDLE=$work/release/tls/server.crt" \
	sh "$installer" --non-interactive --public-ip 127.0.0.1 >"$work/install.log" 2>&1; then
	cat "$work/install.log"
	diagnose
	die "install.sh en echec"
fi
kill "$server_pid" 2>/dev/null || true
server_pid=
wait_for 300 "curl -fsk $panel/api/health" || {
	diagnose
	die "panneau injoignable apres l'installation"
}
# Jeton lu dans l'adresse affichee par install.sh (fragment `#token=`).
token=$(sed -n 's|^https://[^ ]*/setup#token=\([A-Za-z0-9_-]*\)$|\1|p' "$work/install.log" | head -n 1)
[ -n "$token" ] || die "adresse d'installation (/setup#token=) absente de la sortie d'install.sh"
printf '::add-mask::%s\n' "$token"

# --- 2. Pebble et son DNS de test ---------------------------------------------
printf '== Pebble\n'
docker run -d --name challtestsrv --network host "$pebble_challtestsrv" \
	-defaultIPv4 127.0.0.1 -defaultIPv6 "" -http01 "" -https01 "" -tlsalpn01 "" -doh "" >/dev/null
docker run -d --name pebble --network host \
	-e PEBBLE_VA_NOSLEEP=1 -e PEBBLE_WFE_NONCEREJECT=0 \
	"$pebble_image" -config /test/config/pebble-config.json -dnsserver 127.0.0.1:8053 >/dev/null
install -d -m 0755 /etc/ciphera-e2e
docker cp pebble:/test/certs/pebble.minica.pem /etc/ciphera-e2e/pebble.minica.pem
chmod 0644 /etc/ciphera-e2e/pebble.minica.pem
wait_for 30 'curl -fsS --cacert /etc/ciphera-e2e/pebble.minica.pem https://localhost:14000/dir' || {
	docker logs pebble || true
	die "Pebble ne repond pas"
}
# Racine et intermediaire des certificats emis (tires au demarrage de Pebble) :
# seules autorites de confiance du client HTTPS du test.
{
	curl -fsS --cacert /etc/ciphera-e2e/pebble.minica.pem https://localhost:15000/roots/0
	curl -fsS --cacert /etc/ciphera-e2e/pebble.minica.pem https://localhost:15000/intermediates/0
} >"$work/pebble-roots.pem"
chmod 0644 "$work/pebble-roots.pem"

# --- 3. Ciphera : API GitHub simulee, proxy sur 127.0.0.1 et annuaire Pebble ---
printf '== configuration de test de Ciphera\n'
printf 'CIPHERA_GITHUB_API_URL=http://%s\n' "$api_addr" >>/etc/ciphera/ciphera.env
install -d -m 0755 /etc/systemd/system/ciphera-proxy.service.d
cat >/etc/systemd/system/ciphera-proxy.service.d/e2e.conf <<'EOF'
# Parcours golden-path : 127.0.0.2:443 reste libre pour le GitHub simule ;
# 5002 est le port ou Pebble valide HTTP-01 (ecoute autorisee en plus de
# 80 et 443 : SocketBindDeny=any dans l'unite).
[Service]
SocketBindAllow=tcp:5002
Environment=CIPHERA_PROXY_HTTP=127.0.0.1:80,127.0.0.1:5002
Environment=CIPHERA_PROXY_HTTPS=127.0.0.1:443
Environment=CIPHERA_PROXY_ACME_DIRECTORY=https://localhost:14000/dir
Environment=CIPHERA_PROXY_ACME_CA_FILE=/etc/ciphera-e2e/pebble.minica.pem
EOF
systemctl daemon-reload
systemctl restart ciphera-proxy ciphera
wait_for 60 "ss -Hltn 'sport = :443' | grep -q '127.0.0.1:443'" || {
	diagnose
	die "proxy non lie a 127.0.0.1:443"
}
if ss -Hltn 'sport = :443' | grep -qv '127.0.0.1:443'; then
	ss -Hltn 'sport = :443'
	die "une autre ecoute occupe le port 443"
fi
wait_for 120 "curl -fsk $panel/api/health" || {
	diagnose
	die "panneau injoignable apres la configuration"
}

# --- 4. GitHub simule -----------------------------------------------------------
printf '== GitHub simule\n'
gh="$work/github"
mkdir -p "$gh/tls"
(
	cd "$gh/tls"
	openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 2 \
		-subj "/CN=Ciphera e2e CA" -addext basicConstraints=critical,CA:TRUE \
		-addext keyUsage=critical,keyCertSign,cRLSign -keyout ca.key -out ca.crt 2>/dev/null
	openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj /CN=github.com \
		-keyout server.key -out server.csr 2>/dev/null
	printf 'subjectAltName=DNS:github.com\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n' >server.ext
	openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 2 \
		-extfile server.ext -out server.crt 2>/dev/null
	rm -f ca.key server.csr
)
install -m 0644 "$gh/tls/ca.crt" /usr/local/share/ca-certificates/ciphera-e2e-ca.crt
update-ca-certificates >/dev/null

# Cle de l'App (remise au plan de controle a la conversion du manifest).
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$gh/app-key.pem" 2>/dev/null
openssl pkey -in "$gh/app-key.pem" -pubout -out "$gh/app-key.pub.pem"

# Depot golden/app : main sur v1, v2 en attente sous refs/e2e/next.
src="$gh/src"
mkdir -p "$src"
git init -q -b main "$src"
cat >"$src/Dockerfile" <<'EOF'
FROM docker.io/library/busybox:1.37
COPY index.html /srv/index.html
CMD ["httpd", "-f", "-p", "8080", "-h", "/srv"]
EOF
commit() { # <message>
	git -C "$src" -c user.name=golden -c user.email=golden@ciphera-e2e.org add -A
	git -C "$src" -c user.name=golden -c user.email=golden@ciphera-e2e.org commit -q -m "$1"
	git -C "$src" rev-parse HEAD
}
printf 'ciphera golden v1\n' >"$src/index.html"
v1=$(commit "golden v1")
printf 'ciphera golden v2\n' >"$src/index.html"
v2=$(commit "golden v2")
bare="$gh/repos/golden/app.git"
mkdir -p "$(dirname "$bare")"
git clone -q --bare "$src" "$bare"
git -C "$bare" update-ref refs/heads/main "$v1"
git -C "$bare" update-ref refs/e2e/next "$v2"
git -C "$bare" config uploadpack.allowReachableSHA1InWant true

grep -q " github.com\$" /etc/hosts && die "/etc/hosts detourne deja github.com"
printf '%s github.com\n127.0.0.1 %s\n' "$git_ip" "$domain" >>/etc/hosts

systemd-run --unit=ciphera-e2e-github --collect --quiet \
	python3 -I "$here/fake_github.py" --root "$gh" --api "$api_addr" --git "$git_ip:443" \
	--cert "$gh/tls/server.crt" --key "$gh/tls/server.key"
wait_for 30 "curl -fsS http://$api_addr/_e2e/health" || {
	diagnose
	die "API GitHub simulee injoignable"
}
# Certificat verifie par le magasin du systeme ; 401 sans jeton (depot prive).
wait_for 30 'test "$(curl -s -o /dev/null -w "%{http_code}" "https://github.com/golden/app/info/refs?service=git-upload-pack")" = 401' || {
	diagnose
	die "depot Git simule injoignable en HTTPS sous le nom github.com"
}

# --- 5. variables du test ---------------------------------------------------------
{
	printf 'CIPHERA_E2E_URL=%s\n' "$panel"
	printf 'CIPHERA_E2E_SETUP_TOKEN=%s\n' "$token"
	printf 'CIPHERA_E2E_GITHUB=http://%s\n' "$api_addr"
	printf 'CIPHERA_E2E_DOMAIN=%s\n' "$domain"
	printf 'CIPHERA_E2E_ACME_ROOTS=%s\n' "$work/pebble-roots.pem"
} >"$work/e2e.env"
chmod 0644 "$work/e2e.env"
printf 'machine prete : %s\n' "$work/e2e.env"
