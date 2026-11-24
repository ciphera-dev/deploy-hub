#!/bin/sh
# Desinstalle Ciphera : arrete les services, retire binaires et unites, et
# supprime les conteneurs geres par Ciphera (etiquette dev.ciphera.managed).
# Les secrets (/etc/ciphera, /etc/ciphera-agent), les donnees
# (/var/lib/ciphera*, dont l'etat de BuildKit /var/lib/ciphera-buildkit,
# volume Docker ciphera-postgres-data) et les comptes (avec la plage de
# sous-uids/sous-gids de ciphera-build) sont conserves, sauf avec --purge.
#
#   sudo sh uninstall.sh [--purge]
set -eu

BIN_DIR=/usr/local/bin
SHARE_DIR=/usr/local/share/ciphera
# Outils de build (railpack, buildctl) installes par install.sh.
TOOLS_ROOT=/usr/local/lib/ciphera
UNIT_DIR=/etc/systemd/system
BINARIES='ciphera ciphera-agent ciphera-proxy ciphera-cli'
UNITS='ciphera-updater.path ciphera-updater.service ciphera-updater-recover.service ciphera-agent.service ciphera-proxy.service ciphera-proxy.socket ciphera.service'
USERS='ciphera ciphera-proxy ciphera-agent ciphera-postgres ciphera-build'
TMPFILES_DEST=/etc/tmpfiles.d/ciphera.conf
APPARMOR_DEST=/etc/apparmor.d/ciphera-buildkitd
MANAGED=label=dev.ciphera.managed=true
# ciphera-buildkit-cache : cache de BuildKit des versions precedentes.
VOLUMES='ciphera-postgres-data ciphera-buildkit-cache'

say() {
	printf 'ciphera : %s\n' "$*"
}

die() {
	printf 'ciphera : erreur : %s\n' "$*" >&2
	exit 1
}

main() {
	purge=no
	while [ "$#" -gt 0 ]; do
		case "$1" in
		--purge) purge=yes ;;
		-h | --help)
			printf 'Usage : uninstall.sh [--purge]\n  --purge  supprime aussi secrets, donnees et comptes\n'
			exit 0
			;;
		*) die "option inconnue (voir --help)" ;;
		esac
		shift
	done
	[ "$(id -u)" -eq 0 ] || die "droits root requis : sudo sh uninstall.sh"

	# Unites du registre (install.sh et ciphera-cli updater) en plus de la
	# liste de cette version : celles ajoutees par une mise a jour.
	recorded=
	if [ -f "$SHARE_DIR/units.list" ]; then
		recorded=$(grep -E '^ciphera(-[a-z0-9]+)*[.](service|socket|path|timer)$' "$SHARE_DIR/units.list" || true)
	fi
	for u in $UNITS $recorded; do
		systemctl disable --now "$u" >/dev/null 2>&1 || true
		rm -f "$UNIT_DIR/$u"
	done
	systemctl daemon-reload
	for b in $BINARIES; do
		rm -f "$BIN_DIR/$b"
	done
	rm -rf "$SHARE_DIR" "$TOOLS_ROOT"
	rm -f "$TMPFILES_DEST"
	say "services arretes, binaires et unites retires"

	# Arguments fixes, aucune entree utilisateur.
	have_docker=no
	if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
		have_docker=yes
		docker ps -aq --filter "$MANAGED" | xargs -r docker rm -f >/dev/null || true
		say "conteneurs Ciphera supprimes"
	fi
	# Filtrage de la sortie des builds (charge par ciphera-agent.service),
	# Docker joignable ou non : l'agent, seul a lancer des builds, est arrete.
	if [ -x /usr/sbin/nft ]; then
		/usr/sbin/nft delete table inet ciphera_egress >/dev/null 2>&1 || true
	fi
	# Profil de buildkitd : retire seulement une fois son conteneur disparu
	# (sinon un conteneur restant ne pourrait plus redemarrer).
	if [ -e "$APPARMOR_DEST" ]; then
		if [ "$have_docker" = yes ] && ! docker inspect ciphera-buildkitd >/dev/null 2>&1; then
			if command -v apparmor_parser >/dev/null 2>&1; then
				apparmor_parser -R "$APPARMOR_DEST" >/dev/null 2>&1 || true
			fi
			rm -f "$APPARMOR_DEST"
		else
			say "profil AppArmor $APPARMOR_DEST conserve : Docker injoignable ou conteneur ciphera-buildkitd present (relancer uninstall.sh une fois Docker disponible)"
		fi
	fi

	if [ "$purge" = yes ]; then
		if [ "$have_docker" = yes ]; then
			for v in $VOLUMES; do
				docker volume rm -f "$v" >/dev/null 2>&1 || true
			done
			docker network ls -q --filter "$MANAGED" | xargs -r docker network rm >/dev/null 2>&1 || true
		fi
		rm -rf /etc/ciphera /etc/ciphera-agent /etc/ciphera-updater /var/lib/ciphera /var/lib/ciphera-proxy /var/lib/ciphera-agent \
			/var/lib/ciphera-buildkit /var/lib/ciphera-updater /var/backups/ciphera /run/ciphera /run/ciphera-agent
		# Plages de ciphera-build retirees par usermod (verrou de shadow) avant
		# la suppression du compte.
		if id -u ciphera-build >/dev/null 2>&1; then
			grep '^ciphera-build:' /etc/subuid 2>/dev/null | while IFS=: read -r _ s n; do
				usermod --del-subuids "$s-$((s + n - 1))" ciphera-build || true
			done
			grep '^ciphera-build:' /etc/subgid 2>/dev/null | while IFS=: read -r _ s n; do
				usermod --del-subgids "$s-$((s + n - 1))" ciphera-build || true
			done
		fi
		for u in $USERS; do
			if id -u "$u" >/dev/null 2>&1; then
				userdel "$u" || true
			fi
		done
		if grep -qs '^ciphera-build:' /etc/subuid /etc/subgid; then
			say "attention : une plage de ciphera-build reste dans /etc/subuid ou /etc/subgid (a retirer a la main)"
		fi
		say "secrets, donnees (volumes Docker et etat de BuildKit compris) et comptes (plages de ciphera-build comprises) supprimes"
	else
		say "conserves : /etc/ciphera et /etc/ciphera-agent (secrets), /etc/ciphera-updater, /var/lib/ciphera* (dont l'etat de BuildKit), sauvegardes /var/backups/ciphera, volume Docker de PostgreSQL, comptes (--purge pour tout supprimer)"
	fi
}

main "$@"
