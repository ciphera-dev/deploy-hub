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
#   install/sign-latest.sh <version> <cle secrete> <cles publiques> <dist> <fichier signe de la release>
#
# <fichier signe de la release> (SHA256SUMS de cette release, avec son
# .minisig) : la cle qui signe latest.json doit etre celle qui a signe ce
# fichier. Sinon refus, rien n'est publie : apres l'etape « Activer » d'une
# rotation, la release N est signee par l'ancienne cle, et les serveurs
# encore avant N ne connaissent qu'elle ; re-signer son latest.json avec la
# nouvelle cle leur cacherait N. Publier N+1 a la place.
#
# Cle chiffree : mot de passe lu dans MINISIGN_PASSWORD (environnement,
# jamais en argument) et passe a minisign sur son entree standard.
set -eu

die() {
	printf 'sign-latest.sh : %s\n' "$*" >&2
	exit 1
}

[ "$#" -eq 5 ] || die "usage : sign-latest.sh <version> <cle secrete> <cles publiques> <dist> <fichier signe de la release>"
version=$1
key=$2
pub=$3
dist=$4
release_file=$5
here=$(cd "$(dirname "$0")" && pwd)

case "$version" in
v[0-9]*.[0-9]*.[0-9]*) ;;
*) die "version invalide : $version" ;;
esac
case "$version" in *[!v0-9.]* | *.*.*.*) die "version invalide : $version" ;; esac

release_signer=$(sh "$here/verify-any.sh" "$pub" "$release_file") ||
	die "$release_file : aucune cle de $pub ne verifie sa signature"

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

signer=$(sh "$here/verify-any.sh" "$pub" "$file") || signer=
if [ "$signer" != "$release_signer" ]; then
	rm -f "$file.minisig"
	die "latest.json : la cle de signature (${signer:-inconnue}) n'est pas celle de la release $version ($release_signer) : rotation activee apres cette release, publier la release suivante au lieu de re-signer (docs/security/release-signing.md)"
fi
sh "$here/check-latest.sh" "$pub" "$dist" "$version" >/dev/null ||
	die "latest.json : verification apres signature en echec"
printf 'latest.json signe : %s\n' "$comment"
