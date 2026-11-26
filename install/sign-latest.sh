#!/bin/sh
# Signe <dist>/latest.json (derniere version : GET /api/system/version,
# ciphera-cli updater --latest) avec minisign, puis verifie le resultat
# (check-latest.sh) avec les cles de confiance de <cles publiques>.
#   latest.json          {"version":"<version>"}
#   latest.json.minisig  commentaire de confiance
#                        `ciphera-latest version=<version> issued=<ts> expires=<ts + 120 jours>`
#                        (secondes Unix, couvert par la signature globale)
# Appele par sign-release.sh (release) et par refresh-latest.yml (nouvelle
# echeance pour la derniere release). Ne signe jamais que latest.json, et
# ne change jamais sa version : un latest.json deja present doit valoir
# exactement {"version":"<version>"}, sinon refus.
#
#   install/sign-latest.sh <version> <cle secrete> <cles publiques> <dist>
#
# Cle chiffree : mot de passe lu dans MINISIGN_PASSWORD (environnement,
# jamais en argument) et passe a minisign sur son entree standard.
set -eu

die() {
	printf 'sign-latest.sh : %s\n' "$*" >&2
	exit 1
}

[ "$#" -eq 4 ] || die "usage : sign-latest.sh <version> <cle secrete> <cles publiques> <dist>"
version=$1
key=$2
pub=$3
dist=$4
here=$(cd "$(dirname "$0")" && pwd)

case "$version" in
v[0-9]*.[0-9]*.[0-9]*) ;;
*) die "version invalide : $version" ;;
esac
case "$version" in *[!v0-9.]* | *.*.*.*) die "version invalide : $version" ;; esac

json="{\"version\":\"$version\"}"
file="$dist/latest.json"
if [ -e "$file" ] || [ -L "$file" ]; then
	[ -f "$file" ] && [ ! -L "$file" ] && [ "$(cat "$file")" = "$json" ] ||
		die "$file ne vaut pas $json : version jamais changee, rien n'est signe"
else
	printf '%s\n' "$json" >"$file"
fi

issued=$(date -u +%s)
expires=$((issued + 120 * 24 * 3600))
comment="ciphera-latest version=$version issued=$issued expires=$expires"
rm -f "$file.minisig"
if [ -n "${MINISIGN_PASSWORD:-}" ]; then
	printf '%s\n' "$MINISIGN_PASSWORD" | minisign -S -s "$key" -m "$file" -t "$comment" >/dev/null 2>&1
else
	minisign -S -s "$key" -m "$file" -t "$comment" </dev/null >/dev/null 2>&1
fi || die "signature de $file impossible (cle ou mot de passe)"

sh "$here/check-latest.sh" "$pub" "$dist" "$version" >/dev/null ||
	die "latest.json : verification apres signature en echec"
printf 'latest.json signe : %s\n' "$comment"
