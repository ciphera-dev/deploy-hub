#!/usr/bin/env bash
# Ordre des unites systemd de deploy/systemd/ au demarrage, dans un conteneur
# Podman avec systemd (image de install/tests/Containerfile.<distro>) :
# unites installees et activees comme par install.sh, binaires remplaces par
# des bouchons, puis
#   - `systemd-analyze verify` : aucun cycle d'ordre ;
#   - vrai redemarrage du conteneur : sockets.target, paths.target et
#     ciphera-proxy.socket actifs, aucun cycle casse dans le journal.
#
#   install/tests/units-boot.sh [debian] [ubuntu] [fedora]
#
# Exige : Linux (ou machine Podman), podman.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
failures=0
cid=

die() {
	printf 'units-boot : %s\n' "$*" >&2
	exit 1
}

command -v podman >/dev/null || die "podman requis"

cleanup() {
	if [ -n "$cid" ]; then
		podman rm -f "$cid" >/dev/null 2>&1 || true
	fi
}
trap cleanup EXIT

check() { # description, commande...
	local what=$1
	shift
	if "$@"; then
		printf '  ok   %s\n' "$what"
	else
		printf '  ECHEC %s\n' "$what"
		failures=$((failures + 1))
	fi
}

in_ctr() { podman exec "$cid" sh -c "$1"; }

wait_boot() {
	local state
	for _ in $(seq 1 60); do
		state=$(in_ctr 'systemctl is-system-running 2>/dev/null' || true)
		case "$state" in running | degraded) return 0 ;; esac
		sleep 1
	done
}

# Bouchons des binaires, comptes systeme, unites et tmpfiles comme install.sh.
# shellcheck disable=SC2016
setup='set -e
for b in ciphera ciphera-agent ciphera-proxy ciphera-cli; do
	printf "#!/bin/sh\nexec sleep infinity\n" >/usr/local/bin/$b
	chmod 0755 /usr/local/bin/$b
done
for u in ciphera ciphera-agent ciphera-proxy ciphera-postgres ciphera-build; do
	getent passwd $u >/dev/null || useradd -r -M -s /usr/sbin/nologin $u
done
install -m 0644 /units/*.service /units/*.socket /units/*.path /etc/systemd/system/
install -m 0644 /units/ciphera.tmpfiles.conf /etc/tmpfiles.d/ciphera.conf
systemd-tmpfiles --create /etc/tmpfiles.d/ciphera.conf
systemctl daemon-reload
systemctl enable ciphera-proxy.socket ciphera-proxy.service ciphera.service \
	ciphera-agent.service ciphera-updater.path ciphera-updater-recover.service >/dev/null 2>&1'

# shellcheck disable=SC2016
verify='out=$(systemd-analyze verify multi-user.target /etc/systemd/system/ciphera*.service /etc/systemd/system/ciphera*.socket /etc/systemd/system/ciphera*.path 2>&1 || true)
if printf "%s\n" "$out" | grep -qi "ordering cycle"; then printf "%s\n" "$out" >&2; exit 1; fi'

# shellcheck disable=SC2016
booted='for t in sockets.target paths.target ciphera-proxy.socket; do
	test "$(systemctl is-active $t)" = active || { echo "    $t : $(systemctl is-active $t)" >&2; exit 1; }
done
if journalctl -b --no-pager | grep -qi "ordering cycle"; then journalctl -b --no-pager | grep -i cycle >&2; exit 1; fi'

run_distro() {
	local distro=$1 image="ciphera-install-test-$1"
	printf '== %s\n' "$distro"
	podman build -q -t "$image" -f "$here/Containerfile.$distro" "$here" >/dev/null
	cid=$(podman run -d --systemd=always --name "ciphera-units-boot-$distro-$$" \
		-v "$repo/deploy/systemd:/units:ro,Z" "$image")
	wait_boot
	check "unites installees et activees" in_ctr "$setup"
	check "systemd-analyze verify : aucun cycle d'ordre" in_ctr "$verify"
	podman restart "$cid" >/dev/null
	wait_boot
	check "redemarrage : cibles et socket du proxy actifs, aucun cycle" in_ctr "$booted"
	podman rm -f "$cid" >/dev/null
	cid=
}

[ "$#" -gt 0 ] || set -- debian
for distro in "$@"; do
	run_distro "$distro"
done

if [ "$failures" -ne 0 ]; then
	printf '%s verification(s) en echec\n' "$failures"
	exit 1
fi
printf 'tous les cas passent\n'
