#!/bin/sh
# Desinstalle Ciphera : arrete les services, retire binaires et unites, et
# supprime les conteneurs d'infrastructure de Ciphera (PostgreSQL, buildkitd
# et ses aides, etiquette dev.ciphera.managed). Les conteneurs des
# applications des utilisateurs (memes etiquettes, sans role) sont conserves
# et listes ; seul --purge les supprime aussi.
# Les secrets (/etc/ciphera, /etc/ciphera-agent), le reglage du filtrage de
# sortie (/etc/ciphera-egress), les donnees
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
UNITS='ciphera-certsign.path ciphera-certsign.service ciphera-egress-check.timer ciphera-egress-check.service ciphera-updater.path ciphera-updater.service ciphera-updater-recover.service ciphera-agent.service ciphera-proxy.service ciphera-proxy.socket ciphera.service'
USERS='ciphera ciphera-proxy ciphera-agent ciphera-postgres ciphera-build'
TMPFILES_DEST=/etc/tmpfiles.d/ciphera.conf
APPARMOR_DEST=/etc/apparmor.d/ciphera-buildkitd
MANAGED=label=dev.ciphera.managed=true
# Conteneurs d'infrastructure : PostgreSQL (nom fixe, sans etiquette de role)
# et les conteneurs de role buildkitd, buildkit-helper, prepare (agent :
# infra.rs, buildkit.rs, docker.rs). Tout autre conteneur gere est une
# application d'utilisateur.
INFRA_POSTGRES_FILTER='name=^/ciphera-postgres$'
INFRA_ROLES='buildkitd buildkit-helper prepare'
# ciphera-buildkit-cache : cache de BuildKit des versions precedentes.
VOLUMES='ciphera-postgres-data ciphera-buildkit-cache'

say() {
	printf 'ciphera : %s\n' "$*"
}

die() {
	printf 'ciphera : erreur : %s\n' "$*" >&2
	exit 1
}

# Conteneurs geres, services deja arretes (aucun n'est relance) : arret
# propre d'abord (SIGTERM, STOP_TIMEOUT s au plus, puis SIGKILL par Docker),
# puis suppression. Un `docker rm -f` d'emblee tue par SIGKILL, et Docker
# echoue (« PID ... is zombie and can not be killed ») tant que les processus
# du conteneur n'ont pas fini de sortir : nouvel essai, REMOVE_ATTEMPTS au
# plus. Echec : code non nul, derniere erreur de Docker affichee.
STOP_TIMEOUT=30
REMOVE_ATTEMPTS=6
REMOVE_DELAY=5
# Identifiants a supprimer : tous les conteneurs geres avec --purge, sinon
# ceux d'infrastructure seulement (un par ligne).
target_container_ids() {
	if [ "$purge" = yes ]; then
		docker ps -aq --filter "$MANAGED"
		return
	fi
	_all=$(docker ps -aq --filter "$MANAGED" --filter "$INFRA_POSTGRES_FILTER") || return 1
	for _r in $INFRA_ROLES; do
		_one=$(docker ps -aq --filter "$MANAGED" --filter "label=dev.ciphera.role=$_r") || return 1
		_all=$(printf '%s\n%s\n' "$_all" "$_one")
	done
	printf '%s\n' "$_all" | awk 'NF && !seen[$0]++'
}

remove_managed_containers() {
	ids=$(target_container_ids) || return 1
	[ -n "$ids" ] || return 0
	# shellcheck disable=SC2086 # un identifiant par argument
	docker stop -t "$STOP_TIMEOUT" $ids >/dev/null 2>&1 || true
	attempt=1
	while :; do
		ids=$(target_container_ids) || return 1
		[ -n "$ids" ] || return 0
		if [ "$attempt" -ge "$REMOVE_ATTEMPTS" ]; then
			# shellcheck disable=SC2086
			docker rm -f $ids >/dev/null || true
			[ -z "$(target_container_ids)" ]
			return
		fi
		# shellcheck disable=SC2086
		docker rm -f $ids >/dev/null 2>&1 || true
		attempt=$((attempt + 1))
		sleep "$REMOVE_DELAY"
	done
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
	leftover=no
	if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
		have_docker=yes
		if remove_managed_containers; then
			if [ "$purge" = yes ]; then
				say "conteneurs Ciphera supprimes"
			else
				say "conteneurs d'infrastructure de Ciphera supprimes"
				apps=$(docker ps -a --filter "$MANAGED" --format '  {{.Names}} {{.Status}}') || apps=
				if [ -n "$apps" ]; then
					say "conteneurs d'application conserves (--purge les supprime aussi) :"
					printf '%s\n' "$apps"
				fi
			fi
		else
			leftover=yes
			printf 'ciphera : erreur : conteneurs Ciphera restants (relancer uninstall.sh) :\n' >&2
			target_container_ids 2>/dev/null |
				while IFS= read -r cid; do
					docker ps -a --filter "id=$cid" --format '  {{.ID}} {{.Names}} {{.Status}}' >&2 || true
				done
		fi
	fi
	# Filtrage de la sortie des conteneurs (charge par ciphera-agent.service
	# et ciphera-egress-check.timer, arretes plus haut), Docker joignable ou
	# non.
	if [ -x /usr/sbin/nft ]; then
		/usr/sbin/nft delete table inet ciphera_egress >/dev/null 2>&1 || true
	fi
	# Profil de buildkitd : retire seulement une fois son conteneur disparu
	# (sinon un conteneur restant ne pourrait plus redemarrer).
	if [ -e "$APPARMOR_DEST" ]; then
		if [ "$have_docker" = yes ] && [ -z "$(docker ps -aq --filter name=ciphera-buildkitd 2>/dev/null)" ]; then
			if command -v apparmor_parser >/dev/null 2>&1; then
				apparmor_parser -R "$APPARMOR_DEST" >/dev/null 2>&1 || true
			fi
			rm -f "$APPARMOR_DEST"
		else
			say "profil AppArmor $APPARMOR_DEST conserve : Docker injoignable ou conteneur ciphera-buildkitd-* present (relancer uninstall.sh une fois Docker disponible)"
		fi
	fi

	if [ "$purge" = yes ] && [ "$leftover" = yes ]; then
		# Un conteneur encore vivant (buildkitd, PostgreSQL) tient ses
		# dossiers d'etat et de secrets : rien n'est purge sous lui.
		say "purge non faite : conteneurs Ciphera restants ; conserves : /etc/ciphera*, /var/lib/ciphera*, /var/backups/ciphera, volumes Docker, comptes (relancer uninstall.sh --purge)"
	elif [ "$purge" = yes ]; then
		if [ "$have_docker" = yes ]; then
			for v in $VOLUMES; do
				docker volume rm -f "$v" >/dev/null 2>&1 || true
			done
			docker network ls -q --filter "$MANAGED" | xargs -r docker network rm >/dev/null 2>&1 || true
		fi
		rm -rf /etc/ciphera /etc/ciphera-agent /etc/ciphera-updater /etc/ciphera-egress /var/lib/ciphera /var/lib/ciphera-proxy /var/lib/ciphera-agent \
			/var/lib/ciphera-buildkit /var/lib/ciphera-updater /var/backups/ciphera /run/ciphera /run/ciphera-agent \
			/var/log/ciphera-install.log
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
		say "conserves : /etc/ciphera et /etc/ciphera-agent (secrets), /etc/ciphera-updater, /etc/ciphera-egress, /var/lib/ciphera* (dont l'etat de BuildKit), sauvegardes /var/backups/ciphera, volume Docker de PostgreSQL, comptes, journal /var/log/ciphera-install.log (--purge pour tout supprimer)"
	fi
	# Desinstallation incomplete : code non nul.
	[ "$leftover" = no ] || exit 1
}

main "$@"
