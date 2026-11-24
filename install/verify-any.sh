#!/bin/sh
# Code 0 si <fichier>.minisig est une signature valide de <fichier> par l'une
# des cles de <cles publiques> (format minisign, une ou plusieurs cles ;
# meme regle que install.sh et ciphera-cli updater). Affiche la cle retenue.
#
#   install/verify-any.sh <cles publiques> <fichier>
set -eu

[ "$#" -eq 2 ] || {
	echo "usage : verify-any.sh <cles publiques> <fichier>" >&2
	exit 2
}
keys=$(sed -e 's/\r$//' -e '/^untrusted comment:/d' -e '/^[[:space:]]*$/d' "$1")
# Ligne mal formee : refus (comme ciphera-cli updater), jamais ignoree.
for key in $keys; do
	case "$key" in
	RW*) ;;
	*) exit 1 ;;
	esac
	case "$key" in
	*[!A-Za-z0-9+/=]*) exit 1 ;;
	esac
	[ "${#key}" -eq 56 ] || exit 1
done
for key in $keys; do
	if minisign -V -q -P "$key" -m "$2" -x "$2.minisig" >/dev/null 2>&1; then
		printf '%s\n' "$key"
		exit 0
	fi
done
exit 1
