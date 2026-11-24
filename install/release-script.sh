#!/bin/sh
# Construit l'install.sh publie : les cles publiques minisign de confiance de
# <cles publiques> (format minisign, une ou plusieurs paires « untrusted
# comment » + cle ; plusieurs pendant une rotation) remplacent le marqueur
# @CIPHERA_RELEASE_PUBKEYS@ de install.sh. Le script du depot ne porte aucune
# cle et refuse de s'executer hors mode test ; seul le script publie (signe)
# en porte.
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
# Les cles ne contiennent que [A-Za-z0-9+/=] et des espaces : aucun
# caractere special pour sed avec le separateur |.
sed "s|^CIPHERA_PUBKEYS='@CIPHERA_RELEASE_PUBKEYS@'\$|CIPHERA_PUBKEYS='$keys'|" "$src" >"$tmp"
[ "$(grep -c -x -F "CIPHERA_PUBKEYS='$keys'" "$tmp")" -eq 1 ] || {
	rm -f "$tmp"
	die "injection des cles ratee"
}
chmod 0644 "$tmp"
mv -f "$tmp" "$out"
