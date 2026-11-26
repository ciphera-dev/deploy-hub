#!/usr/bin/env bash
# Tests de l'affichage de install/install.sh (sans root, sans conteneur ni
# reseau) :
#   - choix du mode : riche seulement sur un terminal (sorties standard et
#     d'erreur), TERM ni vide ni `dumb`, NO_COLOR absent ou vide ; logo et
#     symboles en ASCII hors locale UTF-8 ; bleu Ciphera en 24 bits si
#     COLORTERM vaut truecolor ou 24bit, sinon couleur 256 la plus proche ;
#   - mode simple (hors terminal) : sortie ligne par ligne, messages
#     `ciphera : ...` et adresse de l'assistant sur sa propre ligne, aucune
#     sequence d'echappement, aucun journal ;
#   - mode riche (force, sorties dans des fichiers) : logo, etapes cochees,
#     avertissements visibles, details seulement dans le journal (0600,
#     lien symbolique remplace sans etre suivi), jeton jamais journalise ;
#     echec : croix, message d'erreur, fin du journal ; interruption : aucun
#     processus du spinner restant, curseur reaffiche ;
#   - install.sh lui-meme hors terminal : aucune sequence d'echappement ;
#   - pseudo-terminal (`script`, si present) : mode riche detecte.
#
#   bash install/tests/display.sh      (exige sh, awk, stat ; `script` facultatif)
# Le code entre apostrophes est evalue par sh (section chargee).
# shellcheck disable=SC2016
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
installer="$root/install/install.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fails=0
esc=$(printf '\033')

check() { # <libelle> <commande...>
	local label=$1
	shift
	if "$@"; then
		printf 'ok   %s\n' "$label"
	else
		printf 'FAIL %s\n' "$label"
		fails=$((fails + 1))
	fi
}

no_esc() { # <fichier>
	! grep -q "$esc" "$1"
}

has() { # <fichier> <texte fixe>
	grep -qF -- "$2" "$1"
}

lacks() { # <fichier> <texte fixe>
	! grep -qF -- "$2" "$1"
}

dead() { # <fichier contenant un pid>
	local pid
	pid=$(cat "$1")
	[ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null
}

# Section d'affichage d'install.sh, chargee seule (le script appelle main a
# sa derniere ligne : il n'est jamais source en entier).
sed -n "/^# --- affichage/,/^# --- fin de l'affichage/p" "$installer" >"$work/ui.sh"
check "section d'affichage presente" grep -q '^ui_step_begin()' "$work/ui.sh"

# Code POSIX lance par sh avec la section chargee ; environnement neutre.
ui_sh() { # <code> : variables W (dossier de travail) et UI (section) fournies
	env -i PATH="$PATH" HOME="$work" W="$work" UI="$work/ui.sh" "${extra_env[@]}" sh -c ". \"\$UI\"; $1"
}

# --- choix du mode ------------------------------------------------------------
mode() { # <tty1> <tty2> [VAR=valeur...] : affiche ui_want ui_utf8
	local t1=$1 t2=$2
	shift 2
	extra_env=("$@")
	ui_sh "ui_choose_mode $t1 $t2; printf '%s %s\n' \"\$ui_want\" \"\$ui_utf8\""
}
utf=(TERM=xterm-256color LANG=fr_CH.UTF-8)
check "terminal, TERM et locale UTF-8 : riche" test "$(mode yes yes "${utf[@]}")" = "rich yes"
check "sortie standard hors terminal : simple" test "$(mode no yes "${utf[@]}" | cut -d' ' -f1)" = plain
check "sortie d'erreur hors terminal : simple" test "$(mode yes no "${utf[@]}" | cut -d' ' -f1)" = plain
check "TERM=dumb : simple" test "$(mode yes yes TERM=dumb LANG=fr_CH.UTF-8 | cut -d' ' -f1)" = plain
check "TERM absent : simple" test "$(mode yes yes LANG=fr_CH.UTF-8 | cut -d' ' -f1)" = plain
check "NO_COLOR=1 : simple" test "$(mode yes yes "${utf[@]}" NO_COLOR=1 | cut -d' ' -f1)" = plain
check "NO_COLOR vide : ignore (no-color.org)" test "$(mode yes yes "${utf[@]}" NO_COLOR= | cut -d' ' -f1)" = rich
check "locale C : riche en ASCII" test "$(mode yes yes TERM=xterm LANG=C)" = "rich no"
check "LC_ALL prime sur LANG" test "$(mode yes yes TERM=xterm LANG=fr_CH.UTF-8 LC_ALL=C)" = "rich no"
check "LC_CTYPE en utf8 : UTF-8" test "$(mode yes yes TERM=xterm LC_CTYPE=en_US.utf8)" = "rich yes"
check "aucune locale : ASCII" test "$(mode yes yes TERM=xterm)" = "rich no"

# --- mode simple --------------------------------------------------------------
report='Ciphera est initialise.
Ouvrez cette adresse pour creer le compte administrateur (jeton a usage unique, valable 24 heures ; ensuite : sudo ciphera admin setup-token) :
https://192.0.2.10:8443/setup#token=SECRETjeton_123-abc
Empreinte SHA-256 : AB:CD:EF
Comparez cette empreinte a celle du certificat affiche par votre navigateur avant de continuer.'
printf '%s\n' "$report" >"$work/report.txt"

extra_env=("${utf[@]}" "INSTALL_LOG=$work/plain/ciphera-install.log")
mkdir "$work/plain"
ui_sh 'INSTALL_LOG=$W/plain/ciphera-install.log
ui_choose_mode no no
ui_start v9.9.9
ui_step_begin 1 "Vérification du système" "Verification du systeme"
say "detail de l etape"
warn "avertissement simple"
ui_step_end
ui_finish v9.9.9 "$(cat "$W/report.txt")" 192.0.2.10' >"$work/plain.out" 2>&1 || fails=$((fails + 1))
check "simple : etape annoncee en ASCII" grep -qx 'ciphera : \[1/5\] Verification du systeme' "$work/plain.out"
check "simple : say inchange" grep -qx 'ciphera : detail de l etape' "$work/plain.out"
check "simple : warn inchange" grep -qx 'ciphera : attention : avertissement simple' "$work/plain.out"
check "simple : message final inchange" grep -qx 'ciphera : Ciphera v9.9.9 est installe et demarre.' "$work/plain.out"
check "simple : adresse de l'assistant seule sur sa ligne" \
	grep -qx 'https://192.0.2.10:8443/setup#token=SECRETjeton_123-abc' "$work/plain.out"
check "simple : explications de validite gardees" has "$work/plain.out" 'valable 24 heures'
check "simple : aucune sequence d'echappement" no_esc "$work/plain.out"
check "simple : aucun journal cree" test ! -e "$work/plain/ciphera-install.log"

set +e
ui_sh 'ui_choose_mode no no; ui_start v9.9.9
ui_step_begin 2 "Téléchargement" "Telechargement"
die "echec simple"; echo APRES_DIE' >"$work/plain-die.out" 2>&1
rc=$?
set -e
check "simple : die sort en 1" test "$rc" -eq 1
check "simple : message d'erreur inchange" grep -qx 'ciphera : erreur : echec simple' "$work/plain-die.out"
check "simple : rien apres die" lacks "$work/plain-die.out" APRES_DIE
check "simple : echec sans sequence d'echappement" no_esc "$work/plain-die.out"

# --- mode riche (force, sorties dans des fichiers) ----------------------------
rich_run() { # <nom> <code> [VAR=valeur...] : sortie ecran dans $work/<nom>.out
	local name=$1 code=$2
	shift 2
	extra_env=("$@")
	mkdir -p "$work/$name"
	set +e
	ui_sh "INSTALL_LOG=\$W/$name/ciphera-install.log
ui_choose_mode yes yes
$code" >"$work/$name.out" 2>&1
	rc=$?
	set -e
}

rich_run ok 'ui_start v9.9.9
ui_step_begin 1 "Vérification du système" "Verification du systeme"
printf "%s\n" "$ui_spin_pid" >"$W/ok/spin.pid"
say "detail reserve au journal"
warn "avertissement visible"
sleep 1
ui_step_end
ui_step_begin 2 "Téléchargement et vérification des signatures" "Telechargement et verification des signatures"
ui_step_end
ui_finish v9.9.9 "$(cat "$W/report.txt")" 192.0.2.10' "${utf[@]}"
log=$work/ok/ciphera-install.log
check "riche : succes" test "$rc" -eq 0
check "riche : logo exact (1re ligne)" has "$work/ok.out" '██████╗ ██╗ ██████╗  ██╗  ██╗ ████████╗ ██████╗   █████╗'
check "riche : logo exact (6e ligne)" has "$work/ok.out" ' ╚═════╝ ╚═╝ ╚═╝      ╚═╝  ╚═╝ ╚═══════╝ ╚═╝  ╚═╝ ╚═╝     ╚═╝'
check "riche : bleu Ciphera en 256 couleurs (33)" has "$work/ok.out" "${esc}[38;5;33m"
check "riche : pas de 24 bits sans COLORTERM" lacks "$work/ok.out" "${esc}[38;2;"
check "riche : etape cochee" has "$work/ok.out" '✔'
check "riche : libelle accentue" has "$work/ok.out" '[1/5] Vérification du système'
check "riche : avertissement visible a l'ecran" has "$work/ok.out" 'avertissement visible'
check "riche : details absents de l'ecran" lacks "$work/ok.out" 'detail reserve au journal'
check "riche : details dans le journal" has "$log" 'ciphera : detail reserve au journal'
check "riche : avertissement dans le journal" has "$log" 'ciphera : attention : avertissement visible'
check "riche : journal 0600" test "$(stat -c %a "$log")" = 600
check "riche : Installation terminee" has "$work/ok.out" 'Installation terminée'
check "riche : adresse de l'assistant a l'ecran" has "$work/ok.out" 'https://192.0.2.10:8443/setup#token=SECRETjeton_123-abc'
check "riche : explications de validite a l'ecran" has "$work/ok.out" 'valable 24 heures'
check "riche : empreinte a l'ecran" has "$work/ok.out" 'Empreinte SHA-256 : AB:CD:EF'
check "riche : jeton jamais dans le journal" lacks "$log" 'SECRETjeton'
check "riche : chemin du journal indique" has "$work/ok.out" "$log"
check "riche : spinner arrete" dead "$work/ok/spin.pid"
check "riche : curseur reaffiche" has "$work/ok.out" "${esc}[?25h"

rich_run tc 'ui_start v9.9.9; ui_step_begin 1 "É" "E"; ui_step_end' "${utf[@]}" COLORTERM=truecolor
check "riche : bleu #2f7fe0 en 24 bits (COLORTERM=truecolor)" has "$work/tc.out" "${esc}[38;2;47;127;224m"
rich_run tc24 'ui_start v9.9.9' "${utf[@]}" COLORTERM=24bit
check "riche : 24 bits aussi pour COLORTERM=24bit" has "$work/tc24.out" "${esc}[38;2;47;127;224m"

rich_run ascii 'ui_start v9.9.9
ui_step_begin 1 "Vérification du système" "Verification du systeme"
ui_step_end' TERM=xterm LANG=C
check "riche ASCII : logo remplace par CIPHERA" has "$work/ascii.out" 'CIPHERA'
check "riche ASCII : aucun caractere de boite" lacks "$work/ascii.out" '█'
check "riche ASCII : libelle sans accent" has "$work/ascii.out" '[1/5] Verification du systeme'
check "riche ASCII : aucun octet non ASCII" bash -c "! LC_ALL=C grep -q '[^ -~[:cntrl:]]' '$work/ascii.out'"

mkdir -p "$work/link"
printf 'intact\n' >"$work/victim"
ln -s "$work/victim" "$work/link/ciphera-install.log"
rich_run link 'ui_start v9.9.9; say "dans le journal"' "${utf[@]}"
check "journal : cible d'un lien preexistant intacte" test "$(cat "$work/victim")" = intact
check "journal : lien remplace par un fichier ordinaire" \
	sh -c "test -f '$work/link/ciphera-install.log' && test ! -L '$work/link/ciphera-install.log'"
check "journal : ecrit dans le nouveau fichier" has "$work/link/ciphera-install.log" 'dans le journal'

mkdir -p "$work/nodir"
rich_run nolog 'INSTALL_LOG=$W/absent/dossier/ciphera-install.log
ui_start v9.9.9
ui_step_begin 1 "É" "E"; say "repli"; ui_step_end' "${utf[@]}"
check "journal impossible : repli en mode simple" grep -qx 'ciphera : repli' "$work/nolog.out"
check "journal impossible : signale" has "$work/nolog.out" 'journal'
check "journal impossible : sans sequence d'echappement" no_esc "$work/nolog.out"

rich_run fail 'ui_start v9.9.9
ui_step_begin 1 "Vérification du système" "Verification du systeme"
ui_step_end
ui_step_begin 2 "Téléchargement" "Telechargement"
printf "%s\n" "$ui_spin_pid" >"$W/fail/spin.pid"
say "ligne de contexte 42"
warn "avertissement avant echec"
die "signature invalide : rien n est installe"
echo APRES_DIE' "${utf[@]}"
check "echec : sortie en 1" test "$rc" -eq 1
check "echec : croix rouge" has "$work/fail.out" "${esc}[31m"
check "echec : croix" has "$work/fail.out" '✖'
check "echec : message d'erreur" has "$work/fail.out" 'ciphera : erreur : signature invalide : rien n est installe'
check "echec : avertissement montre" has "$work/fail.out" 'avertissement avant echec'
check "echec : fin du journal montree" has "$work/fail.out" 'ligne de contexte 42'
check "echec : chemin du journal" has "$work/fail.out" "$work/fail/ciphera-install.log"
check "echec : erreur dans le journal" has "$work/fail/ciphera-install.log" 'ciphera : erreur : signature invalide'
check "echec : rien apres die" lacks "$work/fail.out" APRES_DIE
check "echec : spinner arrete" dead "$work/fail/spin.pid"
check "echec : curseur reaffiche" has "$work/fail.out" "${esc}[?25h"

rich_run sete 'set -e
ui_start v9.9.9
ui_step_begin 3 "Configuration du système" "Configuration du systeme"
printf "%s\n" "$ui_spin_pid" >"$W/sete/spin.pid"
false
echo APRES_DIE' "${utf[@]}"
check "commande en echec (set -e) : sortie non nulle" test "$rc" -ne 0
check "commande en echec (set -e) : croix" has "$work/sete.out" '✖'
check "commande en echec (set -e) : interruption signalee" has "$work/sete.out" 'interrompue'
check "commande en echec (set -e) : spinner arrete" dead "$work/sete/spin.pid"

# Interruption (TERM) pendant une etape : trap, spinner arrete, curseur rendu.
mkdir -p "$work/term"
extra_env=("${utf[@]}")
ui_sh 'INSTALL_LOG=$W/term/ciphera-install.log
ui_choose_mode yes yes
ui_start v9.9.9
ui_step_begin 4 "Installation de Ciphera" "Installation de Ciphera"
printf "%s\n" "$ui_spin_pid" >"$W/term/spin.pid"
printf "%s\n" "$$" >"$W/term/sh.pid"
sleep 30 &
wait $!' >"$work/term.out" 2>&1 &
bg=$!
for _ in $(seq 50); do
	[ -s "$work/term/sh.pid" ] && break
	sleep 0.1
done
kill -TERM "$(cat "$work/term/sh.pid")"
set +e
wait "$bg"
rc=$?
set -e
sleep 0.3
check "interruption : sortie non nulle" test "$rc" -ne 0
check "interruption : spinner arrete" dead "$work/term/spin.pid"
check "interruption : croix" has "$work/term.out" '✖'
check "interruption : curseur reaffiche" has "$work/term.out" "${esc}[?25h"

# Question interactive : spinner suspendu (ligne effacee, curseur visible).
rich_run pause 'ui_start v9.9.9
ui_step_begin 3 "Configuration du système" "Configuration du systeme"
ui_pause
printf "%s\n" "${ui_spin_pid:-aucun}" >"$W/pause/spin.pid"
printf "QUESTION [o/N] " >&3
ui_resume
ui_step_end' "${utf[@]}"
check "question : aucun spinner pendant la question" test "$(cat "$work/pause/spin.pid")" = aucun
check "question : etape terminee ensuite" has "$work/pause.out" '[3/5] Configuration du système'

# --- install.sh hors terminal ---------------------------------------------------
env TERM=xterm-256color LANG=C.UTF-8 COLORTERM=truecolor sh "$installer" --help >"$work/help.out" 2>&1
check "install.sh --help : aucune sequence d'echappement" no_esc "$work/help.out"
env TERM=xterm-256color LANG=C.UTF-8 sh "$installer" --tools-manifest x86_64-unknown-linux-musl >"$work/manifest.out" 2>&1
check "install.sh --tools-manifest : trois lignes, sans echappement" \
	sh -c "test \$(wc -l <'$work/manifest.out') -eq 3 && ! grep -q '$esc' '$work/manifest.out'"
set +e
env TERM=xterm-256color LANG=C.UTF-8 sh "$installer" --non-interactive >"$work/nokey.out" 2>&1
rc=$?
set -e
check "install.sh du depot : refuse" test "$rc" -ne 0
check "install.sh du depot : message inchange" grep -q '^ciphera : erreur : cle publique des releases absente' "$work/nokey.out"
check "install.sh du depot : aucune sequence d'echappement" no_esc "$work/nokey.out"
check "install.sh : syntaxe POSIX valide" sh -n "$installer"

# --- pseudo-terminal ------------------------------------------------------------
if command -v script >/dev/null 2>&1 && script -qec true /dev/null >/dev/null 2>&1; then
	pty() { # <VAR=valeur...> : mode detecte sur un pseudo-terminal
		env -i PATH="$PATH" UI="$work/ui.sh" "$@" \
			script -qec 'sh -c ". \"\$UI\"; ui_detect; printf \"MODE=%s\\n\" \"\$ui_want\""' /dev/null </dev/null
	}
	check "pseudo-terminal : mode riche detecte" sh -c "printf '%s' \"\$1\" | grep -q MODE=rich" _ "$(pty TERM=xterm-256color LANG=C.UTF-8)"
	check "pseudo-terminal : NO_COLOR=1 force le mode simple" sh -c "printf '%s' \"\$1\" | grep -q MODE=plain" _ "$(pty TERM=xterm-256color LANG=C.UTF-8 NO_COLOR=1)"
	check "pseudo-terminal : TERM=dumb force le mode simple" sh -c "printf '%s' \"\$1\" | grep -q MODE=plain" _ "$(pty TERM=dumb LANG=C.UTF-8)"
else
	printf 'skip pseudo-terminal (script absent)\n'
fi

if [ "$fails" -ne 0 ]; then
	printf '%d test(s) en echec\n' "$fails"
	exit 1
fi
printf 'tous les tests d affichage passent\n'
