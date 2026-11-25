#!/usr/bin/env bash
# Test de bout en bout de install/install.sh sur la machine elle-meme
# (runner CI Linux avec systemd et Docker) : chemin de production, ou
# l'agent lance PostgreSQL en conteneur epingle par digest, buildkitd (un par
# projet, a la demande ; ici l'exemple `buildkitd_up <projet>`, construit avec
# --example buildkitd_up, tient lieu de premier build) et le plan de controle
# joint la base par son socket Unix.
#
#   sudo CIPHERA_DESTRUCTIVE_HOST_TEST=1 install/tests/run-docker-host.sh
#
# ATTENTION : installe, desinstalle puis purge Ciphera sur la machine (comptes,
# /etc/ciphera*, volumes Docker). Reserve a une machine jetable (job CI
# install-docker) : refuse sans CIPHERA_DESTRUCTIVE_HOST_TEST=1.
#
# Exige : root, systemd, Docker, minisign, openssl, python3, curl, tar,
# sha256sum. CIPHERA_TEST_BIN_DIR : binaires musl, dont
# examples/buildkitd_up (defaut
# target/<cible musl>/release).
#
# Verifie : (a) installation, agent actif, ciphera-postgres et
# ciphera-buildkitd-<projet> (un par projet, a la demande, etat et socket
# propres) en marche avec les images de deploy/containers, reseau
# ciphera-internal interne, buildkitd rootless non privilegie seul sur
# ciphera-build (profil AppArmor la ou le noyau l'exige), sous le compte
# systeme ciphera-build (uid de chaque processus de buildkitd = id -u
# ciphera-build, ni 1000 ni root ; plages de sous-uids/sous-gids propres ;
# seules capacites SETUID et SETGID ; build par le socket), aucun port publie,
# mot de passe absent de `docker inspect`, sockets Unix crees, socket de
# buildkitd accessible au compte de l'agent, panneau joignable, agent
# connecte au plan de controle ; proxy du noeud sous son compte (port 80 :
# 301 vers le panneau pour un hote IP, 404 sinon ; 443 sans certificat ;
# socket de pilotage 0660 ciphera-proxy:ciphera-agent joint par l'agent ;
# 65536 descripteurs, connexion muette fermee apres 10 s) ; sortie des
# builds filtree (table nftables inet ciphera_egress) : une etape RUN de
# BuildKit et un conteneur sur ciphera-prepare n'atteignent ni l'hote
# (proxy, panneau) ni 169.254.169.254, mais Internet public (temoin : le
# pont par defaut atteint l'hote) ;
# sortie des applications filtree (pont de projet cphp<10 hex>) : un
# conteneur n'atteint ni un port de test de l'hote, ni 169.254.169.254:80
# (adresse liee a lo, servie par le proxy du noeud), ni une adresse de lien
# local routee (169.254.170.2), mais le proxy (80), le reseau prive, le DNS
# et Internet ; CIPHERA_APP_EGRESS_DENY_PRIVATE=1 ferme le reseau prive ;
# docker et docker compose fonctionnent sous le durcissement de l'unite de
# l'agent (SystemCallFilter, RestrictAddressFamilies, PrivateDevices...) ;
# table retablie par ciphera-egress-check apres `nft flush ruleset` ;
# (b) desinstallation sans --purge : conteneurs retires, volume de la base
# conserve, proxy arrete, filtrage et sa minuterie retires ; (c) reinstallation : meme base,
# panneau et proxy joignables, filtrage recharge, plage de sous-uids de ciphera-build conservee sans doublon ;
# (d) --purge : volume, reseaux, profil AppArmor, secrets, comptes (plages
# de ciphera-build comprises) et etat de BuildKit supprimes.

# Les commandes entre apostrophes sont evaluees par sh -c.
# shellcheck disable=SC2016
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

die() {
	printf 'run-docker-host.sh : %s\n' "$*" >&2
	exit 1
}

[ "${CIPHERA_DESTRUCTIVE_HOST_TEST:-}" = 1 ] ||
	die "modifie la machine : relancer avec CIPHERA_DESTRUCTIVE_HOST_TEST=1 (machine jetable seulement)"
[ "$(id -u)" -eq 0 ] || die "droits root requis"
for t in docker systemctl minisign openssl python3 curl tar sha256sum ip; do
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

work=$(mktemp -d)
server_pid=
witness_pid=
witness_ll_pid=
host_port_pid=
app_net=
cleanup() {
	local pid
	for pid in "$server_pid" "$witness_pid" "$witness_ll_pid" "$host_port_pid"; do
		if [ -n "$pid" ]; then
			kill "$pid" 2>/dev/null || true
		fi
	done
	if [ -n "$app_net" ]; then
		docker network rm "$app_net" >/dev/null 2>&1 || true
	fi
	ip addr del 169.254.169.254/32 dev lo 2>/dev/null || true
	ip route del 169.254.170.2/32 2>/dev/null || true
	ip netns del ciphera-witness 2>/dev/null || true
	ip link del cw-host 2>/dev/null || true
	rm -rf "$work" /run/ciphera-compose-test
}
trap cleanup EXIT

# --- release de test signee par une cle jetable --------------------------------
port=9443
archive="ciphera-$version-$target.tar.gz"
release="$work/srv/good/releases/$version"
mkdir -p "$work/keys" "$release"
minisign -G -W -p "$work/keys/test.pub" -s "$work/keys/test.key" >/dev/null
pubkey=$(sed -n 2p "$work/keys/test.pub")
sh "$root/install/package.sh" "$version" "$target" "$bin_dir" "$release"
(cd "$release" && sha256sum "$archive" >SHA256SUMS)
minisign -S -s "$work/keys/test.key" -m "$release/SHA256SUMS" >/dev/null
rm -f "$work/keys/test.key"

sh "$here/serve-release.sh" "$work/srv" "$port" "$work/tls" &
server_pid=$!
for _ in $(seq 1 30); do
	[ -e "$work/tls/ready" ] && break
	sleep 1
done
[ -e "$work/tls/ready" ] || die "serveur de release non demarre"

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

run_install() { # <journal>
	env CIPHERA_INSTALL_TEST=1 \
		"CIPHERA_RELEASE_BASE_URL=https://localhost:$port/good" \
		"CIPHERA_TEST_MINISIGN_PUBKEY=$pubkey" \
		"CURL_CA_BUNDLE=$work/tls/server.crt" \
		sh "$installer" --non-interactive --public-ip 127.0.0.1 >"$1" 2>&1
}

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

no_published_ports() { # <conteneur>
	local ports
	ports=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$1")
	[ "$ports" = '{}' ] || [ "$ports" = null ]
}

pinned_image() { # <conteneur> <fichier de digest>
	[ "$(docker inspect -f '{{.Config.Image}}' "$1")" = "$(tr -d '[:space:]' <"$root/deploy/containers/$2")" ]
}

panel_up() {
	wait_for 300 'curl -fsk https://127.0.0.1:8443/api/health'
}

diagnose() {
	journalctl -u ciphera-agent -u ciphera -u ciphera-proxy -u ciphera-proxy.socket --no-pager -n 80 || true
	docker ps -a || true
	docker logs --tail 40 ciphera-postgres 2>&1 || true
	docker logs --tail 40 "$bk" 2>&1 || true
}

# Avant l'installation : un compte (et son groupe) dont l'id tombe dans la
# premiere plage de sous-identifiants libre (apres les plages existantes),
# qu'install.sh doit eviter ; l'ancien volume du cache de BuildKit (uid
# 1000), qu'install.sh doit supprimer.
id_candidate=$({ cat /etc/subuid /etc/subgid 2>/dev/null || true; } | awk -F: 'NF >= 3 && $2 ~ /^[0-9]+$/ && $3 ~ /^[0-9]+$/ { e = $2 + $3; if (e > m) m = e }
	END { if (m < 100000) m = 100000; printf "%.0f\n", m + 100 }')
groupadd -g "$id_candidate" ciphera-idtest
useradd -M -u "$id_candidate" -g ciphera-idtest -s /usr/sbin/nologin ciphera-idtest 2>/dev/null
docker volume create ciphera-buildkit-cache >/dev/null

# buildkitd d'un projet (demarre a la demande, un conteneur par projet) :
# nom `ciphera-buildkitd-<12 hex de l'UUID>`, socket et etat propres.
bk_project=$(python3 -c 'import uuid; print(uuid.uuid4())')
bk=ciphera-buildkitd-$(printf '%s' "$bk_project" | tr -d - | cut -c1-12)
bk_dir=/run/ciphera-agent/buildkit/$bk_project
bk_state=/var/lib/ciphera-buildkit/p/$bk_project
export bk bk_project bk_dir bk_state

# --- (a) installation -----------------------------------------------------------
printf '== (a) installation\n'
log="$work/a.log"
if run_install "$log"; then
	check "(a) install.sh : succes" true
else
	check "(a) install.sh : succes" false
	printf -- '--- sortie de install.sh ---\n'
	cat "$log"
	diagnose
fi
check "(a) agent actif" sh -c 'test "$(systemctl is-active ciphera-agent)" = active'
check "(a) ciphera-postgres en marche" wait_for 300 "test \"\$(docker inspect -f '{{.State.Running}}' ciphera-postgres 2>/dev/null)\" = true"
check "(a) aucun buildkitd partage (ciphera-buildkitd)" sh -c '! docker inspect ciphera-buildkitd >/dev/null 2>&1'
# Premier usage du projet : l'agent (compte ciphera-agent) cree l'etat et le
# socket sous le compte ciphera-build, puis demarre le conteneur.
# Outil du test : exemple `buildkitd_up` construit par le job (jamais dans le
# binaire de production), copie lisible par ciphera-agent.
bk_up_src=$bin_dir/examples/buildkitd_up
[ -x "$bk_up_src" ] || die "exemple buildkitd_up absent ($bk_up_src) : cargo build --release --target $target -p ciphera-agent --example buildkitd_up"
bk_up=$(mktemp /tmp/ciphera-buildkitd-up.XXXXXX)
install -m 0755 "$bk_up_src" "$bk_up"
check "(a) buildkitd du projet demarre a la demande" \
	wait_for 300 "runuser -u ciphera-agent -- '$bk_up' '$bk_project' >/dev/null"
rm -f "$bk_up"
check "(a) $bk en marche" wait_for 300 "test \"\$(docker inspect -f '{{.State.Running}}' $bk 2>/dev/null)\" = true"
check "(a) image epinglee de PostgreSQL" pinned_image ciphera-postgres postgres.digest
check "(a) image epinglee de BuildKit" pinned_image "$bk" buildkit.digest
check "(a) outils de build installes (railpack, buildctl, mise)" \
	sh -c '/usr/local/lib/ciphera/bin/railpack --version >/dev/null && /usr/local/lib/ciphera/bin/buildctl --version >/dev/null && /usr/local/lib/ciphera/bin/mise-2026.10.14 --version | grep -q 2026.10.14'
check "(a) ciphera-internal est interne" \
	sh -c 'test "$(docker network inspect -f "{{.Internal}}" ciphera-internal)" = true'
check "(a) ciphera-build n'est pas interne" \
	sh -c 'test "$(docker network inspect -f "{{.Internal}}" ciphera-build)" = false'
check "(a) buildkitd seul sur ciphera-build" \
	sh -c 'test "$(docker inspect -f "{{range \$k, \$v := .NetworkSettings.Networks}}{{\$k}} {{end}}" $bk)" = "ciphera-build "'
check "(a) buildkitd non privilegie" \
	sh -c 'test "$(docker inspect -f "{{.HostConfig.Privileged}}" $bk)" = false'
if [ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ] && command -v apparmor_parser >/dev/null; then
	check "(a) profil AppArmor de buildkitd" \
		sh -c 'docker inspect -f "{{json .HostConfig.SecurityOpt}}" "$bk" | grep -q "apparmor=ciphera-buildkitd"'
	check "(a) profil AppArmor de buildkitd charge en mode applique" \
		grep -qxF 'ciphera-buildkitd (enforce)' /sys/kernel/security/apparmor/profiles
	check "(a) buildkitd tourne sous le profil applique" \
		sh -c 'pid=$(docker inspect -f "{{.State.Pid}}" "$bk") && test "$(cat "/proc/$pid/attr/current")" = "ciphera-buildkitd (enforce)"'
fi
check "(a) PostgreSQL sans port publie" no_published_ports ciphera-postgres
check "(a) buildkitd sans port publie" no_published_ports "$bk"
check "(a) mot de passe absent de docker inspect" \
	sh -c '! docker inspect ciphera-postgres | grep -qF "$(cat /etc/ciphera/db_password)"'
check "(a) socket Unix de PostgreSQL" wait_for 120 'test -S /run/ciphera/postgres/.s.PGSQL.5432'
check "(a) socket Unix de buildkitd" wait_for 120 'test -S "$bk_dir/buildkitd.sock"'
bk_sock=$bk_dir/buildkitd.sock
# Connexion reelle au socket : curl sort en 7 seulement si connect() echoue
# (droits refuses) ; toute autre sortie prouve que la connexion a ete etablie.
bk_connect() { # <compte>
	rc=0
	runuser -u "$1" -- curl -s -o /dev/null --max-time 5 --http2-prior-knowledge \
		--unix-socket "$bk_sock" http://buildkitd/ || rc=$?
	[ "$rc" -ne 7 ]
}
bk_access() {
	runuser -u ciphera-agent -- test -r "$bk_sock" -a -w "$bk_sock" && bk_connect ciphera-agent
}
if bk_access; then
	check "(a) socket de buildkitd accessible a l'agent (groupe setgid)" true
else
	check "(a) socket de buildkitd accessible a l'agent (groupe setgid)" false
	printf -- '--- diagnostic du socket de buildkitd ---\n'
	id ciphera-agent || true
	stat /run/ciphera-agent /run/ciphera-agent/buildkit "$bk_sock" || true
	getfacl -p /run/ciphera-agent/buildkit "$bk_sock" 2>&1 || true
	ls -lan /run/ciphera-agent/buildkit || true
	docker inspect -f '{{json .Config.Cmd}} {{json .HostConfig.Binds}}' "$bk" || true
	diagnose
fi
check "(a) socket de buildkitd ferme aux autres comptes" sh -c "! runuser -u nobody -- test -w $bk_sock"

# buildkitd sous le compte systeme dedie ciphera-build : jamais l'uid 1000
# ni root, plage de sous-uids/sous-gids propre, seules capacites SETUID et
# SETGID, etat et socket dans des dossiers a ce compte.
# Une seule plage de ciphera-build, de 65536 ids, sans chevauchement.
subids_ok() { # <fichier>
	awk -F: -v me=ciphera-build '
		NF >= 3 { s[NR] = $2; e[NR] = $2 + $3; if ($1 == me) { n++; mine = NR } }
		END {
			if (n != 1 || e[mine] - s[mine] != 65536) exit 1
			for (i = 1; i <= NR; i++)
				if ((i in s) && i != mine && s[i] < e[mine] && s[mine] < e[i]) exit 1
		}' "$1"
}
# « uid nom » (uid reel de l'hote) de chaque processus du conteneur buildkitd.
bk_process_uids() {
	local pid
	for pid in $(docker top "$bk" -eo pid | tail -n +2); do
		awk '/^Name:/ { n = $2 } /^Uid:/ { u = $2 } END { if (u != "") print u, n }' "/proc/$pid/status" 2>/dev/null || true
	done
}
bk_runs_as_build_account() {
	local build_uid uids
	build_uid=$(id -u ciphera-build) || return 1
	uids=$(bk_process_uids)
	printf '%s\n' "$uids" | grep -q ' buildkitd$' &&
		[ "$(printf '%s\n' "$uids" | cut -d' ' -f1 | sort -u)" = "$build_uid" ] &&
		[ "$build_uid" -ne 1000 ] && [ "$build_uid" -ne 0 ]
}
check "(a) compte systeme ciphera-build (uid 1-999, ni 1000, sans shell)" \
	sh -c 'u=$(id -u ciphera-build) && test "$u" -gt 0 && test "$u" -lt 1000 && getent passwd ciphera-build | cut -d: -f7 | grep -q nologin'
check "(a) plage de sous-uids de ciphera-build (65536, sans chevauchement)" subids_ok /etc/subuid
check "(a) plage de sous-gids de ciphera-build (65536, sans chevauchement)" subids_ok /etc/subgid
# La plage ne contient aucun uid (sous-uids) ni gid (sous-gids) existant,
# dont celui de ciphera-idtest place dans la premiere plage candidate.
range_avoids_ids() { # <fichier> <passwd|group>
	local start count
	IFS=: read -r _ start count < <(grep '^ciphera-build:' "$1" | head -n 1)
	[ -n "$start" ] && [ "$start" -gt "$id_candidate" ] &&
		getent "$2" | awk -F: -v s="$start" -v e="$((start + count))" '$3 >= s && $3 < e { bad = 1 } END { exit bad }'
}
check "(a) sous-uids hors des uids existants (ciphera-idtest evite)" range_avoids_ids /etc/subuid passwd
check "(a) sous-gids hors des gids existants (ciphera-idtest evite)" range_avoids_ids /etc/subgid group
check "(a) ancien volume ciphera-buildkit-cache supprime" \
	sh -c '! docker volume inspect ciphera-buildkit-cache >/dev/null 2>&1'
check "(a) dossier du socket de buildkitd 2770 ciphera-build:ciphera-agent" \
	sh -c 'test "$(stat -c "%a %U %G" /run/ciphera-agent/buildkit)" = "2770 ciphera-build ciphera-agent"'
check "(a) etat de BuildKit 0700 ciphera-build" \
	sh -c 'test "$(stat -c "%a %U %G" /var/lib/ciphera-buildkit)" = "700 ciphera-build ciphera-build"'
check "(a) dossier du socket du projet 2770 ciphera-build:ciphera-agent (setgid herite)" \
	sh -c 'test "$(stat -c "%a %U %G" "$bk_dir")" = "2770 ciphera-build ciphera-agent"'
check "(a) etat du projet 0700 ciphera-build" \
	sh -c 'test "$(stat -c "%a %U %G" "$bk_state")" = "700 ciphera-build ciphera-build"'
check "(a) buildkitd monte l'etat et le socket de son projet seulement" \
	sh -c 'b=$(docker inspect -f "{{json .HostConfig.Binds}}" "$bk"); echo "$b" | grep -q "$bk_state:/home/ciphera-build" && echo "$b" | grep -q "$bk_dir:/run/buildkit" && ! echo "$b" | grep -q "ciphera-buildkit:/"'
check "(a) conteneur buildkitd lance en uid:gid de ciphera-build" \
	sh -c 'test "$(docker inspect -f "{{.Config.User}}" $bk)" = "$(id -u ciphera-build):$(id -g ciphera-build)"'
if bk_runs_as_build_account; then
	check "(a) processus de buildkitd sous l'uid de ciphera-build (ni 1000 ni root)" true
else
	check "(a) processus de buildkitd sous l'uid de ciphera-build (ni 1000 ni root)" false
	printf -- '--- uid des processus de buildkitd (attendu : %s) ---\n' "$(id -u ciphera-build 2>/dev/null)"
	bk_process_uids || true
	docker top "$bk" || true
fi
check "(a) buildkitd : seules capacites SETUID et SETGID" \
	sh -c 'test "$(awk "/^CapBnd:/ { print \$2 }" /proc/"$(docker inspect -f "{{.State.Pid}}" $bk)"/status)" = 00000000000000c0'
# Build reel par le socket (buildctl de l'image, sous le compte du conteneur).
bk_build() {
	docker exec "$bk" sh -c 'set -e; d=$(mktemp -d)
		printf "FROM docker.io/library/busybox:1.37\nRUN echo ok > /ok\n" >"$d/Dockerfile"
		buildctl --addr unix:///run/buildkit/buildkitd.sock build --frontend dockerfile.v0 \
			--local context="$d" --local dockerfile="$d"' >"$work/build.log" 2>&1
}
if bk_build; then
	check "(a) build d'un Dockerfile par le socket de buildkitd" true
else
	check "(a) build d'un Dockerfile par le socket de buildkitd" false
	cat "$work/build.log"
	diagnose
fi
# Filtrage de la sortie des builds (table nftables inet ciphera_egress,
# chargee par ciphera-agent.service) : une etape RUN de BuildKit et un
# conteneur sur ciphera-prepare n'atteignent ni l'hote (proxy :80, panneau
# :8443, chaine input), ni un serveur d'une plage privee hors de l'hote
# (10.250.0.2:8080, chaine forward), ni l'adresse de metadonnees ; Internet
# public (et le DNS) restent ouverts. Temoin : les memes cibles depuis le
# pont par defaut de Docker, qui doit atteindre l'hote et le serveur prive
# (la regle compte, pas seulement l'absence de route).
#
# Serveur prive : espace de noms reseau relie a l'hote par une paire veth
# (10.250.0.1 cote hote, 10.250.0.2 cote serveur), joint par routage, donc
# par la chaine forward, comme un service d'un reseau prive du cloud.
ip netns add ciphera-witness
ip link add cw-host type veth peer name cw-ns
ip link set cw-ns netns ciphera-witness
ip addr add 10.250.0.1/30 dev cw-host
ip link set cw-host up
ip netns exec ciphera-witness ip addr add 10.250.0.2/30 dev cw-ns
ip netns exec ciphera-witness ip link set cw-ns up
ip netns exec ciphera-witness ip link set lo up
ip netns exec ciphera-witness ip route add default via 10.250.0.1
ip netns exec ciphera-witness python3 -m http.server 8080 --bind 10.250.0.2 >/dev/null 2>&1 &
witness_pid=$!
private_url=http://10.250.0.2:8080/
wait_for 30 "curl -fs -o /dev/null $private_url" || printf 'serveur prive temoin injoignable depuis l hote\n'
host_ip=$(ip -4 route get 1.1.1.1 | sed -n 's/.* src \([0-9.]*\).*/\1/p')
public_url=http://deb.debian.org/debian/
# Sonde busybox : « PROBE <cible> reached|blocked » par cible ; bloquee si la
# connexion echoue (rejet ou delai), atteinte sinon (meme sur une erreur HTTP
# ou TLS : le serveur a repondu).
probe_script='for t in "$@"; do
	case $t in
	http://deb.*) wget -T 5 -O /dev/null "$t" 2>/tmp/e ;;
	*) wget -T 5 -O /dev/null --header "Host: sonde.invalid" "$t" 2>/tmp/e ;;
	esac
	if [ $? -eq 0 ] || ! grep -qE "can.t connect|timed out|unreachable|bad address" /tmp/e; then
		echo "PROBE $t reached"
	else
		echo "PROBE $t blocked: $(tr "\n" " " </tmp/e)"
	fi
done'
probe_targets="http://169.254.169.254/ http://$host_ip/ http://$host_ip:8443/ $private_url $public_url"
probe_ok() { # <journal> <cible> <reached|blocked>
	grep -q "PROBE $2 $3" "$1"
}
egress_build() {
	docker exec -i "$bk" sh -c 'set -e; d=$(mktemp -d); cat >"$d/probe.sh"
		printf "FROM docker.io/library/busybox:1.37\nCOPY probe.sh /probe.sh\nRUN echo %s && sh /probe.sh %s\n" \
			"$(date +%s%N)" "$1" >"$d/Dockerfile"
		buildctl --addr unix:///run/buildkit/buildkitd.sock build --progress plain \
			--frontend dockerfile.v0 --local context="$d" --local dockerfile="$d"' \
		sh "$probe_targets" <<<"$probe_script" >"$work/egress-build.log" 2>&1
}
# shellcheck disable=SC2086
egress_container() { # <reseau> <journal>
	docker run --rm --network "$1" --entrypoint sh docker.io/library/busybox:1.37 \
		-c "$probe_script" sh $probe_targets >"$2" 2>&1 # une cible par argument
}
check "(a) ciphera-build : interface nommee ciphera-build" test -d /sys/class/net/ciphera-build
check "(a) filtrage de sortie charge (table inet ciphera_egress)" nft list table inet ciphera_egress
egress_build || true
egress_container bridge "$work/egress-witness.log" || true
# Pont ciphera-prepare : cree comme l'agent le fait (gere par Ciphera, sans
# communication entre conteneurs) s'il n'existe pas encore, puis retire.
prepare_created=
if ! docker network inspect ciphera-prepare >/dev/null 2>&1; then
	docker network create --driver bridge --label dev.ciphera.managed=true \
		-o com.docker.network.bridge.enable_icc=false \
		-o com.docker.network.bridge.name=ciphera-prepare ciphera-prepare >/dev/null
	prepare_created=1
fi
egress_container ciphera-prepare "$work/egress-prepare.log" || true
[ -z "$prepare_created" ] || docker network rm ciphera-prepare >/dev/null 2>&1 || true
check "(a) temoin (pont par defaut) : proxy de l'hote joignable" probe_ok "$work/egress-witness.log" "http://$host_ip/" reached
check "(a) temoin (pont par defaut) : panneau de l'hote joignable" probe_ok "$work/egress-witness.log" "http://$host_ip:8443/" reached
check "(a) temoin (pont par defaut) : serveur prive (10.250.0.2) joignable" probe_ok "$work/egress-witness.log" "$private_url" reached
# Metadonnees du cloud : temoin indicatif (le runner peut ne pas les servir).
grep 'PROBE http://169.254.169.254/' "$work/egress-witness.log" | sed 's/^/  info temoin : /' || true
for log in "$work/egress-build.log" "$work/egress-prepare.log"; do
	case $log in
	*build*) where="etape RUN de BuildKit" ;;
	*) where="conteneur sur ciphera-prepare" ;;
	esac
	check "(a) $where : metadonnees (169.254.169.254) bloquees" probe_ok "$log" http://169.254.169.254/ blocked
	check "(a) $where : proxy de l'hote bloque" probe_ok "$log" "http://$host_ip/" blocked
	check "(a) $where : panneau de l'hote bloque" probe_ok "$log" "http://$host_ip:8443/" blocked
	check "(a) $where : serveur prive (10.250.0.2) bloque" probe_ok "$log" "$private_url" blocked
	check "(a) $where : Internet public (DNS compris) joignable" probe_ok "$log" "$public_url" reached
done
if [ "$failures" -ne 0 ]; then
	printf -- '--- sondes de sortie (hote %s) ---\n' "$host_ip"
	cat "$work/egress-build.log" "$work/egress-witness.log" "$work/egress-prepare.log" 2>/dev/null | grep -E 'PROBE|ERROR|error' || true
	nft list table inet ciphera_egress || true
fi
# Filtrage de la sortie des applications (table inet ciphera_egress,
# chaines app_input et app_forward, pont de projet cphp<10 hex>) :
# - bloques : un port de test de l'hote (chaine input), 169.254.169.254:80
#   (adresse liee a lo, servie par le proxy du noeud qui ecoute toutes les
#   adresses : faux service de metadonnees local, chaine input) et
#   169.254.170.2:8080 (lien local route vers le serveur prive, chaine
#   forward, comme les metadonnees d'un cloud) ;
# - joignables : le proxy de l'hote (port 80, domaines servis par ce noeud),
#   le serveur prive 10.250.0.2 (plages privees permises par defaut), le DNS
#   et Internet public.
# Temoin : les memes cibles depuis le pont par defaut de Docker.
app_project=$(python3 -c 'import uuid; print(uuid.uuid4())')
app_net=ciphera-p-$app_project
app_bridge=cphp$(printf '%s' "$app_project" | tr -d - | cut -c1-10)
# Reseau de projet cree comme l'agent le fait (docker::network_create_request).
docker network create --driver bridge --label dev.ciphera.managed=true \
	-o "com.docker.network.bridge.name=$app_bridge" "$app_net" >/dev/null
host_port=18081
python3 -m http.server "$host_port" --bind 0.0.0.0 >/dev/null 2>&1 &
host_port_pid=$!
ip addr add 169.254.169.254/32 dev lo
ip netns exec ciphera-witness ip addr add 169.254.170.2/32 dev cw-ns
ip route add 169.254.170.2/32 via 10.250.0.2 dev cw-host
ip netns exec ciphera-witness python3 -m http.server 8080 --bind 169.254.170.2 >/dev/null 2>&1 &
witness_ll_pid=$!
wait_for 30 "curl -fs -o /dev/null http://$host_ip:$host_port/" || printf 'port de test de l hote injoignable\n'
wait_for 30 "curl -fs -o /dev/null http://169.254.170.2:8080/" || printf 'serveur de lien local temoin injoignable\n'
app_targets="http://$host_ip:$host_port/ http://169.254.169.254/ http://169.254.170.2:8080/ http://$host_ip/ $private_url $public_url"
# shellcheck disable=SC2086
egress_app() { # <reseau> <journal>
	docker run --rm --network "$1" --entrypoint sh docker.io/library/busybox:1.37 \
		-c "$probe_script; if nslookup deb.debian.org >/tmp/d 2>&1; then echo 'PROBE dns reached'; else echo \"PROBE dns blocked: \$(tr '\n' ' ' </tmp/d)\"; fi" \
		sh $app_targets >"$2" 2>&1 # une cible par argument
}
check "(a) pont du projet nomme $app_bridge" test -d "/sys/class/net/$app_bridge"
egress_app bridge "$work/egress-app-witness.log" || true
egress_app "$app_net" "$work/egress-app.log" || true
for t in "http://$host_ip:$host_port/" http://169.254.169.254/ http://169.254.170.2:8080/ "http://$host_ip/" "$private_url"; do
	check "(a) temoin (pont par defaut) : $t joignable" probe_ok "$work/egress-app-witness.log" "$t" reached
done
check "(a) application : port de test de l'hote bloque" probe_ok "$work/egress-app.log" "http://$host_ip:$host_port/" blocked
check "(a) application : 169.254.169.254:80 (lie a lo) bloque" probe_ok "$work/egress-app.log" http://169.254.169.254/ blocked
check "(a) application : lien local route (169.254.170.2) bloque" probe_ok "$work/egress-app.log" http://169.254.170.2:8080/ blocked
check "(a) application : proxy de l'hote (port 80) joignable" probe_ok "$work/egress-app.log" "http://$host_ip/" reached
check "(a) application : serveur prive (10.250.0.2) joignable par defaut" probe_ok "$work/egress-app.log" "$private_url" reached
check "(a) application : DNS joignable" probe_ok "$work/egress-app.log" dns reached
check "(a) application : Internet public joignable" probe_ok "$work/egress-app.log" "$public_url" reached
# CIPHERA_APP_EGRESS_DENY_PRIVATE=1 : le reseau prive se ferme aussi, le DNS
# et Internet restent ouverts ; applique par ciphera-egress-check.
egress_conf=/etc/ciphera-egress/egress.env
check "(a) reglage du filtrage : root 0644, plages privees permises" \
	sh -c "test \"\$(stat -c %a:%U:%G $egress_conf)\" = 644:root:root && grep -qx CIPHERA_APP_EGRESS_DENY_PRIVATE=0 $egress_conf"
cp "$egress_conf" "$work/egress.env.saved"
printf 'CIPHERA_APP_EGRESS_DENY_PRIVATE=1\n' >"$egress_conf"
check "(a) ciphera-egress-check applique le reglage" systemctl start ciphera-egress-check.service
egress_app "$app_net" "$work/egress-app-deny.log" || true
check "(a) plages privees interdites : serveur prive bloque" probe_ok "$work/egress-app-deny.log" "$private_url" blocked
check "(a) plages privees interdites : DNS joignable" probe_ok "$work/egress-app-deny.log" dns reached
check "(a) plages privees interdites : Internet public joignable" probe_ok "$work/egress-app-deny.log" "$public_url" reached
cp "$work/egress.env.saved" "$egress_conf"
systemctl start ciphera-egress-check.service || true
egress_app "$app_net" "$work/egress-app-back.log" || true
check "(a) reglage remis a 0 : serveur prive joignable" probe_ok "$work/egress-app-back.log" "$private_url" reached
if [ "$failures" -ne 0 ]; then
	printf -- '--- sondes de sortie des applications (hote %s, pont %s) ---\n' "$host_ip" "$app_bridge"
	cat "$work"/egress-app*.log 2>/dev/null | grep -E 'PROBE|ERROR|error' || true
	nft list table inet ciphera_egress || true
	journalctl -u ciphera-egress-check --no-pager -n 20 || true
fi
docker network rm "$app_net" >/dev/null 2>&1 || true
app_net=
kill "$host_port_pid" "$witness_ll_pid" 2>/dev/null || true
host_port_pid=
witness_ll_pid=
ip addr del 169.254.169.254/32 dev lo 2>/dev/null || true
ip route del 169.254.170.2/32 2>/dev/null || true

# Durcissement de ciphera-agent.service (SystemCallFilter=@system-service,
# SystemCallErrorNumber, RestrictAddressFamilies avec AF_NETLINK,
# PrivateDevices, RestrictNamespaces...) : l'agent tourne sous ce filtre, et
# les outils qu'il lance (docker, docker compose, buildctl, git) aussi. Memes
# directives reprises de l'unite installee et appliquees par systemd-run au
# compte ciphera-agent (groupe docker) : docker version, docker run, et un
# projet Compose complet (config, up, ps, down).
agent_pid=$(systemctl show -p MainPID --value ciphera-agent)
check "(a) agent sous filtre seccomp, sans nouveaux privileges" \
	sh -c "grep -q '^Seccomp:[[:space:]]*2' /proc/$agent_pid/status && grep -q '^NoNewPrivs:[[:space:]]*1' /proc/$agent_pid/status"
mapfile -t agent_hardening < <(sed -n '/^\[Service\]/,/^\[Install\]/p' /etc/systemd/system/ciphera-agent.service |
	grep -E '^(NoNewPrivileges|Protect[A-Za-z]+|Private[A-Za-z]+|Restrict[A-Za-z]+|LockPersonality|SystemCall[A-Za-z]+|UMask)=')
hardening_props=()
for line in "${agent_hardening[@]}"; do
	hardening_props+=(-p "$line")
done
mkdir -p /run/ciphera-compose-test
printf 'services:\n  t:\n    image: docker.io/library/busybox:1.37\n    command: ["sleep", "60"]\n' >/run/ciphera-compose-test/compose.yaml
chmod -R a+rX /run/ciphera-compose-test
hardened_run() { # <journal> <script sh>
	systemd-run --wait --pipe --quiet --collect -p User=ciphera-agent -p Group=ciphera-agent \
		-p Environment=HOME=/tmp "${hardening_props[@]}" /bin/sh -c "$2" >"$1" 2>&1
}
hardened_tools='set -e
docker version --format "{{.Server.Version}}"
docker run --rm docker.io/library/busybox:1.37 true
docker compose version
cd /run/ciphera-compose-test
docker compose -p ciphera-hardened config --format json >/dev/null
docker compose -p ciphera-hardened up -d
docker compose -p ciphera-hardened ps --format json | grep -q running
docker compose -p ciphera-hardened down
/usr/local/lib/ciphera/bin/buildctl --version
git --version
echo HARDENED_OK'
check "(a) ${#agent_hardening[@]} directives de durcissement reprises de l'unite" test "${#agent_hardening[@]}" -ge 15
if hardened_run "$work/hardened.log" "$hardened_tools" && grep -q HARDENED_OK "$work/hardened.log"; then
	check "(a) docker et docker compose sous le durcissement de l'agent" true
else
	check "(a) docker et docker compose sous le durcissement de l'agent" false
	printf -- '--- durcissement : %s ---\n' "${agent_hardening[*]}"
	cat "$work/hardened.log"
fi
docker compose -p ciphera-hardened -f /run/ciphera-compose-test/compose.yaml down >/dev/null 2>&1 || true
# Indicatif : appels systeme hors de @system-service tentes par ces outils
# (journalises sans filtre, SystemCallLog=), pour le suivi du durcissement.
if systemd-run --version 2>/dev/null | awk 'NR == 1 { exit !($2 >= 247) }'; then
	t0=$(date +%s)
	systemd-run --wait --pipe --quiet --collect -p User=ciphera-agent -p Group=ciphera-agent \
		-p Environment=HOME=/tmp -p 'SystemCallLog=~@system-service' /bin/sh -c "$hardened_tools" >/dev/null 2>&1 || true
	journalctl -k --since "@$t0" --no-pager 2>/dev/null | grep 'type=1326' | sed 's/.*comm=\("[^"]*"\).*syscall=\([0-9]*\).*/\1 \2/' |
		sort -u | sed 's/^/  info appel hors @system-service (comm, numero) : /' || true
fi
rm -rf /run/ciphera-compose-test

subuid_line=$(grep '^ciphera-build:' /etc/subuid || true)
subgid_line=$(grep '^ciphera-build:' /etc/subgid || true)
if panel_up; then
	check "(a) panneau joignable (base par socket)" true
else
	check "(a) panneau joignable (base par socket)" false
	diagnose
fi
check "(a) agent connecte au plan de controle" \
	wait_for 60 'journalctl -u ciphera --no-pager | grep -q "lien agents : agent .* connecte ("'
# Commandes de secours telles qu'affichees par le panneau (sudo, sans
# l'environnement du service) : base jointe par le socket de l'unite.
check "(a) sudo ciphera admin clear-ip-allowlist (environnement vide)" \
	sh -c 'out=$(env -i /usr/local/bin/ciphera admin clear-ip-allowlist 2>&1); rc=$?; printf "%s\n" "$out" | grep -q "liste d.IP autorisees" || { echo "    code $rc : $out" >&2; false; }'
# Ecouteurs du panneau bornes : descripteurs releves, aucune ecoute HTTP 8080
# (le port 80 appartient au proxy), connexion muette fermee sur 8443.
check "(a) panneau : 65536 descripteurs (LimitNOFILE)" \
	sh -c 'grep "^Max open files" /proc/"$(systemctl show -p MainPID --value ciphera)"/limits | grep -q " 65536 *65536 "'
check "(a) panneau : aucune ecoute HTTP 8080" sh -c '! ss -Hltn "sport = :8080" | grep -q .'
check "(a) panneau : connexion muette fermee sur 8443" \
	bash -c 's=$(date +%s); exec 3<>/dev/tcp/127.0.0.1/8443; timeout 40 cat <&3 >/dev/null; e=$(( $(date +%s) - s )); test "$e" -lt 35'

# Proxy du noeud : port 80 (redirection du panneau, 404 sobre), port 443
# ouvert sans certificat, socket de pilotage reserve a l'agent.
proxy_checks() { # <etape>
	if wait_for 60 'curl -s -o /dev/null http://127.0.0.1/'; then
		check "($1) proxy : port 80 repond" true
	else
		check "($1) proxy : port 80 repond" false
		diagnose
	fi
	check "($1) proxy : hote IP -> 301 vers le panneau (:8443)" \
		sh -c 'test "$(curl -s -o /dev/null -w "%{http_code} %{redirect_url}" -H "Host: 127.0.0.1" "http://127.0.0.1/setup?step=2")" = "301 https://127.0.0.1:8443/setup?step=2"'
	check "($1) proxy : redirection sans corps" \
		sh -c 'test "$(curl -s -H "Host: 127.0.0.1" http://127.0.0.1/ | wc -c)" -eq 0'
	check "($1) proxy : hote inconnu -> 404" \
		sh -c 'test "$(curl -s -o /dev/null -w "%{http_code}" -H "Host: inconnu.example.com" http://127.0.0.1/)" = 404'
	check "($1) proxy : Host invalide -> 400" \
		sh -c 'test "$(curl -s -o /dev/null -w "%{http_code}" -H "Host: bad_host" http://127.0.0.1/)" = 400'
	check "($1) proxy : port 443 ouvert" sh -c 'ss -Hltn "sport = :443" | grep -q .'
	check "($1) proxy : TLS refuse sans certificat" sh -c '! curl -sk --max-time 5 -o /dev/null https://127.0.0.1/'
	check "($1) proxy : socket de pilotage 0660 ciphera-proxy:ciphera-agent" \
		sh -c 'test "$(stat -c "%a %U %G" /run/ciphera/proxy.sock)" = "660 ciphera-proxy ciphera-agent"'
	check "($1) proxy : socket de pilotage ferme aux autres comptes" \
		sh -c '! runuser -u nobody -- test -w /run/ciphera/proxy.sock'
	check "($1) proxy : sous le compte ciphera-proxy" \
		sh -c 'test "$(ps -o uid= -p "$(systemctl show -p MainPID --value ciphera-proxy)" | tr -d " ")" = "$(id -u ciphera-proxy)"'
	check "($1) proxy : seule capacite CAP_NET_BIND_SERVICE" \
		sh -c 'test "$(grep CapEff /proc/"$(systemctl show -p MainPID --value ciphera-proxy)"/status | cut -f2)" = 0000000000000400'
	check "($1) proxy : 65536 descripteurs (LimitNOFILE)" \
		sh -c 'grep "^Max open files" /proc/"$(systemctl show -p MainPID --value ciphera-proxy)"/limits | grep -q " 65536 *65536 "'
	check "($1) proxy : connexion muette fermee apres le delai du premier octet" \
		bash -c 's=$(date +%s); exec 3<>/dev/tcp/127.0.0.1/80; timeout 30 cat <&3 >/dev/null; e=$(( $(date +%s) - s )); test "$e" -ge 8 && test "$e" -lt 25'
	check "($1) agent connecte au socket de pilotage du proxy" \
		wait_for 60 'journalctl -u ciphera-agent --no-pager | grep -q "proxy du noeud joint (/run/ciphera/proxy.sock)"'
}
check "(a) proxy actif" sh -c 'test "$(systemctl is-active ciphera-proxy)" = active'
proxy_checks a

# Table du filtrage disparue (`nft flush ruleset`, autre outil de pare-feu) :
# retablie par ciphera-egress-check (minuterie de 5 min, forcee ici).
check "(a) minuterie ciphera-egress-check active, toutes les 5 min" \
	sh -c 'test "$(systemctl is-active ciphera-egress-check.timer)" = active && systemctl cat ciphera-egress-check.timer | grep -qx "OnCalendar=\*:0/5"'
nft flush ruleset
check "(a) nft flush ruleset : table du filtrage retiree" sh -c '! nft list table inet ciphera_egress >/dev/null 2>&1'
check "(a) ciphera-egress-check : execution forcee" systemctl start ciphera-egress-check.service
check "(a) table du filtrage retablie" nft list table inet ciphera_egress
check "(a) retablissement journalise" \
	sh -c 'journalctl -u ciphera-egress-check --no-pager | grep -q "absente, retablie"'
# Regles de Docker (NAT, isolation) videes aussi : Docker les recree a son
# redemarrage ; la base et le panneau reviennent.
systemctl restart docker
check "(a) apres redemarrage de Docker : ciphera-postgres en marche" \
	wait_for 300 "test \"\$(docker inspect -f '{{.State.Running}}' ciphera-postgres 2>/dev/null)\" = true"
if panel_up; then
	check "(a) apres redemarrage de Docker : panneau joignable" true
else
	check "(a) apres redemarrage de Docker : panneau joignable" false
	diagnose
fi

# --- (b) desinstallation sans --purge ----------------------------------------------
printf '== (b) desinstallation\n'
check "(b) uninstall.sh" sh -c "sh '$root/install/uninstall.sh' >/dev/null"
containers_removed() {
	! docker inspect ciphera-postgres >/dev/null 2>&1 && test -z "$(docker ps -aq --filter name=ciphera-buildkitd)"
}
if containers_removed; then
	check "(b) conteneurs retires" true
else
	check "(b) conteneurs retires" false
	printf -- '--- conteneurs restants ---\n'
	docker ps -a --filter label=dev.ciphera.managed=true --format '{{.ID}} {{.Names}} {{.Status}}' || true
fi
check "(b) volume de la base conserve" docker volume inspect ciphera-postgres-data
check "(b) secrets conserves" test -s /etc/ciphera/db_password
check "(b) proxy arrete, socket de pilotage retire" \
	sh -c '! systemctl is-active -q ciphera-proxy && test ! -e /run/ciphera/proxy.sock'
check "(b) filtrage de sortie retire" sh -c '! nft list table inet ciphera_egress >/dev/null 2>&1'
check "(b) minuterie ciphera-egress-check retiree" sh -c '! systemctl is-active -q ciphera-egress-check.timer && test ! -e /etc/systemd/system/ciphera-egress-check.timer'
check "(b) reglage du filtrage conserve" test -f /etc/ciphera-egress/egress.env

# --- (c) reinstallation : meme base ----------------------------------------------
printf '== (c) reinstallation\n'
log="$work/c.log"
if run_install "$log"; then
	check "(c) install.sh : succes" true
else
	check "(c) install.sh : succes" false
	printf -- '--- sortie de install.sh ---\n'
	cat "$log"
	diagnose
fi
if panel_up; then
	check "(c) panneau joignable avec la base conservee" true
else
	check "(c) panneau joignable avec la base conservee" false
	diagnose
fi
proxy_checks c
check "(c) filtrage de sortie recharge" nft list table inet ciphera_egress
check "(c) minuterie ciphera-egress-check active" sh -c 'test "$(systemctl is-active ciphera-egress-check.timer)" = active'
same_subid_lines() {
	[ -n "$subuid_line" ] && [ "$(grep '^ciphera-build:' /etc/subuid)" = "$subuid_line" ] &&
		[ -n "$subgid_line" ] && [ "$(grep '^ciphera-build:' /etc/subgid)" = "$subgid_line" ]
}
check "(c) plages de sous-uids et sous-gids de ciphera-build conservees (aucun doublon)" same_subid_lines

# --- (d) purge -------------------------------------------------------------------
printf '== (d) purge\n'
check "(d) uninstall.sh --purge" sh -c "sh '$root/install/uninstall.sh' --purge >/dev/null"
check "(d) volumes supprimes" sh -c '! docker volume inspect ciphera-postgres-data >/dev/null 2>&1 && ! docker volume inspect ciphera-buildkit-cache >/dev/null 2>&1'
check "(d) reseaux supprimes" \
	sh -c '! docker network inspect ciphera-internal >/dev/null 2>&1 && ! docker network inspect ciphera-build >/dev/null 2>&1'
check "(d) profil AppArmor retire" test ! -e /etc/apparmor.d/ciphera-buildkitd
check "(d) reglage du filtrage supprime" test ! -e /etc/ciphera-egress
check "(d) secrets et comptes supprimes" \
	sh -c 'test ! -e /etc/ciphera && test ! -e /etc/ciphera-agent && ! id -u ciphera-postgres >/dev/null 2>&1'
check "(d) compte ciphera-build, ses plages et l'etat de BuildKit supprimes" \
	sh -c '! id -u ciphera-build >/dev/null 2>&1 && ! grep -q "^ciphera-build:" /etc/subuid /etc/subgid && test ! -e /var/lib/ciphera-buildkit'

userdel ciphera-idtest 2>/dev/null || true
groupdel ciphera-idtest 2>/dev/null || true

if [ "$failures" -ne 0 ]; then
	printf '%s verification(s) en echec\n' "$failures"
	exit 1
fi
printf 'tous les cas passent\n'
