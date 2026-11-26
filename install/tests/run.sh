#!/usr/bin/env bash
# Tests de bout en bout de install/install.sh, dans des conteneurs Podman
# avec systemd (un conteneur par distribution).
#
#   install/tests/run.sh debian [ubuntu] [fedora]
#
# Pour chaque distribution :
#   - surcharges de test (URL, cle) refusees sans CIPHERA_INSTALL_TEST=1 ;
#   - (b) archive alteree d'un octet : echec, rien d'installe ;
#   - (c) SHA256SUMS.minisig signe par une autre cle : echec, rien d'installe ;
#   - (f) archive railpack alteree sur le miroir des outils : echec, rien
#     d'installe ;
#   - (e) compte ciphera-build existant d'uid 1000 : refus avant tout
#     telechargement, rien d'installe ;
#   - (h) compte ciphera existant avec un shell de connexion, (i) compte
#     ciphera-agent de groupe principal autre : refus avant tout
#     telechargement, rien d'installe ;
#   - (g) compte ciphera-postgres existant de groupe principal 0 (root) :
#     refus avant tout telechargement, rien d'installe ;
#   - (a) release valide : succes, `Empreinte SHA-256 :` affichee,
#     ciphera.service actif une fois la base de test creee ; compte systeme
#     ciphera-build avec ses plages de sous-uids/sous-gids sans chevauchement ;
#     reglage du filtrage de sortie (/etc/ciphera-egress/egress.env, root,
#     defaut 0) et minuterie ciphera-egress-check.timer active ; cle de la
#     CA a root (0600), signataire ciphera-certsign.path actif et
#     ciphera-certsign.service passe a l'installation ;
#   - unites activees : aucun cycle d'ordre (systemd-analyze verify), puis vrai
#     redemarrage du conteneur (sockets.target et paths.target actifs) ;
#   - (r) relance sur l'installation : meme version = reparation (services
#     relances), version superieure ou inferieure refusee ;
#   - (d) --purge retire aussi ce compte et ses plages ; reinstallation.
#
# Exige : Linux, podman, tar, sha256sum, curl (outils tiers telecharges une
# fois depuis GitHub sur l'hote). minisign est pris sur l'hote s'il
# existe, sinon dans un conteneur Debian.
# CIPHERA_TEST_BIN_DIR : dossier des binaires musl (ciphera, ciphera-agent,
# ciphera-proxy, ciphera-cli) ; defaut target/<cible musl>/release.
#
# Les cles minisign sont jetables (une paire par execution) ; la base
# PostgreSQL de la distribution n'est qu'une fixture de test, jointe par son
# socket Unix (CIPHERA_DB_SOCKET_DIR dans /etc/ciphera/ciphera.env) : ces
# conteneurs n'ont pas Docker. Le chemin de production (PostgreSQL en
# conteneur epingle lance par l'agent, Docker reel) est teste par
# install/tests/run-docker-host.sh (job CI install-docker).

# Les commandes entre apostrophes sont evaluees dans le conteneur.
# shellcheck disable=SC2016
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
installer="$root/install/install.sh"

die() {
	printf 'run.sh : %s\n' "$*" >&2
	exit 1
}

[ "${CIPHERA_DESTRUCTIVE_HOST_TEST:-}" = 1 ] ||
	die "lance des conteneurs en root sur la machine : relancer avec CIPHERA_DESTRUCTIVE_HOST_TEST=1 (machine jetable seulement)"

[ "$#" -gt 0 ] || die "usage : run.sh debian [ubuntu] [fedora]"
for d in "$@"; do
	case "$d" in
	debian | ubuntu | fedora) ;;
	*) die "distribution inconnue : $d" ;;
	esac
done
[ -f "$installer" ] || die "install/install.sh absent"
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
version=$(sed -n "s/^DEFAULT_VERSION='\(v[0-9.]*\)'\$/\1/p" "$installer")
[ -n "$version" ] || die "DEFAULT_VERSION introuvable dans install.sh"

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
	# debian:12 epinglee par digest, comme install/tests/Containerfile.debian.
	printf 'FROM docker.io/library/debian@sha256:1d04200c2ece1d4d75a2a6f8e11ebce7d86a2ae28b34bb2c9cbb3ee9f4953e75\nRUN apt-get update && apt-get install -y --no-install-recommends minisign && rm -rf /var/lib/apt/lists/*\n' |
		podman build -q -t "$signer_image" -f - "$work" >/dev/null
	msign() { podman run --rm -v "$work:/w:Z" -w /w "$signer_image" minisign "$@"; }
fi

# --- release de test --------------------------------------------------------
mkdir -p "$work/keys"
msign -G -W -p keys/good.pub -s keys/good.key >/dev/null
msign -G -W -p keys/other.pub -s keys/other.key >/dev/null
good_pub=$(sed -n 2p "$work/keys/good.pub")

archive="ciphera-$version-$target.tar.gz"
release_dir() { printf '%s/srv/%s/releases/%s' "$work" "$1" "$version"; }

good=$(release_dir good)
mkdir -p "$good"
sh "$root/install/package.sh" "$version" "$target" "$bin_dir" "$good"
(cd "$good" && sha256sum "$archive" >SHA256SUMS)
msign -S -s keys/good.key -m "srv/good/releases/$version/SHA256SUMS" >/dev/null

# (b) un octet de l'archive change, SHA256SUMS et signature intacts.
tampered=$(release_dir tampered)
mkdir -p "$tampered"
cp "$good/$archive" "$good/SHA256SUMS" "$good/SHA256SUMS.minisig" "$tampered/"
offset=200
byte=$(od -An -tx1 -j "$offset" -N1 "$tampered/$archive" | tr -d ' ')
if [ "$byte" = ff ]; then new='\000'; else new='\377'; fi
# shellcheck disable=SC2059 # octet en echappement octal voulu
printf "$new" | dd of="$tampered/$archive" bs=1 seek="$offset" conv=notrunc status=none
cmp -s "$good/$archive" "$tampered/$archive" && die "alteration de l'archive ratee"

# (c) SHA256SUMS signe par une autre cle.
otherkey=$(release_dir otherkey)
mkdir -p "$otherkey"
cp "$good/$archive" "$good/SHA256SUMS" "$otherkey/"
msign -S -s keys/other.key -m "srv/otherkey/releases/$version/SHA256SUMS" >/dev/null

# --- outils tiers (railpack, buildctl, mise) ----------------------------------
# Telecharges une fois sur l'hote depuis GitHub (versions et SHA-256 lus dans
# install.sh, verifies ici), puis servis par le serveur de test sous
# srv/tools/<meme chemin que github.com> : install.sh les lit par
# CIPHERA_TOOLS_BASE_URL (mode test) et reverifie ses SHA-256 epingles. Les
# conteneurs ne joignent pas GitHub : CURL_CA_BUNDLE n'y contient que le
# certificat du serveur de test.
pin() { sed -n "s/^$1='\(.*\)'\$/\1/p" "$installer"; }
case "$target" in
x86_64-*) rp_arch=x86_64 bk_arch=amd64 mise_arch=x64 sum_arch=AMD64 ;;
*) rp_arch=arm64 bk_arch=arm64 mise_arch=arm64 sum_arch=ARM64 ;;
esac
rp_version=$(pin RAILPACK_VERSION)
bk_version=$(pin BUILDKIT_VERSION)
mise_version=$(pin MISE_VERSION)
tools=(
	"railwayapp/railpack/releases/download/$rp_version/railpack-$rp_version-$rp_arch-unknown-linux-musl.tar.gz $(pin "RAILPACK_SHA256_$sum_arch")"
	"moby/buildkit/releases/download/$bk_version/buildkit-$bk_version.linux-$bk_arch.tar.gz $(pin "BUILDKIT_SHA256_$sum_arch")"
	"jdx/mise/releases/download/v$mise_version/mise-v$mise_version-linux-$mise_arch-musl.tar.gz $(pin "MISE_SHA256_$sum_arch")"
)
for entry in "${tools[@]}"; do
	path=${entry% *}
	sum=${entry##* }
	case "$sum" in
	[0-9a-f]*) [ "${#sum}" -eq 64 ] || die "SHA-256 de $path introuvable dans install.sh" ;;
	*) die "SHA-256 de $path introuvable dans install.sh" ;;
	esac
	dest="$work/srv/tools/$path"
	mkdir -p "$(dirname "$dest")"
	curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$dest" "https://github.com/$path" ||
		die "telechargement impossible : https://github.com/$path"
	printf '%s  %s\n' "$sum" "$dest" | sha256sum -c --status - ||
		die "SHA-256 de $path incorrect"
done

# (f) miroir dont l'archive railpack est alteree d'un octet.
cp -R "$work/srv/tools" "$work/srv/tools-tampered"
rp_tampered="$work/srv/tools-tampered/${tools[0]% *}"
byte=$(od -An -tx1 -j "$offset" -N1 "$rp_tampered" | tr -d ' ')
if [ "$byte" = ff ]; then new='\000'; else new='\377'; fi
# shellcheck disable=SC2059 # octet en echappement octal voulu
printf "$new" | dd of="$rp_tampered" bs=1 seek="$offset" conv=notrunc status=none
cmp -s "$work/srv/tools/${tools[0]% *}" "$rp_tampered" && die "alteration de l'archive railpack ratee"

cp "$installer" "$root/install/uninstall.sh" "$here/serve-release.sh" "$work/"
rm -f "$work/keys/"*.key

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

nothing_installed() {
	in_ctr 'test ! -e /usr/local/bin/ciphera && test ! -e /usr/local/lib/ciphera && test ! -e /etc/ciphera && test ! -e /etc/ciphera-agent && test ! -e /etc/systemd/system/ciphera.service && test ! -e /etc/tmpfiles.d/ciphera.conf'
}

# Cree ciphera-idtest (uid et gid) dans la premiere plage de
# sous-identifiants libre du conteneur, qu'install.sh doit eviter ; affiche
# cet id.
plant_id_in_candidate_range() {
	local c
	c=$(in_ctr 'cat /etc/subuid /etc/subgid 2>/dev/null || true' | awk -F: '
		NF >= 3 && $2 ~ /^[0-9]+$/ && $3 ~ /^[0-9]+$/ { e = $2 + $3; if (e > m) m = e }
		END { if (m < 100000) m = 100000; printf "%.0f\n", m + 100 }')
	in_ctr "groupadd -g $c ciphera-idtest && useradd -M -u $c -g ciphera-idtest -s /usr/sbin/nologin ciphera-idtest" >/dev/null 2>&1
	printf '%s\n' "$c"
}

# La plage de ciphera-build de <fichier> commence apres <id> et ne contient
# aucun id de la base getent <passwd|group> du conteneur.
range_avoids_ids() { # <fichier> <passwd|group> <id>
	local start count
	IFS=: read -r _ start count < <(in_ctr "grep '^ciphera-build:' $1 | head -n 1")
	[ -n "$start" ] && [ "$start" -gt "$3" ] &&
		in_ctr "getent $2" | awk -F: -v s="$start" -v e="$((start + count))" '$3 >= s && $3 < e { bad = 1 } END { exit bad }'
}

no_build_subids() {
	! in_ctr 'grep -qs "^ciphera-build:" /etc/subuid /etc/subgid'
}

# Contenu de /etc/subuid puis /etc/subgid du conteneur (vide si absents).
subids_snapshot() {
	in_ctr 'for f in /etc/subuid /etc/subgid; do echo "== $f"; cat "$f" 2>/dev/null || true; done'
}

# Une seule plage de ciphera-build dans <fichier> du conteneur, de 65536
# ids, sans chevauchement avec les autres plages.
build_subids_ok() { # <fichier>
	in_ctr "cat $1" | awk -F: -v me=ciphera-build '
		NF >= 3 { s[NR] = $2; e[NR] = $2 + $3; if ($1 == me) { n++; mine = NR } }
		END {
			if (n != 1 || e[mine] - s[mine] != 65536) exit 1
			for (i = 1; i <= NR; i++)
				if ((i in s) && i != mine && s[i] < e[mine] && s[mine] < e[i]) exit 1
		}'
}

# install <fichier de sortie> <env...> : lance install.sh comme `curl | sh`.
install() {
	local out=$1
	shift
	local envs=()
	for e in "$@"; do envs+=(-e "$e"); done
	podman exec "${envs[@]}" "$cid" sh -c 'sh -s -- --non-interactive </test/install.sh' >"$out" 2>&1
}

test_env() { # <cas> [miroir des outils, defaut tools]
	printf '%s\n' CIPHERA_INSTALL_TEST=1 \
		"CIPHERA_RELEASE_BASE_URL=https://localhost:$port/$1" \
		"CIPHERA_TOOLS_BASE_URL=https://localhost:$port/${2:-tools}" \
		"CIPHERA_TEST_MINISIGN_PUBKEY=$good_pub" \
		CURL_CA_BUNDLE=/run/ciphera-test-tls/server.crt
}

# Attend la fin du demarrage de systemd dans le conteneur.
wait_boot() {
	local state
	for _ in $(seq 1 60); do
		state=$(in_ctr 'systemctl is-system-running 2>/dev/null' || true)
		case "$state" in running | degraded) break ;; esac
		sleep 1
	done
}

# Serveur HTTPS de la release de test (certificat jetable dans /run).
start_release_server() {
	in_ctr 'rm -f /run/ciphera-test-tls/ready'
	podman exec -d "$cid" sh /test/serve-release.sh /test/srv "$port" /run/ciphera-test-tls >/dev/null
	for _ in $(seq 1 30); do
		in_ctr "test -s /run/ciphera-test-tls/ready" && break
		sleep 1
	done
}

# Aucun cycle d'ordre entre les unites installees et les cibles du
# demarrage (systemd casserait le cycle en supprimant un job, par exemple
# sockets.target ou paths.target, a chaque demarrage).
units_without_cycle() {
	in_ctr 'out=$(systemd-analyze verify multi-user.target /etc/systemd/system/ciphera*.service /etc/systemd/system/ciphera*.socket /etc/systemd/system/ciphera*.path /etc/systemd/system/ciphera*.timer 2>&1 || true); if printf "%s\n" "$out" | grep -qi "ordering cycle"; then printf "%s\n" "$out" >&2; false; fi'
}

# Apres un vrai redemarrage du conteneur : sockets.target et paths.target
# actives, aucun cycle casse dans le journal du demarrage.
boot_without_cycle() {
	in_ctr 'test "$(systemctl is-active sockets.target)" = active && test "$(systemctl is-active paths.target)" = active && ! journalctl -b --no-pager | grep -qi "ordering cycle" || { journalctl -b --no-pager | grep -i cycle >&2; false; }'
}

run_distro() {
	local distro=$1
	local image="ciphera-install-test-$distro" out
	printf '== %s\n' "$distro"
	podman build -q -t "$image" -f "$here/Containerfile.$distro" "$here" >/dev/null
	cid=$(podman run -d --systemd=always --name "ciphera-install-test-$distro-$$" \
		-v "$work:/test:ro,Z" "$image")
	containers+=("$cid")

	wait_boot
	start_release_server

	local envs
	out="$work/$distro-refus.log"
	if install "$out" "CIPHERA_RELEASE_BASE_URL=https://localhost:$port/good"; then
		check "URL de release refusee sans CIPHERA_INSTALL_TEST=1" false
	else
		check "URL de release refusee sans CIPHERA_INSTALL_TEST=1" grep -q CIPHERA_RELEASE_BASE_URL "$out"
	fi
	if install "$out" "CIPHERA_TEST_MINISIGN_PUBKEY=$good_pub"; then
		check "cle de test refusee sans CIPHERA_INSTALL_TEST=1" false
	else
		check "cle de test refusee sans CIPHERA_INSTALL_TEST=1" grep -q CIPHERA_TEST_MINISIGN_PUBKEY "$out"
	fi
	if install "$out"; then
		check "install.sh du depot (cle non injectee) refuse" false
	else
		check "install.sh du depot (cle non injectee) refuse" grep -q "cle publique des releases absente" "$out"
	fi
	if install "$out" "CIPHERA_TOOLS_BASE_URL=https://localhost:$port/tools"; then
		check "miroir des outils refuse sans CIPHERA_INSTALL_TEST=1" false
	else
		check "miroir des outils refuse sans CIPHERA_INSTALL_TEST=1" grep -q CIPHERA_TOOLS_BASE_URL "$out"
	fi
	check "rien d'installe apres les refus" nothing_installed

	out="$work/$distro-b.log"
	mapfile -t envs < <(test_env tampered)
	if install "$out" "${envs[@]}"; then
		check "(b) archive alteree : echec" false
	else
		check "(b) archive alteree : echec" true
	fi
	check "(b) motif : SHA-256 incorrect" grep -q 'SHA-256 de .* incorrect' "$out"
	check "(b) rien d'installe" nothing_installed

	out="$work/$distro-c.log"
	mapfile -t envs < <(test_env otherkey)
	if install "$out" "${envs[@]}"; then
		check "(c) signature d'une autre cle : echec" false
	else
		check "(c) signature d'une autre cle : echec" true
	fi
	check "(c) motif : signature invalide" grep -q 'signature minisign de SHA256SUMS invalide' "$out"
	check "(c) rien d'installe" nothing_installed

	# (f) outil tiers altere sur le miroir : SHA-256 epingle refuse.
	out="$work/$distro-f.log"
	mapfile -t envs < <(test_env good tools-tampered)
	if install "$out" "${envs[@]}"; then
		check "(f) outil tiers altere : echec" false
	else
		check "(f) outil tiers altere : echec" true
	fi
	check "(f) motif : SHA-256 de l'archive railpack incorrect" grep -q 'SHA-256 de railpack-.* incorrect' "$out"
	check "(f) rien d'installe" nothing_installed

	# (e) ciphera-build existe deja avec l'uid 1000 (celui du premier compte
	# humain) : refus avant toute installation de fichier ou de plage.
	# useradd attribue lui-meme une plage de sous-ids a ce compte non systeme
	# (SUB_UID_* de login.defs) : on verifie qu'install.sh n'en ecrit aucune
	# autre, en comparant /etc/subuid et /etc/subgid avant et apres.
	in_ctr 'useradd -o -u 1000 -M -s /usr/sbin/nologin ciphera-build'
	subids_before=$(subids_snapshot)
	out="$work/$distro-e.log"
	mapfile -t envs < <(test_env good)
	if install "$out" "${envs[@]}"; then
		check "(e) ciphera-build d'uid 1000 : echec" false
	else
		check "(e) ciphera-build d'uid 1000 : echec" true
	fi
	check "(e) motif : compte systeme exige" grep -q "buildkitd exige un compte systeme" "$out"
	check "(e) rien d'installe" nothing_installed
	check "(e) aucune plage de sous-uids ni sous-gids ecrite par install.sh" \
		test "$(subids_snapshot)" = "$subids_before"
	in_ctr 'userdel ciphera-build'

	# (g) ciphera-postgres existe deja avec le groupe principal root (gid 0) :
	# la base tournerait en root ; refus avant toute installation.
	in_ctr 'useradd -r -M -g 0 -s /usr/sbin/nologin ciphera-postgres'
	out="$work/$distro-g.log"
	mapfile -t envs < <(test_env good)
	if install "$out" "${envs[@]}"; then
		check "(g) ciphera-postgres de groupe root : echec" false
	else
		check "(g) ciphera-postgres de groupe root : echec" true
	fi
	check "(g) motif : root refuse pour PostgreSQL" grep -q "root refuse pour PostgreSQL" "$out"
	check "(g) rien d'installe" nothing_installed
	in_ctr 'userdel ciphera-postgres'

	# (h) compte ciphera existant avec un shell de connexion : refuse avant
	# tout telechargement (un compte ouvert heriterait des droits de Ciphera).
	in_ctr 'useradd -r -M -s /bin/bash ciphera'
	out="$work/$distro-h.log"
	mapfile -t envs < <(test_env good)
	if install "$out" "${envs[@]}"; then
		check "(h) ciphera avec un shell de connexion : echec" false
	else
		check "(h) ciphera avec un shell de connexion : echec" true
	fi
	check "(h) motif : shell de connexion" grep -q "shell de connexion" "$out"
	check "(h) rien d'installe" nothing_installed
	in_ctr 'userdel ciphera'

	# (i) compte ciphera-agent existant de groupe principal autre (users) :
	# refuse de meme.
	in_ctr 'useradd -r -M -g users -s /usr/sbin/nologin ciphera-agent'
	out="$work/$distro-i.log"
	mapfile -t envs < <(test_env good)
	if install "$out" "${envs[@]}"; then
		check "(i) ciphera-agent de groupe users : echec" false
	else
		check "(i) ciphera-agent de groupe users : echec" true
	fi
	check "(i) motif : groupe principal" grep -q "le groupe ciphera-agent est exige" "$out"
	check "(i) rien d'installe" nothing_installed
	in_ctr 'userdel ciphera-agent'

	local planted
	planted=$(plant_id_in_candidate_range)

	out="$work/$distro-a.log"
	mapfile -t envs < <(test_env good)
	if install "$out" "${envs[@]}"; then
		check "(a) release valide : succes" true
	else
		check "(a) release valide : succes" false
		cat "$out"
	fi
	check "(a) empreinte affichee" grep -q 'Empreinte SHA-256 :' "$out"
	check "(a) Docker absent signale" grep -q 'Docker' "$out"
	check "(a) db_password en 0600 pour ciphera" \
		in_ctr 'test "$(stat -c %a:%U /etc/ciphera/db_password)" = 600:ciphera'
	check "(a) age.key en 0600 pour ciphera" \
		in_ctr 'test "$(stat -c %a:%U /etc/ciphera/age.key)" = 600:ciphera'
	check "(a) unite de l'agent installee" in_ctr 'test -f /etc/systemd/system/ciphera-agent.service'
	check "(a) cle de l'agent en 0600 pour ciphera-agent" \
		in_ctr 'test "$(stat -c %a:%U /etc/ciphera-agent/agent.key)" = 600:ciphera-agent'
	check "(a) copie du mot de passe en 0600 pour ciphera-postgres" \
		in_ctr 'test "$(stat -c %a:%U /etc/ciphera/postgres/db_password)" = 600:ciphera-postgres'
	# /run/ciphera en 0751 : l'agent (hors groupe ciphera) traverse jusqu'a
	# proxy.sock sans pouvoir lister ; postgres/ reste ferme (0750).
	check "(a) dossiers des sockets (tmpfiles)" \
		in_ctr 'pg=$(stat -c %a:%U:%G /run/ciphera/postgres); run=$(stat -c %a:%U:%G /run/ciphera); test "$pg" = 750:ciphera-postgres:ciphera && test "$run" = 751:root:ciphera || { echo "    observe : /run/ciphera/postgres=$pg (attendu 750:ciphera-postgres:ciphera), /run/ciphera=$run (attendu 751:root:ciphera)" >&2; false; }'
	# buildkitd : compte systeme dedie (jamais l'uid 1000), plages propres,
	# dossiers du socket et de l'etat a ce compte.
	check "(a) compte systeme ciphera-build (uid 1-999)" \
		in_ctr 'u=$(id -u ciphera-build) && test "$u" -gt 0 && test "$u" -lt 1000'
	check "(a) plage de sous-uids de ciphera-build (65536, sans chevauchement)" build_subids_ok /etc/subuid
	check "(a) plage de sous-gids de ciphera-build (65536, sans chevauchement)" build_subids_ok /etc/subgid
	check "(a) sous-uids hors des uids existants (ciphera-idtest evite)" range_avoids_ids /etc/subuid passwd "$planted"
	check "(a) sous-gids hors des gids existants (ciphera-idtest evite)" range_avoids_ids /etc/subgid group "$planted"
	check "(a) dossiers de buildkitd (tmpfiles)" \
		in_ctr 'bk=$(stat -c %a:%U:%G /run/ciphera-agent/buildkit); st=$(stat -c %a:%U:%G /var/lib/ciphera-buildkit); test "$bk" = 2770:ciphera-build:ciphera-agent && test "$st" = 700:ciphera-build:ciphera-build || { echo "    observe : buildkit=$bk state=$st" >&2; false; }'
	check "(a) agent non demarre sans Docker" \
		in_ctr 'test "$(systemctl is-active ciphera-agent)" != active'
	check "(a) reglage du filtrage de sortie : root 0644, plages privees permises" \
		in_ctr 'test "$(stat -c %a:%U:%G /etc/ciphera-egress/egress.env)" = 644:root:root && grep -qx CIPHERA_APP_EGRESS_DENY_PRIVATE=0 /etc/ciphera-egress/egress.env'
	check "(a) minuterie ciphera-egress-check.timer active" \
		in_ctr 'test "$(systemctl is-active ciphera-egress-check.timer)" = active'
	check "(a) cle de la CA root:root 0600" \
		in_ctr 'test "$(stat -c %a:%U:%G /etc/ciphera/ca/ca.key)" = 600:root:root'
	check "(a) dossier de la CA root:ciphera 0750, certificat root:ciphera 0640" \
		in_ctr 'test "$(stat -c %a:%U:%G /etc/ciphera/ca)" = 750:root:ciphera && test "$(stat -c %a:%U:%G /etc/ciphera/ca/ca.crt)" = 640:root:ciphera'
	check "(a) signataire : ciphera-certsign.path actif" \
		in_ctr 'test "$(systemctl is-active ciphera-certsign.path)" = active'
	check "(a) signataire : ciphera-certsign.service passe a l'installation" \
		in_ctr 'test "$(systemctl show -p Result --value ciphera-certsign.service)" = success'

	# Fixture : role et base avec le mot de passe genere par `ciphera init`,
	# joints par le socket de la distribution. Authentification par mot de
	# passe sur le socket, comme le conteneur de production
	# (`--auth-local=scram-sha-256`) : la regle `peer` des distributions
	# refuserait root (`ciphera admin`) pour le role ciphera.
	in_ctr 'pw=$(cat /etc/ciphera/db_password) && printf "%s\n" "CREATE ROLE ciphera LOGIN PASSWORD :'"'"'pw'"'"';" "CREATE DATABASE ciphera OWNER ciphera;" | runuser -u postgres -- psql -q -v ON_ERROR_STOP=1 -v pw="$pw" >/dev/null'
	in_ctr 'hba=$(runuser -u postgres -- psql -tAc "SHOW hba_file") && sed -i "1i local ciphera ciphera scram-sha-256" "$hba" && runuser -u postgres -- psql -qtAc "SELECT pg_reload_conf()" >/dev/null'
	in_ctr 'umask 077 && printf "CIPHERA_DB_SOCKET_DIR=/var/run/postgresql\n" >/etc/ciphera/ciphera.env && systemctl restart ciphera'

	local healthy=no
	for _ in $(seq 1 60); do
		if in_ctr 'curl -fsk https://127.0.0.1:8443/api/health >/dev/null 2>&1'; then
			healthy=yes
			break
		fi
		sleep 2
	done
	check "(a) panneau joignable" test "$healthy" = yes
	check "(a) ciphera.service actif" in_ctr 'test "$(systemctl is-active ciphera)" = active'
	# Commande de secours affichee par le panneau, lancee sans l'environnement
	# du service : unite + /etc/ciphera/ciphera.env (socket de la fixture).
	check "(a) ciphera admin clear-ip-allowlist (environnement vide)" \
		in_ctr 'out=$(env -i /usr/local/bin/ciphera admin clear-ip-allowlist 2>&1); rc=$?; printf "%s\n" "$out" | grep -q "liste d.IP autorisees" || { echo "    code $rc : $out" >&2; false; }'
	if [ "$healthy" != yes ]; then
		in_ctr 'journalctl -u ciphera --no-pager -n 30' || true
	fi

	# Unites activees : aucun cycle d'ordre, ni a la verification ni au
	# redemarrage reel de la machine.
	check "(a) systemd-analyze verify : aucun cycle" units_without_cycle
	podman restart "$cid" >/dev/null
	wait_boot
	check "(a) redemarrage : sockets.target et paths.target actifs, aucun cycle" boot_without_cycle
	check "(a) redemarrage : ciphera-proxy.socket actif" \
		in_ctr 'test "$(systemctl is-active ciphera-proxy.socket)" = active'
	start_release_server

	# (r) relance d'install.sh sur une installation existante : meme version =
	# reparation (services relances ensemble) ; autre version refusee avant
	# tout telechargement (montee de version par ciphera-cli updater, retour
	# arriere refuse).
	local pid_before
	pid_before=$(in_ctr 'systemctl show -p MainPID --value ciphera')
	out="$work/$distro-r.log"
	mapfile -t envs < <(test_env good)
	if install "$out" "${envs[@]}"; then
		check "(r) meme version : reparation reussie" grep -q 'reparation' "$out"
	else
		check "(r) meme version : reparation reussie" false
		cat "$out"
	fi
	check "(r) meme version : plan de controle relance" \
		in_ctr 'test "$(systemctl show -p MainPID --value ciphera)" != "'"$pid_before"'"'
	in_ctr 'cp /usr/local/share/ciphera/VERSION /root/VERSION.saved && echo v99.0.0 >/usr/local/share/ciphera/VERSION'
	out="$work/$distro-r-down.log"
	if install "$out" "${envs[@]}"; then
		check "(r) version installee superieure : refus" false
	else
		check "(r) version installee superieure : refus" grep -q 'retour a .* refuse' "$out"
	fi
	in_ctr 'echo v0.0.1 >/usr/local/share/ciphera/VERSION'
	out="$work/$distro-r-up.log"
	if install "$out" "${envs[@]}"; then
		check "(r) version installee inferieure : renvoi vers l'updater" false
	else
		check "(r) version installee inferieure : renvoi vers l'updater" \
			grep -q 'ciphera-cli updater --version' "$out"
	fi
	check "(r) refus : version installee intacte" \
		in_ctr 'test "$(cat /usr/local/share/ciphera/VERSION)" = v0.0.1'
	in_ctr 'mv /root/VERSION.saved /usr/local/share/ciphera/VERSION'

	# (d) --purge retire aussi l'identite de l'agent ; une reinstallation repart de zero.
	out="$work/$distro-d.log"
	check "(d) desinstallation --purge" in_ctr 'sh /test/uninstall.sh --purge >/dev/null 2>&1'
	check "(d) rien ne subsiste (dont /etc/ciphera-agent)" nothing_installed
	check "(d) compte et etat de ciphera-build retires" \
		in_ctr '! id -u ciphera-build >/dev/null 2>&1 && test ! -e /var/lib/ciphera-buildkit'
	check "(d) aucune plage de ciphera-build restante" no_build_subids
	mapfile -t envs < <(test_env good)
	if install "$out" "${envs[@]}"; then
		check "(d) reinstallation apres purge : succes" true
	else
		check "(d) reinstallation apres purge : succes" false
		cat "$out"
	fi
	check "(d) nouvelle cle de l'agent" in_ctr 'test -s /etc/ciphera-agent/agent.key'
	check "(d) une seule plage de ciphera-build apres reinstallation" build_subids_ok /etc/subuid
}

for distro in "$@"; do
	run_distro "$distro"
done

if [ "$failures" -ne 0 ]; then
	printf '%s verification(s) en echec\n' "$failures"
	exit 1
fi
printf 'tous les cas passent\n'
