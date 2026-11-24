#!/bin/sh
# Serveur HTTPS de test (localhost seulement) pour une release locale :
# install.sh n'accepte que https. Certificat auto-signe jetable, ecrit dans
# <dossier tls>/server.crt (a passer a curl via CURL_CA_BUNDLE).
#
#   serve-release.sh <racine> <port> <dossier tls>
set -eu

main() {
	[ "$#" -eq 3 ] || {
		printf 'usage : serve-release.sh <racine> <port> <dossier tls>\n' >&2
		exit 2
	}
	root=$1
	port=$2
	tls=$3
	mkdir -p "$tls"
	chmod 0700 "$tls"
	openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 1 \
		-subj /CN=localhost -addext subjectAltName=DNS:localhost,IP:127.0.0.1 \
		-keyout "$tls/server.key" -out "$tls/server.crt" 2>/dev/null
	chmod 0644 "$tls/server.crt"
	exec python3 - "$root" "$port" "$tls" <<'PY'
import functools, http.server, ssl, sys

root, port, tls = sys.argv[1], int(sys.argv[2]), sys.argv[3]
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=root)
server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.minimum_version = ssl.TLSVersion.TLSv1_2
ctx.load_cert_chain(f"{tls}/server.crt", f"{tls}/server.key")
server.socket = ctx.wrap_socket(server.socket, server_side=True)
open(f"{tls}/ready", "w").close()
server.serve_forever()
PY
}

main "$@"
