#!/bin/sh
# Installeur Ciphera (docs/adr/0003-installation-et-verification-des-releases.md).
#
#   curl -fsSL https://get.ciphera.ch/install.sh | sudo sh
#   curl -fsSL https://get.ciphera.ch/install.sh | sudo sh -s -- --non-interactive
#
# Mode prudent : telecharger install.sh et install.sh.minisig, verifier avec
#   minisign -V -m install.sh -P <une des cles de CIPHERA_PUBKEYS ci-dessous>
# (cles publiees dans install/ciphera.pub du depot et sur get.ciphera.ch),
# lire le script, puis le lancer. Verification complete (minisign, cosign,
# attestation SLSA) : docs/security/release-signing.md.
#
# Rien n'est installe tant que la signature minisign de SHA256SUMS (par une
# des cles integrees ci-dessous) puis le SHA-256 de l'archive ne sont pas verifies.
# Tout le code est dans des fonctions et `main` est appele a la derniere
# ligne : un telechargement coupe n'execute rien.
set -eu

# Cles publiques minisign de confiance des releases, separees par des
# espaces (plusieurs pendant une rotation ; une signature de l'une suffit) :
# marqueur dans le depot, remplace par les cles de install/ciphera.pub a la
# construction de la release (install/release-script.sh). Sans cle injectee,
# le script refuse de s'executer hors mode test (resolve_sources).
CIPHERA_PUBKEYS='@CIPHERA_RELEASE_PUBKEYS@'
DEFAULT_VERSION='v0.1.1'
DEFAULT_BASE_URL='https://get.ciphera.ch'
# Origine des outils tiers (railpack, buildctl, mise) : releases GitHub.
DEFAULT_TOOLS_BASE_URL='https://github.com'

BIN_DIR=/usr/local/bin
SHARE_DIR=/usr/local/share/ciphera
UNIT_DIR=/etc/systemd/system
CONFIG_DIR=/etc/ciphera
# Configuration de l'updater (root seulement, voir write_updater_config).
UPDATER_CONFIG_DIR=/etc/ciphera-updater
# Reglage du filtrage de la sortie des conteneurs (root seulement, lu par
# `ciphera-agent egress`, voir write_egress_config).
EGRESS_CONFIG_DIR=/etc/ciphera-egress
EGRESS_CONFIG=$EGRESS_CONFIG_DIR/egress.env
# Etat et verrou de ciphera-cli updater (journal d'une mise a jour en cours).
UPDATER_STATE_DIR=/var/lib/ciphera-updater
UPDATE_LOCK=/run/ciphera/update.lock
PANEL_PORT=8443
BINARIES='ciphera ciphera-agent ciphera-proxy ciphera-cli'
UNITS='ciphera.service ciphera-proxy.socket ciphera-proxy.service ciphera-agent.service ciphera-updater.service ciphera-updater.path ciphera-updater-recover.service ciphera-egress-check.service ciphera-egress-check.timer ciphera-certsign.service ciphera-certsign.path'
# Dossiers des sockets Unix (PostgreSQL, buildkitd), recrees a chaque demarrage.
TMPFILES=ciphera.tmpfiles.conf
TMPFILES_DEST=/etc/tmpfiles.d/ciphera.conf
# Profil AppArmor applique de buildkitd rootless (la ou AppArmor est actif) :
# variante AppArmor 4 (`apparmor/ciphera-buildkitd`) ou AppArmor 3
# (`apparmor/ciphera-buildkitd.abi3`) selon la version d'apparmor_parser,
# installee sous le meme nom.
APPARMOR_PROFILE=ciphera-buildkitd
APPARMOR_PROFILE_ABI3=ciphera-buildkitd.abi3
APPARMOR_DEST=/etc/apparmor.d/ciphera-buildkitd
APPARMOR_ENABLED=/sys/module/apparmor/parameters/enabled
USERNS_RESTRICT=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
# Comptes systeme : plan de controle, proxy, agent, conteneurs PostgreSQL
# et BuildKit.
USERS='ciphera ciphera-proxy ciphera-agent ciphera-postgres ciphera-build'
# Compte de buildkitd rootless et sa plage de sous-uids/sous-gids.
BUILD_ACCOUNT=ciphera-build
SUBID_MIN=100000
SUBID_COUNT=65536
# Volume du cache de BuildKit des versions precedentes (uid 1000).
LEGACY_BUILDKIT_VOLUME=ciphera-buildkit-cache
# Outils du pipeline de build de l'agent, epingles par version et SHA-256
# (archives officielles des releases GitHub, empreintes publiees par
# GitHub) : railpack (plan de build) et buildctl (client de buildkitd, meme
# version que l'image deploy/containers/buildkit.digest). Telecharges et
# verifies avant toute installation, jamais pendant un build. Source unique
# de ces empreintes : `install.sh --tools-manifest <cible>` les reprend dans
# l'archive de release (tools.manifest), ou ciphera-cli updater les lit.
TOOLS_DIR=/usr/local/lib/ciphera/bin
RAILPACK_VERSION='v0.43.0'
RAILPACK_SHA256_AMD64='248a8f60f36854aabed6124113631078a802de3461660855fc4de9c26065c1fb'
RAILPACK_SHA256_ARM64='732ece14eb4f4676a43ebba19431f954edf7b130acb76159ef0f2ff86ac42ac9'
BUILDKIT_VERSION='v0.35.1'
BUILDKIT_SHA256_AMD64='b248c2b91698aafef11818fe8b681dc5e341581f7d5676709ff45ca1f29b0408'
BUILDKIT_SHA256_ARM64='46c7d1799250bd8b31266ed718715ab9d3464bad0455d5bfbe48e2eeb92ca965'
# mise exact attendu par railpack (core/mise/version.txt de RAILPACK_VERSION) :
# monte en lecture seule dans le bac a sable de `railpack prepare`, qui ne
# telecharge donc jamais un mise non verifie.
MISE_VERSION='2026.10.14'
MISE_SHA256_AMD64='0f95d528ad0b8c54f7022be2ff22a36be149123ff9b27bdaa216d349a9f59c17'
MISE_SHA256_ARM64='8f7fa64c3d08ba515aac501f18f6f43c00c381793185380fd1a92c411c00cdde'

# --- affichage ----------------------------------------------------------------
# Deux modes (docs/operations/installation.md) :
# - simple (hors terminal : CI, journaux, `| tee`) : messages `ciphera : ...`
#   ligne par ligne et sorties des commandes telles quelles, aucune sequence
#   d'echappement ;
# - riche (sorties standard et d'erreur sur un terminal, TERM ni vide ni
#   dumb, NO_COLOR absent ou vide) : logo, une ligne par etape (spinner, puis
#   coche ou croix), avertissements sous l'etape ; tout le reste (say, warn,
#   die, sorties des commandes) va dans le journal (INSTALL_LOG_DIR, dossier
#   root 0700 ; fichier 0600). Le jeton de setup n'y est jamais ecrit : seul
#   ui_finish l'affiche, a l'ecran.
# Logo et symboles en ASCII hors locale UTF-8. Ecran : descripteurs 3
# (sortie standard) et 4 (erreurs), sauves par ui_start ; descripteur 5 :
# journal en lecture (fin affichee en cas d'echec).
INSTALL_LOG_DIR=/var/log/ciphera
INSTALL_LOG_NAME=install.log
INSTALL_LOG=$INSTALL_LOG_DIR/$INSTALL_LOG_NAME
STEP_TOTAL=5
ui_want=plain
ui_mode=plain
ui_utf8=no
ui_active=no
ui_paused=no
ui_label=
ui_spin_pid=
ui_warnings=
ui_warn_count=0

say() {
	printf 'ciphera : %s\n' "$*"
}

warn() {
	printf 'ciphera : attention : %s\n' "$*" >&2
	[ "$ui_mode" = rich ] || return 0
	ui_warn_count=$((ui_warn_count + 1))
	if [ "$ui_active" = yes ]; then
		ui_warnings="$ui_warnings$*
"
	else
		ui_show_warning "$*"
	fi
}

die() {
	printf 'ciphera : erreur : %s\n' "$*" >&2
	if [ "$ui_mode" = rich ]; then
		ui_fail "$*"
	fi
	exit 1
}

# Texte accentue en UTF-8, ASCII sinon.
ui_t() { # <texte UTF-8> <texte ASCII>
	if [ "$ui_utf8" = yes ]; then
		printf '%s' "$1"
	else
		printf '%s' "$2"
	fi
}

# Mode voulu (ui_want), locale (ui_utf8) et couleurs. Bleu Ciphera #2f7fe0
# (--brand-grad-b) : 24 bits si le terminal l'annonce, sinon couleur 33 des
# 256 (la plus proche).
ui_choose_mode() { # <sortie standard sur un terminal : yes|no> <sortie d'erreur : yes|no>
	ui_want=plain
	ui_utf8=no
	case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
	*[Uu][Tt][Ff]-8* | *[Uu][Tt][Ff]8*) ui_utf8=yes ;;
	esac
	[ "$1" = yes ] && [ "$2" = yes ] || return 0
	[ "${TERM:-dumb}" != dumb ] || return 0
	[ -z "${NO_COLOR:-}" ] || return 0
	ui_want=rich
	ui_esc=$(printf '\033')
	case "${COLORTERM:-}" in
	truecolor | 24bit) c_blue="${ui_esc}[38;2;47;127;224m" ;;
	*) c_blue="${ui_esc}[38;5;33m" ;;
	esac
	c_green="${ui_esc}[32m"
	c_red="${ui_esc}[31m"
	c_yellow="${ui_esc}[33m"
	c_bold="${ui_esc}[1m"
	c_dim="${ui_esc}[2m"
	c_reset="${ui_esc}[0m"
	c_eol="${ui_esc}[K"
	c_hide="${ui_esc}[?25l"
	c_show="${ui_esc}[?25h"
	if [ "$ui_utf8" = yes ]; then
		ui_ok='✔'
		ui_ko='✖'
		ui_frames='⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏'
	else
		ui_ok='+'
		ui_ko='x'
		ui_frames='- \ | /'
	fi
}

ui_detect() {
	ui_t1=no
	ui_t2=no
	if [ -t 1 ]; then ui_t1=yes; fi
	if [ -t 2 ]; then ui_t2=yes; fi
	ui_choose_mode "$ui_t1" "$ui_t2"
}

# Dossier du journal : vrai dossier (pas un lien), au compte qui lance
# l'installeur (root), en 0700 ; cree s'il manque. /var/log peut etre
# modifiable par un groupe (root:syslog 0775 sur Ubuntu) : rien n'y est
# ecrit directement.
ui_log_dir_ok() { # <dossier>
	if [ ! -e "$1" ] && [ ! -L "$1" ]; then
		(umask 077 && mkdir "$1") 2>/dev/null || return 1
	fi
	[ -d "$1" ] && [ ! -L "$1" ] || return 1
	[ "$(stat -c %u:%a "$1" 2>/dev/null)" = "$(id -u):700" ]
}

# Journal neuf : cree par mktemp (0600) dans le dossier verifie, ouvert
# (sortie standard et d'erreur ; lecture sur le descripteur 5) AVANT d'etre
# renomme en INSTALL_LOG_NAME, et son chemin reel verifie par /proc : un
# lien pose entre-temps (a la place du dossier ou du fichier) n'est jamais
# suivi pour ecrire ni pour relire. Ecran sauve sur 3 et 4.
# Le chemin reel du dossier doit etre INSTALL_LOG_DIR lui-meme (aucun lien
# sur le chemin), et celui du fichier ouvert exactement
# INSTALL_LOG_DIR/<nom donne par mktemp> : un dossier renomme puis remplace
# par un lien vers un autre dossier (meme de root en 0700) est refuse.
ui_log_open() {
	ui_log_dir_ok "$INSTALL_LOG_DIR" || return 1
	ui_real=$(readlink -f "$INSTALL_LOG_DIR") || return 1
	[ "$ui_real" = "$INSTALL_LOG_DIR" ] || return 1
	ui_new=$(umask 077 && mktemp "$INSTALL_LOG_DIR/.install.XXXXXX" 2>/dev/null) || return 1
	case "$ui_new" in
	"$INSTALL_LOG_DIR"/.install.*) ;;
	*) return 1 ;;
	esac
	exec 3>&1 4>&2
	# Ecriture (1, 2) et relecture (5) du meme journal : voulu.
	# shellcheck disable=SC2094
	exec >>"$ui_new" 2>&1 5<"$ui_new"
	if [ "$(readlink "/proc/$$/fd/1")" != "$ui_new" ] || [ "$(readlink "/proc/$$/fd/5")" != "$ui_new" ] ||
		! mv -fT "$ui_new" "$INSTALL_LOG_DIR/$INSTALL_LOG_NAME" 2>/dev/null; then
		exec >&3 2>&4 5<&- 3>&- 4>&-
		rm -f "$ui_new"
		return 1
	fi
}

# Trap EXIT du mode riche : jamais en echec (terminal disparu, SIGHUP),
# pour que la suite du trap (dossier temporaire) s'execute toujours ; code
# de sortie inchange.
ui_on_exit() { # <code de sortie>
	ui_cleanup "$1" 2>/dev/null || true
}

# Mode riche : journal (ui_log_open), puis logo. Journal impossible : mode
# simple.
ui_start() { # <version>
	[ "$ui_want" = rich ] || return 0
	INSTALL_LOG=$INSTALL_LOG_DIR/$INSTALL_LOG_NAME
	if ! ui_log_open; then
		warn "journal $INSTALL_LOG impossible a ouvrir ($INSTALL_LOG_DIR doit etre un dossier de root en 0700, sans lien) : affichage simple"
		return 0
	fi
	ui_mode=rich
	trap 'ui_on_exit "$?"' EXIT
	trap 'exit 1' HUP INT TERM
	printf "Journal de l'installation de Ciphera %s (%s). Le jeton de setup n'y est jamais ecrit.\n" \
		"$1" "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
	ui_logo "$1"
}

ui_logo() { # <version>
	printf '\n' >&3
	if [ "$ui_utf8" = yes ]; then
		while IFS= read -r ui_line; do
			printf '  %s%s%s%s\n' "$c_bold" "$c_blue" "$ui_line" "$c_reset" >&3
		done <<'EOF'
██████╗ ██╗ ██████╗  ██╗  ██╗ ████████╗ ██████╗   █████╗
██╔════╝ ██║ ██╔══██╗ ██║  ██║ ██╔═════╝ ██╔══██╗  ╚══███╗
██║      ██║ ██████╔╝ ███████║ ██║ ████╗ ██████╔╝   ██╗╚██╗
██║      ██║ ██╔═══╝  ██╔══██║ ██║ ╚═══╝ ██╔══██╗  ██╔╝ ╚██╗
╚██████╗ ██║ ██║      ██║  ██║ ████████╗ ██║  ██║ ██╔╝   ╚██╗
 ╚═════╝ ╚═╝ ╚═╝      ╚═╝  ╚═╝ ╚═══════╝ ╚═╝  ╚═╝ ╚═╝     ╚═╝
EOF
	else
		printf '  %s%sCIPHERA%s\n' "$c_bold" "$c_blue" "$c_reset" >&3
	fi
	printf '\n  %sInstallation de Ciphera %s%s\n\n' "$c_dim" "$1" "$c_reset" >&3
}

# Etape <numero>/STEP_TOTAL. Mode simple : une ligne `ciphera : [n/N] ...`.
ui_step_begin() { # <numero> <libelle UTF-8> <libelle ASCII>
	if [ "$ui_mode" != rich ]; then
		say "[$1/$STEP_TOTAL] $3"
		return 0
	fi
	ui_label="[$1/$STEP_TOTAL] $(ui_t "$2" "$3")"
	ui_active=yes
	ui_warnings=
	printf '\n== [%s/%s] %s ==\n' "$1" "$STEP_TOTAL" "$3"
	printf '%s' "$c_hide" >&3
	ui_spin_start
}

ui_step_end() {
	[ "$ui_mode" = rich ] || return 0
	ui_spin_stop
	printf '\r  %s%s%s %s%s\n' "$c_green" "$ui_ok" "$c_reset" "$ui_label" "$c_eol" >&3
	ui_flush_warnings
	ui_active=no
}

# Spinner : processus d'arriere-plan qui ne redessine que la ligne de
# l'etape et s'arrete seul si l'installeur disparait (aucun orphelin, meme
# apres un SIGKILL) ; arrete par ui_spin_stop (fin d'etape, question, echec,
# trap EXIT). Il ne garde pas le verrou de l'updater (descripteur 9).
ui_spin_start() {
	ui_parent=$$
	(
		trap - EXIT HUP INT
		# Arret : le sleep en cours est tue aussi (aucun processus restant).
		ui_sl=
		trap 'if [ -n "$ui_sl" ]; then kill "$ui_sl" 2>/dev/null; fi; exit 0' TERM
		ui_d=0.1
		sleep "$ui_d" 2>/dev/null || ui_d=1
		while :; do
			for ui_f in $ui_frames; do
				kill -0 "$ui_parent" 2>/dev/null || exit 0
				printf '\r  %s%s%s %s%s' "$c_blue" "$ui_f" "$c_reset" "$ui_label" "$c_eol" >&3
				sleep "$ui_d" &
				ui_sl=$!
				wait "$ui_sl" || true
				ui_sl=
			done
		done
	) 4>&- 5<&- 9>&- </dev/null &
	ui_spin_pid=$!
}

ui_spin_stop() {
	[ -n "$ui_spin_pid" ] || return 0
	kill "$ui_spin_pid" 2>/dev/null || true
	wait "$ui_spin_pid" 2>/dev/null || true
	ui_spin_pid=
}

# Question interactive : spinner suspendu, ligne effacee, curseur visible.
ui_pause() {
	[ "$ui_mode" = rich ] && [ -n "$ui_spin_pid" ] || return 0
	ui_spin_stop
	printf '\r%s%s' "$c_eol" "$c_show" >&3
	ui_paused=yes
}

ui_resume() {
	[ "$ui_paused" = yes ] || return 0
	ui_paused=no
	printf '\r%s%s' "$c_eol" "$c_hide" >&3
	ui_spin_start
}

ui_show_warning() { # <message>
	printf '      %s!%s %s\n' "$c_yellow" "$c_reset" "$1" >&3
}

ui_flush_warnings() {
	[ -n "$ui_warnings" ] || return 0
	printf '%s' "$ui_warnings" | while IFS= read -r ui_w; do
		ui_show_warning "$ui_w"
	done
	ui_warnings=
}

# Caracteres de controle retires (aucune sequence d'echappement venue d'une
# sortie de commande) : C0 et DEL, puis C1 (U+0080 a U+009F, octets C2 80 a
# C2 9F, retirees jusqu a point fixe) en UTF-8 ; hors UTF-8, tout octet
# non ASCII (C1 bruts 80 a 9F compris).
ui_sanitize() {
	if [ "$ui_utf8" = yes ]; then
		LC_ALL=C tr -d '\000-\010\013-\037\177' |
			LC_ALL=C sed -e ":a" -e "s/$(printf '\302')[$(printf '\200')-$(printf '\237')]//" -e "ta"
	else
		LC_ALL=C tr -d '\000-\010\013-\037\177-\377'
	fi
}

# Fin du journal a l'ecran, relue par le descripteur 5 (jamais par son
# chemin), nettoyee par ui_sanitize.
ui_log_tail() {
	printf "\n  Journal d'installation (%s) : %s\n" "$(ui_t 'dernières lignes' 'dernieres lignes')" \
		"$INSTALL_LOG" >&4
	tail -n 15 <&5 2>/dev/null | ui_sanitize |
		while IFS= read -r ui_l; do
			printf '    %s|%s %s\n' "$c_dim" "$c_reset" "$ui_l"
		done >&4
	printf '\n' >&4
}

# Echec (die) : croix sur l'etape, avertissements, erreur, fin du journal.
ui_fail() { # <message>
	ui_spin_stop
	if [ "$ui_active" = yes ]; then
		printf '\r  %s%s%s %s%s\n' "$c_red" "$ui_ko" "$c_reset" "$ui_label" "$c_eol" >&3
		ui_flush_warnings
		ui_active=no
	fi
	printf '\n  %s%sciphera : erreur : %s%s\n' "$c_bold" "$c_red" "$1" "$c_reset" >&4
	ui_log_tail
	printf '%s' "$c_show" >&3
}

# Trap EXIT du mode riche : spinner arrete, curseur reaffiche ; une etape
# encore ouverte (interruption, commande en echec hors die) est marquee.
ui_cleanup() { # <code de sortie>
	[ "$ui_mode" = rich ] || return 0
	ui_spin_stop
	if [ "$ui_active" = yes ]; then
		printf 'ciphera : erreur : installation interrompue (code %s)\n' "$1"
		printf '\r  %s%s%s %s%s\n' "$c_red" "$ui_ko" "$c_reset" "$ui_label" "$c_eol" >&3
		ui_flush_warnings
		ui_active=no
		printf '\n  %s%sciphera : erreur : installation interrompue (code %s)%s\n' \
			"$c_bold" "$c_red" "$1" "$c_reset" >&4
		ui_log_tail
	fi
	printf '%s' "$c_show" >&3
}

# Sortie d'erreur d'une commande gardee dans <fichier> : telle quelle en mode
# simple, en avertissements visibles en mode riche.
ui_relay_stderr() { # <fichier>
	[ -s "$1" ] || return 0
	if [ "$ui_mode" = rich ]; then
		while IFS= read -r ui_l; do
			warn "$ui_l"
		done <"$1"
	else
		cat "$1" >&2
	fi
}

# Fin de l'installation. Mode simple : message et rapport de `ciphera init`
# inchanges. Mode riche : « Installation terminee », adresse de l'assistant
# mise en valeur avec ses explications ; rien du rapport dans le journal.
ui_finish() { # <version> <rapport de ciphera init, peut etre vide> <IP du panneau, peut etre vide>
	say "Ciphera $1 est installe et demarre."
	if [ "$ui_mode" != rich ]; then
		if [ -n "$2" ]; then
			printf '\n%s\n' "$2"
		fi
		return 0
	fi
	if [ -n "$2" ]; then
		say "adresse de l'assistant affichee a l'ecran seulement (jamais journalisee)"
	fi
	printf '\n  %s%s%s %s%s\n' "$c_bold" "$c_green" "$ui_ok" \
		"$(ui_t "Installation terminée : Ciphera $1 est installé et démarré." \
			"Installation terminee : Ciphera $1 est installe et demarre.")" "$c_reset" >&3
	if [ -n "$2" ]; then
		printf '\n' >&3
		printf '%s\n' "$2" | while IFS= read -r ui_l; do
			case "$ui_l" in
			https://*) printf '\n      %s%s%s%s\n\n' "$c_bold" "$c_blue" "$ui_l" "$c_reset" ;;
			'Empreinte SHA-256 :'*) printf '  %s%s%s\n' "$c_bold" "$ui_l" "$c_reset" ;;
			*) printf '  %s\n' "$ui_l" ;;
			esac
		done >&3
	elif [ -n "$3" ]; then
		ui_host=$3
		case "$3" in
		*:*) ui_host="[$3]" ;;
		esac
		printf '\n  Panneau : %s%shttps://%s:%s%s\n' "$c_bold" "$c_blue" \
			"$ui_host" "${PANEL_PORT:-8443}" "$c_reset" >&3
		printf '  %s\n' "$(ui_t "Instance déjà initialisée ; tant qu'aucun compte n'existe, nouveau lien de l'assistant : sudo ciphera admin setup-token" \
			"Instance deja initialisee ; tant qu'aucun compte n'existe, nouveau lien de l'assistant : sudo ciphera admin setup-token")" >&3
	fi
	if [ "$ui_warn_count" -gt 0 ]; then
		printf '\n  %s!%s %s\n' "$c_yellow" "$c_reset" \
			"$(ui_t "$ui_warn_count avertissement(s) affiché(s) plus haut." "$ui_warn_count avertissement(s) affiche(s) plus haut.")" >&3
	fi
	printf "\n  %sJournal d'installation : %s%s\n\n" "$c_dim" "$INSTALL_LOG" "$c_reset" >&3
	printf '%s' "$c_show" >&3
}
# --- fin de l'affichage -------------------------------------------------------

usage() {
	cat <<'EOF'
Usage : install.sh [options]

  --version vX.Y.Z     version a installer (defaut : celle de ce script)
  --public-ip IP       adresse IP publique du serveur (defaut : detectee)
  --non-interactive    aucune question ; Docker n'est installe qu'avec --install-docker
  --install-docker     installe Docker Engine (paquet de la distribution) s'il manque
  --tools-manifest CIBLE
                       affiche les outils tiers epingles pour CIBLE et s'arrete
                       (construction des releases, install/package.sh)
  -h, --help           cette aide

Variable d'environnement :
  CIPHERA_APP_EGRESS_DENY_PRIVATE=0|1
                       1 : les applications ne joignent pas non plus les plages
                       privees (10/8, 172.16/12, 192.168/16, 100.64/10, fc00::/7) ;
                       0 (defaut) : permises. Ecrite dans /etc/ciphera-egress/egress.env
                       (fichier garde tel quel a la reinstallation sans cette variable).
EOF
}

# --- arguments et sources -----------------------------------------------------

check_version() {
	case "$1" in
	v[0-9]*.[0-9]*.[0-9]*) ;;
	*) die "version invalide (attendu vX.Y.Z)" ;;
	esac
	case "$1" in
	*[!v0-9.]* | *.*.*.* | v*v*) die "version invalide (attendu vX.Y.Z)" ;;
	esac
}

check_ip() {
	case "$1" in
	'' | *[!0-9A-Fa-f:.]*) die "adresse IP invalide" ;;
	esac
}

parse_args() {
	version=$DEFAULT_VERSION
	public_ip=
	interactive=yes
	install_docker=no
	manifest_target=
	while [ "$#" -gt 0 ]; do
		case "$1" in
		--version)
			[ "$#" -ge 2 ] || die "--version attend une valeur"
			version=$2
			shift 2
			;;
		--public-ip)
			[ "$#" -ge 2 ] || die "--public-ip attend une valeur"
			public_ip=$2
			shift 2
			;;
		--non-interactive)
			interactive=no
			shift
			;;
		--install-docker)
			install_docker=yes
			shift
			;;
		--tools-manifest)
			[ "$#" -ge 2 ] || die "--tools-manifest attend une cible"
			manifest_target=$2
			shift 2
			;;
		-h | --help)
			usage
			exit 0
			;;
		*) die "option inconnue (voir --help)" ;;
		esac
	done
	check_version "$version"
	if [ -n "$public_ip" ]; then
		check_ip "$public_ip"
	fi
	case "${CIPHERA_APP_EGRESS_DENY_PRIVATE:-}" in
	'' | 0 | 1) ;;
	*) die "CIPHERA_APP_EGRESS_DENY_PRIVATE : 0 ou 1 attendu" ;;
	esac
}

# >>> mode-test
# Surcharges reservees aux tests de bout en bout (install/tests/run.sh) :
# refusees si CIPHERA_INSTALL_TEST ne vaut pas 1. Ce bloc est retire du
# script publie par install/release-script.sh : seul le script du depot
# les porte.

# URL de test <nom> <valeur> : mode test exige, https, caracteres surs.
check_test_url() {
	[ "$test_mode" = yes ] ||
		die "$1 est reservee aux tests (CIPHERA_INSTALL_TEST=1) : refusee"
	case "$2" in
	https://*) ;;
	*) die "$1 doit commencer par https://" ;;
	esac
	case "$2" in
	*[!A-Za-z0-9:/._-]*) die "$1 contient des caracteres interdits" ;;
	esac
}

apply_test_overrides() {
	test_mode=no
	if [ "${CIPHERA_INSTALL_TEST:-}" = 1 ]; then
		test_mode=yes
	fi

	if [ -n "${CIPHERA_RELEASE_BASE_URL:-}" ] && [ "$CIPHERA_RELEASE_BASE_URL" != "$DEFAULT_BASE_URL" ]; then
		check_test_url CIPHERA_RELEASE_BASE_URL "$CIPHERA_RELEASE_BASE_URL"
		base_url=$CIPHERA_RELEASE_BASE_URL
		warn "MODE TEST : releases lues depuis $base_url"
	fi

	# Miroir des outils tiers (meme arborescence que github.com) : seule
	# l'origine change, les empreintes epinglees restent verifiees.
	if [ -n "${CIPHERA_TOOLS_BASE_URL:-}" ] && [ "$CIPHERA_TOOLS_BASE_URL" != "$DEFAULT_TOOLS_BASE_URL" ]; then
		check_test_url CIPHERA_TOOLS_BASE_URL "$CIPHERA_TOOLS_BASE_URL"
		tools_base_url=$CIPHERA_TOOLS_BASE_URL
		warn "MODE TEST : outils tiers lus depuis $tools_base_url (SHA-256 epingles verifies)"
	fi

	if [ -n "${CIPHERA_TEST_MINISIGN_PUBKEY:-}" ]; then
		[ "$test_mode" = yes ] ||
			die "CIPHERA_TEST_MINISIGN_PUBKEY est reservee aux tests (CIPHERA_INSTALL_TEST=1) : refusee"
		valid_key "$CIPHERA_TEST_MINISIGN_PUBKEY" || die "CIPHERA_TEST_MINISIGN_PUBKEY invalide"
		pubkeys=$CIPHERA_TEST_MINISIGN_PUBKEY
		warn "MODE TEST : cle minisign de test"
	fi
}
# <<< mode-test

# Sources des releases et des outils : adresses et cles publiques de
# confiance (les surcharges de test ne sont presentes que dans le depot).
resolve_sources() {
	base_url=$DEFAULT_BASE_URL
	pubkeys=$CIPHERA_PUBKEYS
	tools_base_url=$DEFAULT_TOOLS_BASE_URL

	# >>> mode-test
	apply_test_overrides
	# <<< mode-test

	# install.sh du depot (cle non injectee) : jamais d'installation.
	[ -n "$pubkeys" ] || die "cle publique des releases absente"
	for k in $pubkeys; do
		valid_key "$k" ||
			die "cle publique des releases absente : ce fichier est la source du depot, pas le script publie (curl -fsSL $DEFAULT_BASE_URL/install.sh | sudo sh)"
	done
}

# Cle publique minisign : RW + base64, 56 caracteres.
valid_key() {
	case "$1" in
	RW*) ;;
	*) return 1 ;;
	esac
	case "$1" in
	*[!A-Za-z0-9+/=]*) return 1 ;;
	esac
	[ "${#1}" -eq 56 ]
}

# Code 0 si <fichier> porte une signature minisign <fichier>.minisig valide
# pour l'une des cles de confiance.
signed_by_trusted_key() { # <fichier>
	for k in $pubkeys; do
		if minisign -V -q -P "$k" -m "$1" -x "$1.minisig" >/dev/null 2>&1; then
			return 0
		fi
	done
	return 1
}

# --- machine cible ------------------------------------------------------------

need_root() {
	[ "$(id -u)" -eq 0 ] ||
		die "droits root requis : curl -fsSL $DEFAULT_BASE_URL/install.sh | sudo sh"
}

# Lit un champ de /etc/os-release sans executer le fichier.
os_release_field() {
	sed -n "s/^$1=//p" /etc/os-release | head -n 1 | tr -d "\"'"
}

detect_platform() {
	[ "$(uname -s)" = Linux ] || die "systeme non pris en charge (Linux seulement)"
	case "$(uname -m)" in
	x86_64 | amd64) target=x86_64-unknown-linux-musl ;;
	aarch64 | arm64) target=aarch64-unknown-linux-musl ;;
	*) die "architecture non prise en charge : $(uname -m) (amd64 ou arm64 seulement)" ;;
	esac
	[ -r /etc/os-release ] || die "/etc/os-release introuvable : distribution inconnue"
	os_id=$(os_release_field ID)
	case "$os_id" in
	debian | ubuntu) pkg=apt ;;
	fedora) pkg=dnf ;;
	*) die "distribution non prise en charge : $os_id (Debian, Ubuntu ou Fedora seulement)" ;;
	esac
	[ -d /run/systemd/system ] || die "systemd est requis (aucun systemd actif detecte)"
}

require_tools() {
	for t in curl tar gzip sha256sum awk install useradd usermod getent systemctl systemd-tmpfiles; do
		command -v "$t" >/dev/null 2>&1 || die "outil requis absent : $t"
	done
}

pkg_install() {
	case "$pkg" in
	apt)
		DEBIAN_FRONTEND=noninteractive apt-get update -qq ||
			die "apt-get update a echoue"
		DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "$@" >/dev/null ||
			die "installation du paquet $* impossible"
		;;
	dnf)
		dnf install -y -q "$@" >/dev/null || die "installation du paquet $* impossible"
		;;
	esac
}

# minisign vient du depot signe de la distribution : aucun binaire telecharge
# n'est execute avant la verification.
ensure_minisign() {
	if command -v minisign >/dev/null 2>&1; then
		return 0
	fi
	say "minisign absent : installation du paquet de la distribution"
	pkg_install minisign
	command -v minisign >/dev/null 2>&1 || die "minisign introuvable apres installation"
}

# --- telechargement et verification -------------------------------------------

regular_file() {
	[ -f "$1" ] && [ ! -L "$1" ]
}

# Plafonds de telechargement : temps total (s) et taille (octets), pour que
# ni un serveur lent ni une reponse sans fin ne bloque ou ne remplisse le
# disque. Petits fichiers (SHA256SUMS, signatures) et archives. TOTAL : duree
# maximale cumulee des essais (--retry-max-time).
FETCH_SMALL_TIME=30
FETCH_SMALL_MAX=1048576
FETCH_SMALL_TOTAL=120
FETCH_ARCHIVE_TIME=600
FETCH_ARCHIVE_MAX=536870912
FETCH_ARCHIVE_TOTAL=1800

# fetch <url> <sortie> [archive] : « archive » releve les plafonds.
fetch() {
	_ftime=$FETCH_SMALL_TIME
	_fmax=$FETCH_SMALL_MAX
	_ftotal=$FETCH_SMALL_TOTAL
	if [ "${3:-}" = archive ]; then
		_ftime=$FETCH_ARCHIVE_TIME
		_fmax=$FETCH_ARCHIVE_MAX
		_ftotal=$FETCH_ARCHIVE_TOTAL
	fi
	curl --proto '=https' --tlsv1.2 -fsSL --retry 3 \
		--max-time "$_ftime" --retry-max-time "$_ftotal" --max-filesize "$_fmax" -o "$2" "$1" ||
		die "telechargement impossible : $1"
}

download_and_verify() {
	archive="ciphera-$version-$target.tar.gz"
	release_url="$base_url/releases/$version"
	say "telechargement de Ciphera $version ($target)"

	fetch "$release_url/SHA256SUMS" "$tmp/SHA256SUMS"
	fetch "$release_url/SHA256SUMS.minisig" "$tmp/SHA256SUMS.minisig"
	signed_by_trusted_key "$tmp/SHA256SUMS" ||
		die "signature minisign de SHA256SUMS invalide : rien n'est installe"
	say "signature minisign verifiee"

	# Seule la ligne de l'archive choisie est verifiee.
	awk -v f="$archive" '$2 == f || $2 == ("*" f)' "$tmp/SHA256SUMS" >"$tmp/SHA256SUMS.selected"
	[ "$(wc -l <"$tmp/SHA256SUMS.selected")" -eq 1 ] ||
		die "SHA256SUMS ne contient pas exactement une ligne pour $archive : rien n'est installe"
	awk '{ exit ($1 ~ /^[0-9a-f]+$/ && length($1) == 64) ? 0 : 1 }' "$tmp/SHA256SUMS.selected" ||
		die "empreinte SHA-256 mal formee : rien n'est installe"

	fetch "$release_url/$archive" "$tmp/$archive" archive
	(cd "$tmp" && sha256sum -c --status SHA256SUMS.selected) ||
		die "SHA-256 de $archive incorrect : rien n'est installe"
	say "SHA-256 verifie"

	mkdir "$tmp/x"
	tar -xzf "$tmp/$archive" -C "$tmp/x" --no-same-owner ||
		die "archive illisible : rien n'est installe"
	pkg_dir="$tmp/x/ciphera-$version-$target"
	for f in $BINARIES; do
		regular_file "$pkg_dir/bin/$f" || die "archive incomplete ($f) : rien n'est installe"
	done
	for f in $UNITS $TMPFILES; do
		regular_file "$pkg_dir/systemd/$f" || die "archive incomplete ($f) : rien n'est installe"
	done
	for f in $APPARMOR_PROFILE $APPARMOR_PROFILE_ABI3; do
		regular_file "$pkg_dir/apparmor/$f" || die "archive incomplete ($f) : rien n'est installe"
	done
}

# Code 0 si le SHA-256 de <fichier> vaut <empreinte>.
sha256_matches() { # <fichier> <empreinte>
	printf '%s  %s\n' "$2" "$1" | sha256sum -c --status -
}

# Outils tiers epingles pour <cible>, une ligne par outil :
#   <nom> <version> <sha256> <chemin sous l'origine> <membre de l'archive> <nom installe>
# Lu par download_build_tools et install_files, et copie tel quel dans
# l'archive de release (tools.manifest) pour ciphera-cli updater : les
# empreintes ne sont ecrites qu'ici.
tools_manifest() { # <cible>
	case "$1" in
	x86_64-unknown-linux-musl)
		rp_arch=x86_64
		bk_arch=amd64
		rp_sum=$RAILPACK_SHA256_AMD64
		bk_sum=$BUILDKIT_SHA256_AMD64
		mise_arch=x64
		mise_sum=$MISE_SHA256_AMD64
		;;
	aarch64-unknown-linux-musl)
		rp_arch=arm64
		bk_arch=arm64
		rp_sum=$RAILPACK_SHA256_ARM64
		bk_sum=$BUILDKIT_SHA256_ARM64
		mise_arch=arm64
		mise_sum=$MISE_SHA256_ARM64
		;;
	*) die "cible invalide (x86_64-unknown-linux-musl ou aarch64-unknown-linux-musl)" ;;
	esac
	printf '%s %s %s %s %s %s\n' \
		railpack "$RAILPACK_VERSION" "$rp_sum" \
		"railwayapp/railpack/releases/download/$RAILPACK_VERSION/railpack-$RAILPACK_VERSION-$rp_arch-unknown-linux-musl.tar.gz" \
		railpack railpack \
		buildctl "$BUILDKIT_VERSION" "$bk_sum" \
		"moby/buildkit/releases/download/$BUILDKIT_VERSION/buildkit-$BUILDKIT_VERSION.linux-$bk_arch.tar.gz" \
		bin/buildctl buildctl \
		mise "$MISE_VERSION" "$mise_sum" \
		"jdx/mise/releases/download/v$MISE_VERSION/mise-v$MISE_VERSION-linux-$mise_arch-musl.tar.gz" \
		mise/bin/mise "mise-$MISE_VERSION"
}

# Outils du manifeste pour l'architecture cible, verifies dans $tmp/tools.
download_build_tools() {
	tools_manifest "$target" >"$tmp/tools.manifest"
	say "telechargement des outils de build (railpack $RAILPACK_VERSION, buildctl $BUILDKIT_VERSION, mise $MISE_VERSION)"
	mkdir "$tmp/tools"
	while read -r _name _version sum path member dest; do
		file=${path##*/}
		fetch "$tools_base_url/$path" "$tmp/$file" archive
		sha256_matches "$tmp/$file" "$sum" ||
			die "SHA-256 de $file incorrect : rien n'est installe"
		mkdir "$tmp/tools/$dest.d"
		tar -xzf "$tmp/$file" -C "$tmp/tools/$dest.d" --no-same-owner "$member" ||
			die "archive $file illisible : rien n'est installe"
		regular_file "$tmp/tools/$dest.d/$member" ||
			die "outils de build absents des archives : rien n'est installe"
		mv "$tmp/tools/$dest.d/$member" "$tmp/tools/$dest"
	done <"$tmp/tools.manifest"
	say "SHA-256 des outils de build verifies"
}

# --- installation -------------------------------------------------------------

# Compte deja present : accepte seulement s'il est un compte systeme (uid
# inferieur a UID_MIN de login.defs, 1000 a defaut), sans shell de connexion
# (nologin ou false) et de groupe principal homonyme. Un compte humain ou
# ouvert reutilise sous ce nom heriterait des droits de Ciphera (groupe,
# fichiers de /etc/ciphera, secrets).
check_account_shape() { # <compte>
	_entry=$(getent passwd "$1" 2>/dev/null) || return 0
	_uid=$(printf '%s\n' "$_entry" | awk -F: '{ print $3 }')
	_shell=$(printf '%s\n' "$_entry" | awk -F: '{ print $7 }')
	_umin=$(login_defs_min UID_MIN)
	case "$_uid" in
	'' | *[!0-9]*) die "le compte $1 existe deja avec un uid illisible : refuse (supprimer ce compte puis relancer install.sh)" ;;
	esac
	if [ "$_uid" -eq 0 ] || [ "$_uid" -ge "$_umin" ]; then
		die "le compte $1 existe deja avec l'uid $_uid : un compte systeme (uid de 1 a $((_umin - 1))) est exige (supprimer ce compte puis relancer install.sh)"
	fi
	case "$_shell" in
	*/nologin | */false) ;;
	*) die "le compte $1 existe deja avec un shell de connexion (${_shell:-aucun}) : nologin ou false exige (supprimer ce compte puis relancer install.sh)" ;;
	esac
	_pgroup=$(id -gn "$1" 2>/dev/null) || die "groupe principal du compte $1 illisible : refuse"
	[ "$_pgroup" = "$1" ] ||
		die "le compte $1 existe deja avec le groupe principal $_pgroup : le groupe $1 est exige (supprimer ce compte puis relancer install.sh)"
}

ensure_user() {
	if id -u "$1" >/dev/null 2>&1; then
		check_account_shape "$1"
		return 0
	fi
	nologin=$(command -v nologin || printf '/usr/sbin/nologin')
	useradd --system --user-group --no-create-home --home-dir /nonexistent \
		--shell "$nologin" "$1" || die "creation du compte $1 impossible"
}

can_prompt() {
	[ "$interactive" = yes ] && (: </dev/tty) 2>/dev/null
}

install_docker_engine() {
	say "installation de Docker Engine (paquet de la distribution)"
	case "$pkg" in
	apt) pkg_install docker.io ;;
	dnf) pkg_install moby-engine ;;
	esac
	systemctl enable --now docker.service || die "demarrage de Docker impossible"
	have_docker=yes
}

setup_docker() {
	have_docker=no
	if command -v docker >/dev/null 2>&1; then
		have_docker=yes
		return 0
	fi
	if [ "$install_docker" = yes ]; then
		install_docker_engine
		return 0
	fi
	if can_prompt; then
		ui_pause
		printf 'Docker Engine est absent. Installer le paquet de la distribution ? [o/N] ' >/dev/tty
		answer=
		read -r answer </dev/tty || answer=
		ui_resume
		case "$answer" in
		o | O | oui | y | Y | yes)
			install_docker_engine
			return 0
			;;
		esac
	fi
	warn "Docker Engine absent : non installe (relancer avec --install-docker pour l'installer)."
	warn "ciphera-agent est installe mais ne sera demarre qu'avec Docker."
}

install_files() {
	for b in $BINARIES; do
		install -m 0755 -o root -g root "$pkg_dir/bin/$b" "$BIN_DIR/.$b.new"
		mv -f "$BIN_DIR/.$b.new" "$BIN_DIR/$b"
	done
	install -d -m 0755 "$SHARE_DIR/containers"
	install -d -m 0755 "$TOOLS_DIR"
	while read -r _name _version _sum _path _member dest; do
		install -m 0755 -o root -g root "$tmp/tools/$dest" "$TOOLS_DIR/.$dest.new"
		mv -f "$TOOLS_DIR/.$dest.new" "$TOOLS_DIR/$dest"
	done <"$tmp/tools.manifest"
	for f in "$pkg_dir"/containers/*.digest; do
		install -m 0644 -o root -g root "$f" "$SHARE_DIR/containers/"
	done
	# Version installee, lue par ciphera-cli updater (refus des versions
	# inferieures ou egales).
	printf '%s\n' "$version" >"$SHARE_DIR/.VERSION.new"
	chmod 0644 "$SHARE_DIR/.VERSION.new"
	mv -f "$SHARE_DIR/.VERSION.new" "$SHARE_DIR/VERSION"
	for u in $UNITS; do
		install -m 0644 -o root -g root "$pkg_dir/systemd/$u" "$UNIT_DIR/$u"
	done
	# Registre des unites installees par Ciphera, lu par ciphera-cli updater :
	# seules ces unites sont desactivees ou retirees par une mise a jour.
	for u in $UNITS; do
		printf '%s\n' "$u"
	done >"$SHARE_DIR/.units.list.new"
	chmod 0644 "$SHARE_DIR/.units.list.new"
	mv -f "$SHARE_DIR/.units.list.new" "$SHARE_DIR/units.list"
	install -m 0644 -o root -g root "$pkg_dir/systemd/$TMPFILES" "$TMPFILES_DEST"
	systemd-tmpfiles --create "$TMPFILES_DEST" || die "creation des dossiers de sockets impossible"
	write_updater_config
	write_egress_config
	install_apparmor_profile
	systemctl daemon-reload
	say "binaires installes dans $BIN_DIR, unites dans $UNIT_DIR"
}

# Reglage du filtrage de la sortie des conteneurs, lu en root par
# `ciphera-agent egress` (qui refuse un fichier d'un autre proprietaire que
# root ou modifiable par d'autres) : dossier root 0755, fichier root 0644.
# Ecrit s'il est absent (defaut : plages privees permises aux applications)
# ou si CIPHERA_APP_EGRESS_DENY_PRIVATE est donnee ; sinon garde, droits
# remis.
write_egress_config() {
	install -d -m 0755 -o root -g root "$EGRESS_CONFIG_DIR"
	if [ ! -f "$EGRESS_CONFIG" ] || [ -n "${CIPHERA_APP_EGRESS_DENY_PRIVATE:-}" ]; then
		{
			printf '# Filtrage de la sortie des applications (ciphera-agent egress).\n'
			printf '# 1 : plages privees (10/8, 172.16/12, 192.168/16, 100.64/10, fc00::/7)\n'
			printf '# interdites aux applications ; 0 : permises. Applique en 5 min au plus\n'
			printf '# (ciphera-egress-check.timer) ou par systemctl restart ciphera-agent.\n'
			printf 'CIPHERA_APP_EGRESS_DENY_PRIVATE=%s\n' "${CIPHERA_APP_EGRESS_DENY_PRIVATE:-0}"
		} >"$EGRESS_CONFIG_DIR/.egress.env.new"
		mv -f "$EGRESS_CONFIG_DIR/.egress.env.new" "$EGRESS_CONFIG"
	fi
	chown root:root "$EGRESS_CONFIG"
	chmod 0644 "$EGRESS_CONFIG"
}

# Configuration de ciphera-cli updater (root) : jamais /etc/ciphera/ciphera.env
# (fichier du service, dans un dossier du service : il ne doit jamais pouvoir
# donner root). Dossier root 0755, fichier root 0600,
# cles lues par l'updater lui-meme (toute autre cle est ignoree). Un fichier
# existant (reglages de l'administrateur) est garde, droits remis.
write_updater_config() {
	install -d -m 0755 -o root -g root "$UPDATER_CONFIG_DIR"
	if [ ! -f "$UPDATER_CONFIG_DIR/updater.env" ]; then
		{
			printf '# Configuration de ciphera-cli updater (root). Memes valeurs que le\n'
			printf '# plan de controle si /etc/ciphera/ciphera.env les change.\n'
			printf 'CIPHERA_PANEL_LOCAL_PORT=8444\n'
			printf 'CIPHERA_DB_USER=ciphera\n'
			printf 'CIPHERA_DB_NAME=ciphera\n'
			printf 'CIPHERA_DATA_DIR=/var/lib/ciphera\n'
		} >"$UPDATER_CONFIG_DIR/.updater.env.new"
		mv -f "$UPDATER_CONFIG_DIR/.updater.env.new" "$UPDATER_CONFIG_DIR/updater.env"
	fi
	chown root:root "$UPDATER_CONFIG_DIR/updater.env"
	chmod 0600 "$UPDATER_CONFIG_DIR/updater.env"
}

# Variante du profil de buildkitd pour l'apparmor_parser de la machine :
# AppArmor 4 a partir de la version 4, AppArmor 3 sinon (version illisible
# comprise : l'ABI 3.0 est aussi lue par les parsers 4).
apparmor_variant() {
	major=$(apparmor_parser --version 2>/dev/null |
		sed -n 's/^AppArmor parser version \([0-9][0-9]*\)\..*/\1/p' | head -n 1)
	if [ -n "$major" ] && [ "$major" -ge 4 ]; then
		printf '%s\n' "$APPARMOR_PROFILE"
	else
		printf '%s\n' "$APPARMOR_PROFILE_ABI3"
	fi
}

# Le conteneur de buildkitd rootless tourne sous un profil AppArmor applique
# (deploy/apparmor/ : espaces de noms utilisateur permis, /proc et /sys
# sensibles refuses), charge la ou AppArmor est actif ; le reglage global de
# la machine n'est pas modifie. L'agent l'utilise s'il est present ; s'il
# n'est pas charge, buildkitd ne demarre pas (aucun repli sans profil).
install_apparmor_profile() {
	if [ "$(cat "$APPARMOR_ENABLED" 2>/dev/null)" != Y ]; then
		return 0
	fi
	restrict=$(cat "$USERNS_RESTRICT" 2>/dev/null || true)
	if ! command -v apparmor_parser >/dev/null 2>&1; then
		warn "ATTENTION : AppArmor est actif mais apparmor_parser est absent : le profil $APPARMOR_PROFILE n'est pas installe."
		if [ "$restrict" = 1 ]; then
			warn "Le noyau restreint les espaces de noms utilisateur ($USERNS_RESTRICT = 1) : BuildKit (conteneurs ciphera-buildkitd-<projet>) NE DEMARRERA PAS."
		else
			warn "BuildKit (conteneurs ciphera-buildkitd-<projet>) tournera sans profil AppArmor."
		fi
		warn "Installer le paquet apparmor puis relancer install.sh."
		return 0
	fi
	variant=$(apparmor_variant)
	if [ "$variant" = "$APPARMOR_PROFILE_ABI3" ] && [ "$restrict" = 1 ]; then
		warn "ATTENTION : le noyau restreint les espaces de noms utilisateur ($USERNS_RESTRICT = 1) mais apparmor_parser est en version 3 :"
		warn "le profil AppArmor 3 ne sait pas les autoriser, BuildKit pourrait ne pas demarrer (selon le noyau, par exemple Ubuntu 22.04 avec noyau HWE)."
		warn "Verifier apres un premier build : en cas d'echec, journalctl -k | grep 'apparmor=\"DENIED\"' montre les refus de ciphera-buildkitd."
		warn "Si c'est le cas : mettre AppArmor a jour (4 ou plus) ou remettre $USERNS_RESTRICT a 0."
	fi
	install -d -m 0755 /etc/apparmor.d
	install -m 0644 -o root -g root "$pkg_dir/apparmor/$variant" "$APPARMOR_DEST"
	apparmor_parser -r -W "$APPARMOR_DEST" ||
		die "chargement du profil AppArmor $APPARMOR_PROFILE ($variant) impossible"
	say "profil AppArmor $APPARMOR_PROFILE charge ($variant, buildkitd rootless)"
}

# buildkitd rootless tourne sous ciphera-build : un compte local de meme uid
# atteindrait son socket et son espace de noms (builds, cache, secrets).
# Compte systeme exige : uid propre, ni 0 ni 1000 ni dans la plage des
# comptes humains (UID_MIN de /etc/login.defs, 1000 par defaut) ; groupe
# principal ni 0 ni humain (GID_MIN) ni groupe principal d'un autre compte.
login_defs_min() { # <UID_MIN|GID_MIN>
	v=$(awk -v k="$1" '$1 == k { print $2; exit }' /etc/login.defs 2>/dev/null || true)
	case "$v" in
	'' | *[!0-9]*) v=1000 ;;
	esac
	printf '%s\n' "$v"
}

# Compte de PostgreSQL : jamais root (uid ou groupe principal 0), sinon la
# base tournerait en root dans son conteneur ; l'agent le refuse aussi.
check_postgres_account() {
	pg_uid=$(id -u ciphera-postgres 2>/dev/null) || return 0
	pg_gid=$(id -g ciphera-postgres) || die "groupe du compte ciphera-postgres illisible"
	if [ "$pg_uid" -eq 0 ] || [ "$pg_gid" -eq 0 ]; then
		die "le compte ciphera-postgres a l'uid $pg_uid et le groupe $pg_gid : root refuse pour PostgreSQL (supprimer ce compte puis relancer install.sh)"
	fi
}

check_build_account() {
	if ! build_uid=$(id -u "$BUILD_ACCOUNT" 2>/dev/null) || ! build_gid=$(id -g "$BUILD_ACCOUNT" 2>/dev/null); then
		die "compte $BUILD_ACCOUNT introuvable (creation echouee ?) : buildkitd ne peut pas etre configure"
	fi
	uid_min=$(login_defs_min UID_MIN)
	gid_min=$(login_defs_min GID_MIN)
	if [ "$build_uid" -eq 0 ] || [ "$build_uid" -eq 1000 ] || [ "$build_uid" -ge "$uid_min" ]; then
		die "le compte $BUILD_ACCOUNT a l'uid $build_uid : buildkitd exige un compte systeme d'uid inferieur a $uid_min, ni 0 ni 1000 (supprimer ce compte puis relancer install.sh)"
	fi
	if [ "$build_gid" -eq 0 ] || [ "$build_gid" -eq 1000 ] || [ "$build_gid" -ge "$gid_min" ]; then
		die "le compte $BUILD_ACCOUNT a le groupe principal $build_gid : buildkitd exige un groupe systeme de gid inferieur a $gid_min, ni 0 ni 1000 (supprimer ce compte puis relancer install.sh)"
	fi
	same_uid=$(getent passwd | awk -F: -v u="$build_uid" '$3 == u { n++ } END { print n + 0 }')
	[ "$same_uid" -eq 1 ] ||
		die "l'uid $build_uid du compte $BUILD_ACCOUNT est partage avec un autre compte : refuse"
	same_gid=$(getent passwd | awk -F: -v g="$build_gid" '$4 == g { n++ } END { print n + 0 }')
	[ "$same_gid" -eq 1 ] ||
		die "le groupe $build_gid du compte $BUILD_ACCOUNT est le groupe principal d'un autre compte : refuse"
}

# Identifiants occupes, un intervalle [debut, fin) par ligne : plages des
# deux fichiers d'un autre proprietaire que ciphera-build (une ligne a son
# uid en chiffres compte comme autre : install.sh et l'agent cherchent la
# plage par nom), puis uids (uid) ou gids (gid) des comptes et groupes
# enumerables par getent. Les comptes d'annuaire non enumerables (sssd sans
# `enumerate`, par exemple) ne peuvent pas etre listes : sur ces hotes,
# l'administrateur doit reserver la plage de ciphera-build hors de celle
# de l'annuaire (ligne `ciphera-build:debut:65536` dans /etc/subuid et
# /etc/subgid avant install.sh, alors verifiee puis conservee).
taken_ids() { # <uid|gid>
	awk -F: -v me="$BUILD_ACCOUNT" '$1 != me && NF >= 3 && $2 ~ /^[0-9]+$/ && $3 ~ /^[0-9]+$/ {
		printf "%.0f %.0f\n", $2, $2 + $3 }' /etc/subuid /etc/subgid
	case "$1" in
	uid) getent passwd ;;
	gid) getent group ;;
	esac | awk -F: '$3 ~ /^[0-9]+$/ { printf "%.0f %.0f\n", $3, $3 + 1 }'
}

# Premier debut >= SUBID_MIN dont les SUBID_COUNT ids ne touchent aucun
# intervalle lu sur l'entree.
first_free_range() {
	sort -n -k1,1 | awk -v c="$SUBID_MIN" -v n="$SUBID_COUNT" '
		$1 < c + n && $2 > c { c = $2 }
		END { printf "%.0f\n", c }'
}

# Code 0 si [debut, debut+nombre) touche un intervalle lu sur l'entree.
range_conflicts() { # <debut> <nombre>
	awk -v s="$1" -v e="$(($1 + $2))" '$1 < e && $2 > s { found = 1 } END { exit found ? 0 : 1 }'
}

# Plage deja attribuee a ciphera-build (un des fichiers absent : rien a
# verifier ici, ensure_subids les cree puis revient) : lisible,
# >= SUBID_COUNT ids, sans recouvrir une autre plage ni un id.
check_subids() {
	[ -e /etc/subuid ] && [ -e /etc/subgid ] || return 0
	for kind in uid gid; do
		line=$(grep "^$BUILD_ACCOUNT:" "/etc/sub$kind" | head -n 1 || true)
		[ -n "$line" ] || continue
		start=$(printf '%s' "$line" | cut -d: -f2)
		count=$(printf '%s' "$line" | cut -d: -f3)
		case "$start$count" in
		'' | *[!0-9]*) die "plage de $BUILD_ACCOUNT illisible dans /etc/sub$kind : $line" ;;
		esac
		[ "$count" -ge "$SUBID_COUNT" ] ||
			die "plage de $BUILD_ACCOUNT dans /etc/sub$kind trop petite ($count ids, $SUBID_COUNT exiges) : la corriger ou la retirer puis relancer install.sh"
		if taken_ids "$kind" | range_conflicts "$start" "$count"; then
			die "la plage $start:$count de $BUILD_ACCOUNT dans /etc/sub$kind recouvre une autre plage ou un ${kind} existant : la retirer (install.sh en choisira une libre) ou la deplacer, puis relancer install.sh"
		fi
	done
}

# Compare deux versions vX.Y.Z deja validees : affiche -1, 0 ou 1.
version_cmp() {
	_a=${1#v}
	_b=${2#v}
	for _ in 1 2 3; do
		_x=${_a%%.*}
		_y=${_b%%.*}
		_a=${_a#*.}
		_b=${_b#*.}
		if [ "$_x" -gt "$_y" ]; then
			echo 1
			return 0
		fi
		if [ "$_x" -lt "$_y" ]; then
			echo -1
			return 0
		fi
	done
	echo 0
}

# Installation existante (VERSION ecrite par install.sh ou l'updater) : seule
# la meme version est acceptee (reparation : fichiers reinstalles, services
# relances). Une version superieure passe par ciphera-cli updater (sauvegarde
# de la base, journal, retour arriere automatique) ; une version inferieure
# est refusee (base deja migree). Jamais pendant une mise a jour (journal ou
# verrou de l'updater) : binaires, unites et services ne sont jamais melanges.
check_existing_install() {
	repair=no
	installed=
	[ -e "$SHARE_DIR/VERSION" ] || return 0
	installed=$(head -n 1 "$SHARE_DIR/VERSION")
	case "$installed" in
	v[0-9]*.[0-9]*.[0-9]*) ;;
	*) die "version installee illisible ($SHARE_DIR/VERSION)" ;;
	esac
	case "$installed" in
	*[!v0-9.]* | *.*.*.* | v*v*) die "version installee illisible ($SHARE_DIR/VERSION)" ;;
	esac
	if [ -e "$UPDATER_STATE_DIR/journal.json" ]; then
		die "mise a jour interrompue en attente de reprise : lancer sudo ciphera-cli updater --recover (ou redemarrer la machine), puis relancer l'installation"
	fi
	case $(version_cmp "$version" "$installed") in
	-1) die "Ciphera $installed est installe : retour a $version refuse (base deja migree). Retour arriere : sudo ciphera-cli updater --version $version --allow-downgrade --restore-db <sauvegarde de $version>" ;;
	1) die "Ciphera $installed est installe : pour passer a $version, lancer sudo ciphera-cli updater --version $version (sauvegarde de la base, retour arriere automatique)" ;;
	esac
	repair=yes
	say "Ciphera $installed deja installe : reparation (fichiers reinstalles, services relances)"
}

# Reparation : verrou de l'updater tenu jusqu'a la fin (aucune mise a jour
# lancee par le panneau pendant le remplacement des fichiers). Puis version
# et journal relus : une mise a jour a pu passer pendant les telechargements.
hold_update_lock() {
	if [ "$repair" = yes ] && [ -d "${UPDATE_LOCK%/*}" ]; then
		if command -v flock >/dev/null 2>&1; then
			(umask 077 && : >>"$UPDATE_LOCK")
			exec 9>>"$UPDATE_LOCK"
			flock -n 9 || die "mise a jour en cours (verrou $UPDATE_LOCK) : relancer l'installation une fois terminee"
		else
			warn "flock absent : verrou de l'updater non pris"
		fi
	fi
	recheck_existing_install
}

# Meme etat qu'a check_existing_install, sinon rien n'est installe.
recheck_existing_install() {
	if [ -e "$UPDATER_STATE_DIR/journal.json" ]; then
		die "mise a jour en cours ou interrompue pendant les telechargements : rien n'est installe, relancer l'installation une fois terminee"
	fi
	now=
	if [ -e "$SHARE_DIR/VERSION" ]; then
		now=$(head -n 1 "$SHARE_DIR/VERSION")
	fi
	[ "$now" = "$installed" ] ||
		die "version installee changee pendant les telechargements (${installed:-aucune} -> ${now:-aucune}) : rien n'est installe, relancer l'installation"
}

# Verifications locales, avant tout telechargement : un compte ciphera-build
# deja present (uid, groupe, plages) est refuse ici plutot qu'apres le
# telechargement des archives. Un compte absent est cree puis verifie de
# meme pendant l'installation.
preflight_checks() {
	check_postgres_account
	if id -u "$BUILD_ACCOUNT" >/dev/null 2>&1; then
		check_build_account
		check_subids
	fi
	for u in $USERS; do
		check_account_shape "$u"
	done
}

# Plage de sous-uids et de sous-gids reservee a ciphera-build (espace de
# noms de rootlesskit : les processus des builds tournent sur l'hote sous
# ces ids). Elle ne recouvre ni une autre plage ni un uid/gid existant.
# Une plage deja attribuee est verifiee de meme (et >= SUBID_COUNT ids)
# puis conservee : relance sans effet.
ensure_subids() {
	for f in /etc/subuid /etc/subgid; do
		if [ ! -e "$f" ]; then
			install -m 0644 -o root -g root /dev/null "$f"
		fi
	done
	check_subids
	if ! grep -q "^$BUILD_ACCOUNT:" /etc/subuid || ! grep -q "^$BUILD_ACCOUNT:" /etc/subgid; then
		first=$({
			taken_ids uid
			taken_ids gid
		} | first_free_range)
		last=$((first + SUBID_COUNT - 1))
		[ "$last" -lt 4294967295 ] || die "aucune plage de sous-identifiants libre pour $BUILD_ACCOUNT"
		if ! grep -q "^$BUILD_ACCOUNT:" /etc/subuid; then
			usermod --add-subuids "$first-$last" "$BUILD_ACCOUNT" ||
				die "attribution des sous-uids de $BUILD_ACCOUNT impossible"
		fi
		if ! grep -q "^$BUILD_ACCOUNT:" /etc/subgid; then
			usermod --add-subgids "$first-$last" "$BUILD_ACCOUNT" ||
				die "attribution des sous-gids de $BUILD_ACCOUNT impossible"
		fi
	fi
	say "compte $BUILD_ACCOUNT (uid $build_uid), sous-uids $(grep "^$BUILD_ACCOUNT:" /etc/subuid | head -n 1 | cut -d: -f2-)"
}

# Les projets Compose sont normalises par le greffon officiel `docker
# compose` : signale s'il manque (les autres builds fonctionnent).
check_compose_plugin() {
	[ "$have_docker" = yes ] || return 0
	if ! docker compose version >/dev/null 2>&1; then
		warn "greffon docker compose absent : les projets Compose ne pourront pas etre construits."
		warn "installer le greffon officiel (paquet docker-compose-plugin, docker-compose-v2 ou docker-compose selon la distribution)."
	fi
}

# nft (paquet nftables) : ciphera-agent.service charge avant chaque demarrage
# le filtrage de la sortie des conteneurs (ponts ciphera-build et
# ciphera-prepare des builds, ponts cphp* des projets ; table
# inet ciphera_egress) ; sans nft, l'agent ne demarre pas. Installe
# meme sans Docker : l'unite est activee dans tous les cas et demarre des
# que Docker apparait (ConditionPathExists).
ensure_nftables() {
	[ -x /usr/sbin/nft ] && return 0
	say "nft absent : installation du paquet nftables de la distribution"
	pkg_install nftables
	[ -x /usr/sbin/nft ] || die "/usr/sbin/nft introuvable apres installation de nftables"
}

# Ancien volume du cache de BuildKit (uid 1000 de l'image), remplace par
# /var/lib/ciphera-buildkit : supprime s'il existe et n'est plus utilise.
remove_legacy_buildkit_volume() {
	[ "$have_docker" = yes ] && docker info >/dev/null 2>&1 || return 0
	docker volume inspect "$LEGACY_BUILDKIT_VOLUME" >/dev/null 2>&1 || return 0
	if docker volume rm "$LEGACY_BUILDKIT_VOLUME" >/dev/null 2>&1; then
		say "ancien volume $LEGACY_BUILDKIT_VOLUME supprime"
	else
		warn "ancien volume $LEGACY_BUILDKIT_VOLUME encore utilise : conserve (docker volume rm $LEGACY_BUILDKIT_VOLUME une fois libre)"
	fi
}

# buildkitd tourne avec l'init de Docker (`--init`, binaire docker-init,
# tini) : sans lui, chaque build echoue au demarrage de buildkitd. Present
# si le demon en lit la version (InitCommit) ou si le binaire configure
# (InitBinary) se trouve dans les dossiers ou dockerd le cherche.
# Avertissement seulement : le PATH de dockerd peut differer du notre.
check_docker_init() {
	[ "$have_docker" = yes ] && docker info >/dev/null 2>&1 || return 0
	init_id=$(docker info --format '{{.InitCommit.ID}}' 2>/dev/null || true)
	case $init_id in
	'' | N/A | '<no value>') ;;
	*) return 0 ;;
	esac
	init_bin=$(docker info --format '{{.InitBinary}}' 2>/dev/null || true)
	[ -n "$init_bin" ] && [ "$init_bin" != '<no value>' ] || init_bin=docker-init
	case $init_bin in
	/*) [ -x "$init_bin" ] && return 0 ;;
	*)
		for d in /usr/local/libexec/docker /usr/libexec/docker /usr/local/lib/docker /usr/lib/docker; do
			[ -x "$d/$init_bin" ] && return 0
		done
		command -v "$init_bin" >/dev/null 2>&1 && return 0
		;;
	esac
	warn "init de Docker ($init_bin) introuvable : les builds echoueront tant qu'il manque (installer tini, paquet qui fournit docker-init, ou regler init-path de dockerd)"
}

# Adresse source de la route par defaut : aucun paquet n'est envoye.
detect_ip() {
	ip -4 route get 1.1.1.1 2>/dev/null |
		awk '{ for (i = 1; i < NF; i++) if ($i == "src") { print $(i + 1); exit } }'
}

init_instance() {
	init_report=
	if [ -e "$CONFIG_DIR/db_password" ]; then
		say "instance deja initialisee ($CONFIG_DIR) : secrets conserves"
		return 0
	fi
	ip_addr=$public_ip
	if [ -z "$ip_addr" ]; then
		ip_addr=$(detect_ip || true)
	fi
	[ -n "$ip_addr" ] || die "adresse IP non detectee : relancer avec --public-ip"
	check_ip "$ip_addr"
	# Sortie standard (adresse et jeton) gardee en memoire, jamais dans le
	# journal ; sortie d'erreur (avertissements, cause d'un echec) relayee.
	if ! init_report=$("$BIN_DIR/ciphera" init --config-dir "$CONFIG_DIR" \
		--public-ip "$ip_addr" --panel-port "$PANEL_PORT" --owner ciphera \
		--agent-owner ciphera-agent --postgres-owner ciphera-postgres 2>"$tmp/init.err"); then
		ui_relay_stderr "$tmp/init.err"
		die "ciphera init a echoue"
	fi
	ui_relay_stderr "$tmp/init.err"
}

# Surcharges du service (/etc/ciphera/ciphera.env), lues par systemd pour
# ciphera.service et par les commandes root `ciphera admin` (qui refusent un
# fichier qui n'est pas a root ou qui est modifiable par le groupe ou les
# autres) : root:ciphera 0640. Le fichier est refait dans un dossier root,
# puis mis en place par renommage : aucun chown ni chmod sur un chemin du
# dossier du service. Contenu repris seulement d'un fichier ordinaire de root
# a un seul lien, non modifiable par le groupe ni les autres (meme regle que
# la CLI), lu sans suivre de lien ; sinon fichier ecarte (root 0600), avec un
# avertissement repete en fin d'installation.
env_notice=
protect_env_file() {
	env_file=$CONFIG_DIR/ciphera.env
	stage=$(mktemp -d /etc/.ciphera-env.XXXXXX) || die "dossier temporaire impossible dans /etc"
	if [ "$(stat -c %d "$stage")" != "$(stat -c %d "$CONFIG_DIR")" ]; then
		rmdir "$stage"
		warn "$CONFIG_DIR sur un autre systeme de fichiers : droits de $env_file non verifies (attendu : root:ciphera 0640)"
		return 0
	fi
	: >"$stage/ciphera.env"
	if [ -e "$env_file" ] || [ -L "$env_file" ]; then
		env_stat=$(stat -c %u:%h:%a "$env_file" 2>/dev/null) || env_stat=
		env_mode=${env_stat##*:}
		if [ ! -L "$env_file" ] && [ -f "$env_file" ] && [ "${env_stat%:*}" = 0:1 ] &&
			[ -n "$env_mode" ] && [ $((0$env_mode & 022)) -eq 0 ]; then
			dd if="$env_file" of="$stage/ciphera.env" iflag=nofollow,nonblock bs=65536 count=1 status=none 2>/dev/null ||
				stage_failed "lecture de $env_file impossible"
		else
			set_aside=$env_file.ecarte-$(date +%Y%m%d%H%M%S)
			mv -fT "$env_file" "$set_aside" || stage_failed "$env_file inutilisable et impossible a ecarter"
			# Contenu non fiable : garde pour relecture par root seulement.
			chown -h root:root "$set_aside" 2>/dev/null || true
			if [ ! -L "$set_aside" ]; then
				chmod 0600 "$set_aside" 2>/dev/null || true
			fi
			env_notice="$env_file ecarte ($set_aside, root 0600) : pas un fichier de root protege en ecriture ; le service demarre sans ces reglages. Relire son contenu, le reprendre dans $env_file (root:ciphera 0640), puis redemarrer ciphera."
			warn "$env_notice"
		fi
	fi
	if ! { chown root:ciphera "$stage/ciphera.env" && chmod 0640 "$stage/ciphera.env" &&
		mv -fT "$stage/ciphera.env" "$env_file"; }; then
		stage_failed "mise en place de $env_file impossible"
	fi
	rmdir "$stage"
}

# Echec de protect_env_file : dossier temporaire retire, puis arret.
stage_failed() {
	rm -rf "$stage"
	die "$1"
}

# Dossier de configuration a root, groupe ciphera, 0750 : le service le
# traverse et y lit ses fichiers, mais ne peut ni renommer ni remplacer
# ca/ (signataire root) ou un autre de ses dossiers. Installation anterieure
# (dossier au compte ciphera) : corrige ici (droits d'abord, proprietaire
# ensuite). Un lien symbolique est refuse.
harden_config_dir() {
	[ -e "$CONFIG_DIR" ] || [ -L "$CONFIG_DIR" ] || return 0
	if [ -L "$CONFIG_DIR" ] || [ ! -d "$CONFIG_DIR" ]; then
		die "$CONFIG_DIR n'est pas un dossier (lien symbolique ?) : a corriger avant de relancer install.sh"
	fi
	if ! { chmod 0750 "$CONFIG_DIR" && chown root:ciphera "$CONFIG_DIR"; }; then
		die "$CONFIG_DIR : droits root:ciphera 0750 non appliques"
	fi
}

# Cle de la CA absente, ou a root sans droit pour le groupe ni les autres.
ca_key_protected() {
	ca_key_file=$CONFIG_DIR/ca/ca.key
	[ -e "$ca_key_file" ] || [ -L "$ca_key_file" ] || return 0
	if [ -L "$ca_key_file" ] || [ ! -f "$ca_key_file" ]; then
		return 1
	fi
	case $(stat -c %u:%a "$ca_key_file") in
	0:600 | 0:400) return 0 ;;
	*) return 1 ;;
	esac
}

# Le socket de pilotage du proxy d'abord (l'agent s'y connecte), puis
# l'agent : il lance PostgreSQL (conteneur), que le plan de controle attend.
# Sans Docker, l'agent est active mais sa condition (/var/run/docker.sock)
# l'empeche de demarrer. Le proxy repond sur le port 80 des son demarrage
# (redirection 301 des hotes IP vers le panneau sur le port 8443).
start_services() {
	systemctl enable --now ciphera-proxy.socket ||
		die "creation du socket de pilotage du proxy impossible (journalctl -u ciphera-proxy.socket)"
	if [ "$have_docker" = yes ]; then
		systemctl enable --now ciphera-agent.service || die "demarrage de ciphera-agent impossible"
	else
		systemctl enable ciphera-agent.service
	fi
	systemctl enable --now ciphera.service ciphera-proxy.service ||
		die "demarrage des services impossible (journalctl -u ciphera -u ciphera-proxy)"
	# Demandes de mise a jour deposees par le panneau (ciphera-cli updater).
	systemctl enable --now ciphera-updater.path ||
		die "activation de ciphera-updater.path impossible (journalctl -u ciphera-updater.path)"
	# Reprise d'une mise a jour interrompue, au demarrage, avant les services.
	systemctl enable ciphera-updater-recover.service ||
		die "activation de ciphera-updater-recover.service impossible"
	# Filtrage de sortie retabli toutes les 5 min s'il disparait (sans
	# Docker, le service est saute par sa condition).
	systemctl enable --now ciphera-egress-check.timer ||
		die "activation de ciphera-egress-check.timer impossible (journalctl -u ciphera-egress-check)"
	# Signataire root des certificats : demandes du plan de controle
	# (ciphera-certsign.path) ; lance aussi maintenant, ce qui remet la cle de
	# la CA d'une installation anterieure a root (0600).
	systemctl enable --now ciphera-certsign.path ||
		die "activation de ciphera-certsign.path impossible (journalctl -u ciphera-certsign.path)"
	systemctl enable ciphera-certsign.service ||
		die "activation de ciphera-certsign.service impossible"
	if ! systemctl start ciphera-certsign.service; then
		# Une demande refusee ne fait pas echouer le signataire : il reste une
		# CA illisible ou un dossier de la CA non protege. Le service demarre
		# quand meme (il ne lit jamais la cle) ; seule une cle encore
		# accessible au compte du service arrete l'installation.
		if ca_key_protected; then
			warn "signataire des certificats en echec : renouvellements suspendus jusqu'a correction (journalctl -u ciphera-certsign, puis systemctl start ciphera-certsign)"
		else
			die "cle de la CA ($CONFIG_DIR/ca/ca.key) pas en root 0600 et signataire en echec : voir journalctl -u ciphera-certsign (lien symbolique, plusieurs liens durs ?), corriger puis relancer install.sh"
		fi
	fi
	# Reparation : `enable --now` ne relance pas un service deja actif ; les
	# trois services repartent ensemble sur les fichiers reinstalles.
	if [ "$repair" = yes ]; then
		if [ "$have_docker" = yes ]; then
			systemctl restart ciphera-proxy.service ciphera-agent.service ciphera.service ||
				die "relance des services impossible (journalctl -u ciphera -u ciphera-agent -u ciphera-proxy)"
		else
			systemctl restart ciphera-proxy.service ciphera.service ||
				die "relance des services impossible (journalctl -u ciphera -u ciphera-proxy)"
		fi
	fi
}

main() {
	parse_args "$@"
	if [ -n "$manifest_target" ]; then
		tools_manifest "$manifest_target"
		exit 0
	fi
	ui_detect
	resolve_sources
	need_root
	ui_start "$version"

	ui_step_begin 1 'Vérification du système' 'Verification du systeme'
	detect_platform
	require_tools
	check_existing_install
	preflight_checks
	ui_step_end

	tmp=$(mktemp -d)
	trap 'ui_on_exit "$?"; rm -rf "$tmp"' EXIT
	trap 'exit 1' HUP INT TERM

	ui_step_begin 2 'Téléchargement et vérification des signatures' 'Telechargement et verification des signatures'
	ensure_minisign
	download_and_verify
	download_build_tools
	ui_step_end

	# Tout est verifie : installation.
	ui_step_begin 3 'Configuration du système' 'Configuration du systeme'
	hold_update_lock
	for u in $USERS; do
		ensure_user "$u"
	done
	check_postgres_account
	check_build_account
	ensure_subids
	setup_docker
	check_compose_plugin
	ensure_nftables
	if [ "$have_docker" = yes ] && getent group docker >/dev/null 2>&1; then
		usermod -aG docker ciphera-agent
	fi
	remove_legacy_buildkit_volume
	check_docker_init
	ui_step_end

	ui_step_begin 4 'Installation de Ciphera' 'Installation de Ciphera'
	install_files
	init_instance
	harden_config_dir
	protect_env_file
	ui_step_end

	ui_step_begin 5 'Démarrage des services' 'Demarrage des services'
	start_services
	ui_step_end

	# Instance deja initialisee (pas de rapport) : adresse du panneau
	# rappelee en mode riche.
	panel_ip=
	if [ "$ui_mode" = rich ] && [ -z "$init_report" ]; then
		panel_ip=${public_ip:-$(detect_ip || true)}
	fi
	# Avertissement de ciphera.env repete en fin d'installation : apres le
	# message final en mode simple (sortie inchangee), avant le resume (et
	# compte) en mode riche.
	if [ -n "$env_notice" ] && [ "$ui_mode" = rich ]; then
		warn "$env_notice"
	fi
	ui_finish "$version" "$init_report" "$panel_ip"
	if [ -n "$env_notice" ] && [ "$ui_mode" != rich ]; then
		warn "$env_notice"
	fi
}

main "$@"
