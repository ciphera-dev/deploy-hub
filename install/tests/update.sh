#!/usr/bin/env bash
# Tests de bout en bout de la mise a jour de Ciphera (ciphera-cli updater,
# unites ciphera-updater.path et .service), dans un conteneur Podman avec
# systemd (meme machine de test qu'install/tests/run.sh).
#
#   install/tests/update.sh debian [ubuntu] [fedora]
#
# Pour chaque distribution, apres une installation de la release v_A par
# install.sh (releases de test signees par une cle jetable, servies en
# HTTPS local) :
#   - (a) demande au HMAC faux (autre cle) : ignoree, rien ne change ;
#   - (b) demande valide pour la version installee : refusee ;
#   - (c) release signee par une autre cle : echec avant toute
#     modification, aucune sauvegarde ;
#   - (d) demande valide pour v_B deposee comme le plan de controle
#     (ecriture atomique, compte ciphera) : l'unite .path lance l'updater,
#     sauvegarde /var/backups/ciphera/<ts>.sql.zst, v_B installee, outils
#     du manifeste en place, panneau joignable ; l'unite que v_B ajoute
#     (ciphera-test-extra.service) est installee, activee et demarree ;
#     une unite ciphera-* ecrite par l'administrateur (hors registre
#     units.list) reste en place et active, une unite de Ciphera qu'il a
#     desactivee (ciphera-agent.service) le reste ;
#   - (e) release v_C dont `ciphera` modifie la base puis ne demarre pas :
#     retour aux binaires de v_B apres 60 s, base restauree (modification
#     de v_C disparue), panneau de nouveau joignable ; l'unite de v_B que
#     v_C retire est de nouveau activee ;
#   - (f) retour a v_A : refuse sans --allow-downgrade, hors root et sans
#     --restore-db ; accepte en root avec --allow-downgrade et la sauvegarde
#     prise sous v_A ; l'unite ajoutee par v_B est desactivee et retiree ;
#   - (g) updater tue pendant la mise a jour vers v_C : la reprise au
#     demarrage (ciphera-updater-recover.service) remet v_A et sa base ;
#   - (h) PATH et LD_PRELOAD ajoutes a /etc/ciphera/ciphera.env (modifiable
#     par le compte ciphera) n'atteignent jamais l'updater (root).
#
# Ces conteneurs n'ont pas Docker : un faux `docker` (seulement
# `docker exec [-i] -e PGPASSWORD ciphera-postgres pg_dump|psql|pg_isready`)
# lance ces outils sur la base PostgreSQL de la distribution, fixture de
# test comme dans run.sh.
#
# Exige : Linux, podman, tar, sha256sum, curl. minisign est pris sur l'hote
# s'il existe, sinon dans un conteneur Debian.
# CIPHERA_TEST_BIN_DIR : dossier des binaires musl ; defaut
# target/<cible musl>/release.

# Les commandes entre apostrophes sont evaluees dans le conteneur.
# shellcheck disable=SC2016
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
installer="$root/install/install.sh"

die() {
	printf 'update.sh : %s\n' "$*" >&2
	exit 1
}

[ "$#" -gt 0 ] || die "usage : update.sh debian [ubuntu] [fedora]"
for d in "$@"; do
	case "$d" in
	debian | ubuntu | fedora) ;;
	*) die "distribution inconnue : $d" ;;
	esac
done
command -v podman >/dev/null || die "podman requis"
command -v curl >/dev/null || die "curl requis"

case "$(uname -m)" in
x86_64 | amd64) target=x86_64-unknown-linux-musl ;;
aarch64 | arm64) target=aarch64-unknown-linux-musl ;;
*) die "architecture non geree : $(uname -m)" ;;
esac
bin_dir=${CIPHERA_TEST_BIN_DIR:-$root/target/$target/release}
for b in ciphera ciphera-agent ciphera-proxy ciphera-cli; do
	[ -f "$bin_dir/$b" ] || die "binaire absent : $bin_dir/$b"
done
v_a=$(sed -n "s/^DEFAULT_VERSION='\(v[0-9.]*\)'\$/\1/p" "$installer")
[ -n "$v_a" ] || die "DEFAULT_VERSION introuvable dans install.sh"
# Versions suivantes de test : patch + 1, + 2, + 3.
next() { # <vX.Y.Z> <increment>
	local base=${1%.*} patch=${1##*.}
	printf '%s.%s' "$base" "$((patch + $2))"
}
v_b=$(next "$v_a" 1)
v_c=$(next "$v_a" 2)
v_other=$(next "$v_a" 3)

work=$(mktemp -d)
containers=()
cleanup() {
	for c in "${containers[@]}"; do
		podman rm -f "$c" >/dev/null 2>&1 || true
	done
	rm -rf "$work"
}
trap cleanup EXIT

# --- minisign (hote ou conteneur), chemins relatifs a $work -----------------
signer_image=ciphera-install-test-minisign
if command -v minisign >/dev/null; then
	msign() { (cd "$work" && minisign "$@"); }
else
	printf 'FROM docker.io/library/debian:12\nRUN apt-get update && apt-get install -y --no-install-recommends minisign && rm -rf /var/lib/apt/lists/*\n' |
		podman build -q -t "$signer_image" -f - "$work" >/dev/null
	msign() { podman run --rm -v "$work:/w:Z" -w /w "$signer_image" minisign "$@"; }
fi

# --- releases de test ---------------------------------------------------------
mkdir -p "$work/keys"
msign -G -W -p keys/good.pub -s keys/good.key >/dev/null
msign -G -W -p keys/other.pub -s keys/other.key >/dev/null
good_pub=$(sed -n 2p "$work/keys/good.pub")

# release <version> <cle> [dossier de binaires] [dossier d'unites en plus] :
# archive, SHA256SUMS signe.
release() {
	local version=$1 key=$2 bins=${3:-$bin_dir} extra=${4:-}
	local dir="$work/srv/good/releases/$version"
	mkdir -p "$dir"
	CIPHERA_PACKAGE_EXTRA_UNITS=$extra sh "$root/install/package.sh" "$version" "$target" "$bins" "$dir"
	(cd "$dir" && sha256sum "ciphera-$version-$target.tar.gz" >SHA256SUMS)
	msign -S -s "keys/$key.key" -m "srv/good/releases/$version/SHA256SUMS" >/dev/null
}
release "$v_a" good
# v_B ajoute une unite (activee, demarree) ; v_A et v_C ne l'ont pas.
mkdir -p "$work/units-extra"
cat >"$work/units-extra/ciphera-test-extra.service" <<'EOF'
[Unit]
Description=Ciphera - unite ajoutee par une release de test

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/bin/true

[Install]
WantedBy=multi-user.target
EOF
release "$v_b" good "$bin_dir" "$work/units-extra"
# v_C : `ciphera` modifie la base (comme une migration) puis s'arrete
# aussitot (le panneau ne repond plus).
mkdir -p "$work/bins-broken"
cp "$bin_dir"/ciphera-agent "$bin_dir"/ciphera-proxy "$bin_dir"/ciphera-cli "$work/bins-broken/"
cat >"$work/bins-broken/ciphera" <<'EOF'
#!/bin/sh
PATH=/usr/sbin:/usr/bin:/sbin:/bin
PGPASSWORD=$(cat /etc/ciphera/db_password) psql --host=127.0.0.1 --username=ciphera \
	--dbname=ciphera --no-password -q -c 'CREATE TABLE IF NOT EXISTS migration_vc (id int)' || true
exit 1
EOF
chmod 0755 "$work/bins-broken/ciphera"
release "$v_c" good "$work/bins-broken"
release "$v_other" other

# --- outils tiers (railpack, buildctl, mise) ----------------------------------
# Manifeste d'install.sh (source unique des empreintes), telecharges une
# fois sur l'hote depuis GitHub puis servis sous srv/tools.
while read -r _name _version sum path _member _dest; do
	dest="$work/srv/tools/$path"
	mkdir -p "$(dirname "$dest")"
	curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$dest" "https://github.com/$path" ||
		die "telechargement impossible : https://github.com/$path"
	printf '%s  %s\n' "$sum" "$dest" | sha256sum -c --status - ||
		die "SHA-256 de $path incorrect"
done < <(sh "$installer" --tools-manifest "$target")

cp "$installer" "$here/serve-release.sh" "$work/"
rm -f "$work/keys/"*.key

# Faux docker du conteneur de test : seules les commandes de l'updater.
cat >"$work/docker" <<'EOF'
#!/bin/sh
# Faux docker de test : `docker exec [-i] -e PGPASSWORD ciphera-postgres
# pg_dump|psql|pg_isready ...` devient cet outil sur la base de la
# distribution (TCP local, mot de passe dans PGPASSWORD).
[ "$1" = exec ] || exit 1
shift
if [ "$1" = -i ]; then shift; fi
[ "$1 $2 $3" = "-e PGPASSWORD ciphera-postgres" ] || exit 1
shift 3
case "$1" in
pg_dump | psql | pg_isready) ;;
*) exit 1 ;;
esac
tool=$1
shift
exec "$tool" --host=127.0.0.1 "$@"
EOF
chmod 0755 "$work/docker"

# --- execution par distribution ---------------------------------------------
port=9443
failures=0

check() { # description, commande...
	local what=$1
	shift
	if "$@"; then
		printf '  ok   %s\n' "$what"
	else
		printf '  FAIL %s\n' "$what"
		failures=$((failures + 1))
	fi
}

in_ctr() { podman exec "$cid" sh -c "$1"; }

test_env() {
	printf '%s\n' CIPHERA_INSTALL_TEST=1 \
		"CIPHERA_RELEASE_BASE_URL=https://localhost:$port/good" \
		"CIPHERA_TOOLS_BASE_URL=https://localhost:$port/tools" \
		"CIPHERA_TEST_MINISIGN_PUBKEY=$good_pub" \
		CURL_CA_BUNDLE=/run/ciphera-test-tls/server.crt
}

installed() { in_ctr 'cat /usr/local/share/ciphera/VERSION'; }
bin_sums() { in_ctr 'sha256sum /usr/local/bin/ciphera /usr/local/bin/ciphera-agent /usr/local/bin/ciphera-proxy /usr/local/bin/ciphera-cli'; }
backups() { in_ctr 'ls /var/backups/ciphera 2>/dev/null | grep -c "\.sql\.zst$" || true'; }

healthy() {
	local _
	for _ in $(seq 1 60); do
		if in_ctr 'curl -fs http://127.0.0.1:8444/api/health >/dev/null 2>&1'; then
			return 0
		fi
		sleep 2
	done
	return 1
}

# ciphera-cli updater en root avec l'environnement de test ; sortie dans <fichier>.
manual() { # <fichier> <arguments...>
	local out=$1
	shift
	local envs=()
	while read -r e; do envs+=(-e "$e"); done < <(test_env)
	podman exec "${envs[@]}" "$cid" /usr/local/bin/ciphera-cli updater "$@" >"$out" 2>&1
}

# Demande signee deposee comme le plan de controle : HMAC-SHA256 sous la
# cle derivee de l'identite age (HMAC de "ciphera/update-request/v1"), JSON
# ecrit par le compte ciphera puis renomme. <cle> : "instance" ou "autre".
request() { # <version> <instance|autre>
	podman exec -e "V=$1" -e "WHICH=$2" "$cid" sh -c '
		set -eu
		if [ "$WHICH" = instance ]; then
			id=$(grep -v "^#" /etc/ciphera/age.key | head -n 1)
		else
			id=AGE-SECRET-KEY-1AUTRECLEDETESTQUINESTPASCELLEDELINSTANCE
		fi
		k=$(printf %s "ciphera/update-request/v1" | openssl dgst -sha256 -mac HMAC -macopt "key:$id" -r | cut -d" " -f1)
		ts=$(date +%s)
		mac=$(printf "ciphera-update-request\n%s\n%s" "$V" "$ts" | openssl dgst -sha256 -mac HMAC -macopt "hexkey:$k" -r | cut -d" " -f1)
		printf "{\"version\":\"%s\",\"requested_at\":%s,\"hmac\":\"%s\"}" "$V" "$ts" "$mac" |
			runuser -u ciphera -- sh -c "umask 077 && cat >/var/lib/ciphera/.update-request.json.tmp && mv /var/lib/ciphera/.update-request.json.tmp /var/lib/ciphera/update-request.json"'
}

# Attend la fin du passage de l'updater declenche par la demande.
wait_updater() {
	local _ state taken=no
	for _ in $(seq 1 30); do
		if in_ctr 'test ! -e /var/lib/ciphera/update-request.json'; then
			taken=yes
			break
		fi
		sleep 1
	done
	[ "$taken" = yes ] || return 1
	for _ in $(seq 1 180); do
		state=$(in_ctr 'systemctl show -p ActiveState --value ciphera-updater.service')
		case "$state" in activating | active | reloading) ;; *) return 0 ;; esac
		sleep 1
	done
	return 1
}

# Table creee par le `ciphera` de v_C (comme une migration) : t ou f.
vc_table() { in_ctr "runuser -u postgres -- psql -d ciphera -tAc \"SELECT to_regclass('public.migration_vc') IS NOT NULL\""; }
no_vc_table() { [ "$(vc_table)" = f ]; }

updater_log() { in_ctr 'journalctl -u ciphera-updater.service --no-pager -o cat'; }
# grep lit tout (pas de -q) : pas de SIGPIPE sous pipefail.
log_has() { updater_log | grep -- "$1" >/dev/null; }

operator_unit_kept() {
	in_ctr 'test -f /etc/systemd/system/ciphera-operator.service && test "$(systemctl is-enabled ciphera-operator.service)" = enabled && test "$(systemctl is-active ciphera-operator.service)" = active'
}
agent_disabled() { in_ctr 'test "$(systemctl is-enabled ciphera-agent.service)" = disabled'; }

run_distro() {
	local distro=$1
	local image="ciphera-install-test-$distro" out envs
	printf '== %s\n' "$distro"
	podman build -q -t "$image" -f "$here/Containerfile.$distro" "$here" >/dev/null
	cid=$(podman run -d --systemd=always --name "ciphera-update-test-$distro-$$" \
		-v "$work:/test:ro,Z" "$image")
	containers+=("$cid")

	local state
	for _ in $(seq 1 60); do
		state=$(in_ctr 'systemctl is-system-running 2>/dev/null' || true)
		case "$state" in running | degraded) break ;; esac
		sleep 1
	done
	podman exec -d "$cid" sh /test/serve-release.sh /test/srv "$port" /run/ciphera-test-tls >/dev/null
	for _ in $(seq 1 30); do
		in_ctr "test -s /run/ciphera-test-tls/ready" && break
		sleep 1
	done

	# Installation de v_A, base de test (comme run.sh).
	out="$work/$distro-install.log"
	mapfile -t envs < <(test_env)
	local e_args=()
	for e in "${envs[@]}"; do e_args+=(-e "$e"); done
	if ! podman exec "${e_args[@]}" "$cid" sh -c 'sh -s -- --non-interactive </test/install.sh' >"$out" 2>&1; then
		cat "$out"
		die "installation de $v_a impossible ($distro)"
	fi
	in_ctr 'pw=$(cat /etc/ciphera/db_password) && printf "%s\n" "CREATE ROLE ciphera LOGIN PASSWORD :'"'"'pw'"'"';" "CREATE DATABASE ciphera OWNER ciphera;" | runuser -u postgres -- psql -q -v ON_ERROR_STOP=1 -v pw="$pw" >/dev/null'
	in_ctr 'umask 077 && printf "CIPHERA_DB_SOCKET_DIR=/var/run/postgresql\n" >/etc/ciphera/ciphera.env && systemctl restart ciphera'
	check "$v_a installee et joignable" healthy
	check "version installee notee ($v_a)" test "$(installed)" = "$v_a"
	check "ciphera-updater.path actif" in_ctr 'test "$(systemctl is-active ciphera-updater.path)" = active'
	check "configuration de l'updater root 0600" \
		in_ctr 'test "$(stat -c %a:%U:%G /etc/ciphera-updater/updater.env)" = 600:root:root && test "$(stat -c %a:%U /etc/ciphera-updater)" = 755:root'
	# L'updater lance docker par son chemin absolu (dossiers systeme).
	in_ctr 'install -m 0755 /test/docker /usr/bin/docker'
	# Piege dans le fichier d'environnement du plan de controle (modifiable
	# par le compte ciphera) : sans effet sur l'updater (cas d a g).
	in_ctr 'mkdir -p /var/lib/ciphera/piege && for t in docker curl systemctl systemd-tmpfiles psql; do printf "#!/bin/sh\ntouch /var/lib/ciphera/piege/execute\nexit 1\n" >/var/lib/ciphera/piege/$t && chmod 0755 /var/lib/ciphera/piege/$t; done && printf "PATH=/var/lib/ciphera/piege\nLD_PRELOAD=/var/lib/ciphera/piege/piege.so\n" >>/etc/ciphera/ciphera.env'
	# Surcharges de test de l'unite (origine, cle et CA de test).
	in_ctr 'mkdir -p /etc/systemd/system/ciphera-updater.service.d'
	test_env | sed 's/^/Environment=/' | { printf '[Service]\n'; cat; } |
		podman exec -i "$cid" sh -c 'cat >/etc/systemd/system/ciphera-updater.service.d/test.conf && systemctl daemon-reload'
	local sums_a
	sums_a=$(bin_sums)

	# (a) HMAC faux.
	request "$v_b" autre
	check "(a) updater lance par l'unite .path" wait_updater
	check "(a) demande au HMAC faux ignoree" log_has 'HMAC de la demande invalide'
	check "(a) rien ne change" test "$(bin_sums)" = "$sums_a"
	check "(a) version inchangee" test "$(installed)" = "$v_a"
	check "(a) aucune sauvegarde" test "$(backups)" = 0

	# (b) version deja installee.
	request "$v_a" instance
	check "(b) updater lance" wait_updater
	check "(b) version egale refusee" log_has "n'est pas superieure"
	check "(b) rien ne change" test "$(bin_sums)" = "$sums_a"

	# (c) release signee par une autre cle.
	out="$work/$distro-c.log"
	if manual "$out" --version "$v_other"; then
		check "(c) signature d'une autre cle : echec" false
	else
		check "(c) signature d'une autre cle : echec" grep -q 'signature minisign de SHA256SUMS invalide' "$out"
	fi
	check "(c) rien ne change" test "$(bin_sums)" = "$sums_a"
	check "(c) aucune sauvegarde" test "$(backups)" = 0

	# Unite de l'administrateur (hors registre) et unite de Ciphera qu'il a
	# desactivee : jamais touchees par l'updater (cas d a f).
	check "registre des unites ecrit par install.sh" \
		in_ctr 'grep -qx ciphera.service /usr/local/share/ciphera/units.list && ! grep -q ciphera-test-extra /usr/local/share/ciphera/units.list'
	in_ctr 'printf "[Unit]\nDescription=Unite de test de l administrateur\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/usr/bin/true\n\n[Install]\nWantedBy=multi-user.target\n" >/etc/systemd/system/ciphera-operator.service && systemctl daemon-reload && systemctl enable --now ciphera-operator.service >/dev/null 2>&1 && systemctl disable ciphera-agent.service >/dev/null 2>&1'

	# (d) mise a jour valide par le panneau (demande + unite .path).
	request "$v_b" instance
	check "(d) updater lance" wait_updater
	check "(d) $v_b installee" test "$(installed)" = "$v_b"
	check "(d) sauvegarde zstd de la base" \
		in_ctr 'f=$(ls /var/backups/ciphera/*.sql.zst | head -n 1) && test -s "$f" && test "$(od -An -tx1 -N4 "$f" | tr -d " ")" = 28b52ffd && test "$(stat -c %a:%U "$f")" = 600:root'
	check "(d) dossier des sauvegardes root 0700" in_ctr 'test "$(stat -c %a:%U /var/backups/ciphera)" = 700:root'
	check "(d) outils du manifeste en place" \
		in_ctr 'test -x /usr/local/lib/ciphera/bin/railpack && test -x /usr/local/lib/ciphera/bin/buildctl && ls /usr/local/lib/ciphera/bin/mise-* >/dev/null'
	check "(d) panneau joignable" healthy
	check "(d) reprise tiree par le redemarrage : rendue aussitot (verrou tenu)" \
		in_ctr 'journalctl -u ciphera-updater-recover.service --no-pager -o cat | grep "verrou tenu" >/dev/null'
	check "(d) aucun fichier temporaire laisse" in_ctr '! ls -a /usr/local/bin | grep -q ciphera-new'
	check "(d) unite ajoutee par $v_b installee" in_ctr 'test -f /etc/systemd/system/ciphera-test-extra.service'
	check "(d) unite ajoutee activee et demarree" \
		in_ctr 'test "$(systemctl is-enabled ciphera-test-extra.service)" = enabled && test "$(systemctl is-active ciphera-test-extra.service)" = active'
	check "(d) post-installation lancee" log_has 'post-installation'
	check "(d) registre des unites de $v_b" \
		in_ctr 'grep -qx ciphera-test-extra.service /usr/local/share/ciphera/units.list && ! grep -q ciphera-operator /usr/local/share/ciphera/units.list'
	check "(d) unite de l'administrateur gardee et active" operator_unit_kept
	check "(d) unite desactivee par l'administrateur laissee desactivee" agent_disabled
	if [ "$(installed)" != "$v_b" ]; then updater_log | tail -n 30; fi
	local sums_b
	sums_b=$(bin_sums)

	# Sauvegarde prise sous v_A (premiere, cas d).
	local dump_a
	dump_a=$(in_ctr 'ls /var/backups/ciphera/*.sql.zst | head -n 1')

	# (e) v_C modifie la base puis ne demarre pas : retour a v_B et a la base.
	out="$work/$distro-e.log"
	if manual "$out" --version "$v_c"; then
		check "(e) panneau muet : echec" false
	else
		check "(e) panneau muet : echec" grep -q 'retour a la version precedente' "$out"
	fi
	check "(e) binaires de $v_b remis" test "$(bin_sums)" = "$sums_b"
	check "(e) version $v_b remise" test "$(installed)" = "$v_b"
	check "(e) base restauree" grep -q 'base restauree depuis' "$out"
	check "(e) modification de v_C disparue" no_vc_table
	check "(e) panneau de nouveau joignable" healthy
	check "(e) unite de $v_b retiree par v_C remise et activee" \
		in_ctr 'test -f /etc/systemd/system/ciphera-test-extra.service && test "$(systemctl is-enabled ciphera-test-extra.service)" = enabled && test "$(systemctl is-active ciphera-test-extra.service)" = active'
	check "(e) registre de $v_b remis" \
		in_ctr 'grep -qx ciphera-test-extra.service /usr/local/share/ciphera/units.list'
	check "(e) unite de l'administrateur gardee et active" operator_unit_kept
	check "(e) unite desactivee par l'administrateur laissee desactivee" agent_disabled

	# (f) retour arriere : root, --allow-downgrade et --restore-db seulement.
	out="$work/$distro-f.log"
	if manual "$out" --version "$v_a"; then
		check "(f) sans --allow-downgrade : refus" false
	else
		check "(f) sans --allow-downgrade : refus" grep -q "n'est pas superieure" "$out"
	fi
	if manual "$out" --version "$v_a" --allow-downgrade; then
		check "(f) sans --restore-db : refus" false
	else
		check "(f) sans --restore-db : refus" grep -q -- '--restore-db' "$out"
	fi
	if in_ctr "runuser -u ciphera -- /usr/local/bin/ciphera-cli updater --version $v_a --allow-downgrade --restore-db $dump_a" >"$out" 2>&1; then
		check "(f) hors root : refus" false
	else
		check "(f) hors root : refus" grep -q 'exige root' "$out"
	fi
	if manual "$out" --version "$v_a" --allow-downgrade --restore-db "$dump_a"; then
		check "(f) --allow-downgrade en root : $v_a remise" test "$(installed)" = "$v_a"
	else
		check "(f) --allow-downgrade en root : $v_a remise" false
		cat "$out"
	fi
	check "(f) panneau joignable" healthy
	check "(f) unite de $v_b desactivee et retiree" \
		in_ctr 'test ! -e /etc/systemd/system/ciphera-test-extra.service && test ! -e /etc/systemd/system/multi-user.target.wants/ciphera-test-extra.service'
	check "(f) unite de l'administrateur gardee et active" operator_unit_kept
	check "(f) unite desactivee par l'administrateur laissee desactivee" agent_disabled
	in_ctr 'systemctl disable --now ciphera-operator.service >/dev/null 2>&1; rm -f /etc/systemd/system/ciphera-operator.service; systemctl enable ciphera-agent.service >/dev/null 2>&1; systemctl daemon-reload'

	# (g) arret brutal de l'updater pendant la mise a jour vers v_C (attente
	# du panneau), puis demarrage : reprise avant les services.
	check "(g) reprise au demarrage activee" \
		in_ctr 'test "$(systemctl is-enabled ciphera-updater-recover.service)" = enabled'
	local sums_a2
	sums_a2=$(bin_sums)
	local envs_g=()
	while read -r e; do envs_g+=(-e "$e"); done < <(test_env)
	podman exec -d "${envs_g[@]}" "$cid" /usr/local/bin/ciphera-cli updater --version "$v_c"
	local _
	for _ in $(seq 1 120); do
		[ "$(installed)" = "$v_c" ] && [ "$(vc_table)" = t ] && break
		sleep 1
	done
	# [c] : le motif ne correspond pas a la ligne de commande de ce sh -c.
	in_ctr 'pkill -9 -f "[c]iphera-cli updater" || true'
	check "(g) journal laisse par l'arret brutal" in_ctr 'test -e /var/lib/ciphera-updater/journal.json'
	# Demarrage simule : services arretes, reprise, puis services.
	in_ctr 'systemctl stop ciphera.service && systemctl start ciphera-updater-recover.service && systemctl start ciphera.service'
	check "(g) binaires de $v_a remis" test "$(bin_sums)" = "$sums_a2"
	check "(g) version $v_a remise" test "$(installed)" = "$v_a"
	check "(g) journal retire" in_ctr 'test ! -e /var/lib/ciphera-updater/journal.json'
	check "(g) base restauree" no_vc_table
	check "(g) panneau joignable" healthy
	check "(h) PATH et LD_PRELOAD de ciphera.env sans effet sur l'updater" \
		in_ctr 'test ! -e /var/lib/ciphera/piege/execute'
}

for distro in "$@"; do
	run_distro "$distro"
done

if [ "$failures" -ne 0 ]; then
	printf '%s verification(s) en echec\n' "$failures"
	exit 1
fi
printf 'tous les cas passent\n'
