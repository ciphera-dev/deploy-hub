#!/bin/sh
# Code 0 si <dossier>/latest.json est le fichier de derniere version de
# <version>, signe et en cours de validite :
#   - <dossier>/latest.json.minisig est une signature valide par l'une des
#     cles de <cles publiques> (verify-any.sh : signature du fichier et
#     signature globale, qui couvre le commentaire de confiance) ;
#   - commentaire de confiance exactement
#     `ciphera-latest version=<version> issued=<n> expires=<n>`, echeance
#     apres la signature, au plus 120 jours plus tard, et pas encore passee ;
#   - latest.json vaut exactement {"version":"<version>"}.
# Meme regle que shared::latest (plan de controle, ciphera-cli updater),
# l'echeance en plus : un fichier echu n'est jamais publie.
#
#   install/check-latest.sh <cles publiques> <dossier> <version>
set -eu

die() {
	printf 'check-latest.sh : %s\n' "$*" >&2
	exit 1
}

[ "$#" -eq 3 ] || die "usage : check-latest.sh <cles publiques> <dossier> <version>"
pub=$1
dir=$2
version=$3
here=$(cd "$(dirname "$0")" && pwd)
validity=$((120 * 24 * 3600))

case "$version" in
v[0-9]*.[0-9]*.[0-9]*) ;;
*) die "version invalide : $version" ;;
esac
case "$version" in *[!v0-9.]* | *.*.*.*) die "version invalide : $version" ;; esac

[ -f "$dir/latest.json" ] && [ -f "$dir/latest.json.minisig" ] ||
	die "latest.json ou latest.json.minisig absent de $dir"
sh "$here/verify-any.sh" "$pub" "$dir/latest.json" >/dev/null ||
	die "latest.json : aucune cle de $pub ne verifie la signature"
[ "$(cat "$dir/latest.json")" = "{\"version\":\"$version\"}" ] ||
	die "latest.json : contenu inattendu (attendu {\"version\":\"$version\"})"

# Ligne 3 de la signature : commentaire de confiance (verifie ci-dessus).
comment=$(sed -n '3s/^trusted comment: //p' "$dir/latest.json.minisig" | tr -d '\r')
issued=${comment#"ciphera-latest version=$version issued="}
[ "$issued" != "$comment" ] || die "latest.json : commentaire de confiance mal forme : $comment"
expires=${issued#* expires=}
issued=${issued%% expires=*}
for n in "$issued" "$expires"; do
	case "$n" in
	'' | *[!0-9]* | 0?*) die "latest.json : commentaire de confiance mal forme : $comment" ;;
	esac
	[ "${#n}" -le 18 ] || die "latest.json : commentaire de confiance mal forme : $comment"
done
[ "$comment" = "ciphera-latest version=$version issued=$issued expires=$expires" ] ||
	die "latest.json : commentaire de confiance mal forme : $comment"
[ "$expires" -gt "$issued" ] && [ $((expires - issued)) -le "$validity" ] ||
	die "latest.json : echeance invalide : $comment"
now=$(date -u +%s)
[ "$now" -lt "$expires" ] || die "latest.json : echeance passee ($comment) : re-signer (refresh-latest.yml)"
printf '%s\n' "$comment"
