#!/bin/sh
# Construit l'install.sh publie : les cles publiques minisign de confiance de
# <cles publiques> (format minisign, une ou plusieurs paires « untrusted
# comment » + cle ; plusieurs pendant une rotation) remplacent le marqueur
# @CIPHERA_RELEASE_PUBKEYS@ de install.sh. Le script du depot ne porte aucune
# cle et refuse de s'executer hors mode test ; seul le script publie (signe)
# en porte. Le bloc du mode test (marqueurs mode-test) est retire du script
# publie.
#
#   install/release-script.sh <cles publiques> <install.sh du depot> <sortie>
set -eu

die() {
	printf 'release-script.sh : %s\n' "$*" >&2
	exit 1
}

[ "$#" -eq 3 ] || die "usage : release-script.sh <cles publiques> <install.sh> <sortie>"
pub=$1
src=$2
out=$3

# Cles : chaque ligne qui n'est ni vide ni un commentaire minisign (meme
# lecture que ciphera-cli updater).
keys=$(sed -e 's/\r$//' -e '/^untrusted comment:/d' -e '/^[[:space:]]*$/d' "$pub")
count=0
for key in $keys; do
	case "$key" in
	RW*) ;;
	*) die "cle publique invalide : $pub" ;;
	esac
	case "$key" in
	*[!A-Za-z0-9+/=]*) die "cle publique invalide : $pub" ;;
	esac
	[ "${#key}" -eq 56 ] || die "cle publique invalide : $pub"
	count=$((count + 1))
done
[ "$count" -ge 1 ] && [ "$count" -le 3 ] || die "cle publique invalide : $pub (1 a 3 cles attendues)"
[ "$(printf '%s\n' "$keys" | wc -l)" -eq "$count" ] || die "cle publique invalide : $pub"
keys=$(printf '%s\n' "$keys" | tr '\n' ' ' | sed 's/ $//')

marker="CIPHERA_PUBKEYS='@CIPHERA_RELEASE_PUBKEYS@'"
[ "$(grep -c -x -F "$marker" "$src")" -eq 1 ] ||
	die "marqueur $marker absent ou repete dans $src"

tmp="$out.tmp.$$"
trap 'rm -f "$tmp" "$tmp.body"' EXIT

# Blocs du mode test (« # >>> mode-test » ... « # <<< mode-test », eventuellement
# indentes) : retires du script publie. Marqueurs equilibres, sans
# imbrication, au moins un bloc.
awk '
	/^[ \t]*# >>> mode-test[ \t]*$/ { if (in_block) bad = 1; in_block = 1; blocks++; next }
	/^[ \t]*# <<< mode-test[ \t]*$/ { if (!in_block) bad = 1; in_block = 0; next }
	!in_block { print }
	END { exit (bad || in_block || blocks < 1) ? 1 : 0 }
' "$src" >"$tmp.body" || die "marqueurs mode-test absents ou mal formes dans $src"
if grep -q 'CIPHERA_INSTALL_TEST' "$tmp.body"; then
	die "CIPHERA_INSTALL_TEST subsiste hors du bloc mode-test dans $src"
fi

# Les cles ne contiennent que [A-Za-z0-9+/=] et des espaces : aucun
# caractere special pour sed avec le separateur |.
sed "s|^CIPHERA_PUBKEYS='@CIPHERA_RELEASE_PUBKEYS@'\$|CIPHERA_PUBKEYS='$keys'|" "$tmp.body" >"$tmp"
rm -f "$tmp.body"
[ "$(grep -c -x -F "CIPHERA_PUBKEYS='$keys'" "$tmp")" -eq 1 ] || die "injection des cles ratee"
if grep -q 'CIPHERA_INSTALL_TEST' "$tmp"; then
	die "le script publie mentionne encore CIPHERA_INSTALL_TEST"
fi
chmod 0644 "$tmp"
mv -f "$tmp" "$out"
