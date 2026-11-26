#!/bin/sh
# Signe une release dans <dist> (archives ciphera-*.tar.gz deja presentes) :
#   install.sh          script publie, cle de <cle publique> injectee
#   install.sh.minisig  signature minisign de install.sh
#   install.sh.sha256   empreinte de install.sh (publiee a cote sur le site)
#   SHA256SUMS          empreintes des archives
#   SHA256SUMS.minisig  signature minisign de SHA256SUMS
#   latest.json         derniere version (GET /api/system/version,
#                       ciphera-cli updater --latest)
#   latest.json.minisig signature minisign de latest.json (sign-latest.sh :
#                       version et echeance de 120 jours dans le commentaire
#                       de confiance)
# puis verifie tout avec les cles de confiance de <cles publiques> : une cle
# secrete qui ne correspond a aucune cle publiee n'aboutit a rien.
#
#   install/sign-release.sh <version> <cle secrete> <cles publiques> <dist> <libelle>
#
# Cle chiffree : mot de passe lu dans MINISIGN_PASSWORD (environnement,
# jamais en argument) et passe a minisign sur son entree standard.
set -eu

die() {
	printf 'sign-release.sh : %s\n' "$*" >&2
	exit 1
}

[ "$#" -eq 5 ] || die "usage : sign-release.sh <version> <cle secrete> <cle publique> <dist> <libelle>"
version=$1
key=$2
pub=$3
dist=$4
label=$5
here=$(cd "$(dirname "$0")" && pwd)

case "$version" in
v[0-9]*.[0-9]*.[0-9]*) ;;
*) die "version invalide : $version" ;;
esac
case "$version" in *[!v0-9.]*) die "version invalide : $version" ;; esac
case "$label" in *[!A-Za-z0-9\ -]*) die "libelle invalide" ;; esac

msign() { # <fichier> <commentaire de confiance>
	if [ -n "${MINISIGN_PASSWORD:-}" ]; then
		printf '%s\n' "$MINISIGN_PASSWORD" | minisign -S -s "$key" -m "$1" -t "$2" >/dev/null 2>&1
	else
		minisign -S -s "$key" -m "$1" -t "$2" </dev/null >/dev/null 2>&1
	fi || die "signature de $1 impossible (cle ou mot de passe)"
}

sh "$here/release-script.sh" "$pub" "$here/install.sh" "$dist/install.sh"
(
	cd "$dist"
	set -- ciphera-*.tar.gz
	[ -f "$1" ] || die "aucune archive dans $dist"
	# Noms limites a un jeu sur : sha256sum echappe (prefixe \) les noms avec
	# antislash ou saut de ligne, et selon l'implementation (coreutils, uutils)
	# sha256sum -c ne relit pas toujours ces lignes.
	for f in "$@"; do
		case $f in *[!A-Za-z0-9._+-]*) die "nom d'archive non autorise (A-Z a-z 0-9 . _ + - seulement) : $f" ;; esac
	done
	sha256sum "$@" >SHA256SUMS
	sha256sum install.sh >install.sh.sha256
)
msign "$dist/SHA256SUMS" "ciphera $version SHA256SUMS ($label)"
msign "$dist/install.sh" "ciphera $version install.sh ($label)"
sh "$here/sign-latest.sh" "$version" "$key" "$pub" "$dist" "$dist/SHA256SUMS" >/dev/null

# Une cle de confiance doit verifier les deux signatures (rotation : la cle
# de signature est l'une des cles de <cles publiques>).
signer=$(sh "$here/verify-any.sh" "$pub" "$dist/SHA256SUMS") ||
	die "SHA256SUMS : aucune cle de $pub ne verifie la signature"
[ "$(sh "$here/verify-any.sh" "$pub" "$dist/install.sh")" = "$signer" ] ||
	die "install.sh : aucune cle de $pub ne verifie la signature"
(cd "$dist" && sha256sum -c --strict --quiet SHA256SUMS install.sh.sha256) ||
	die "empreintes incorrectes"
printf 'release %s signee (%s, cle %s)\n' "$version" "$label" "$signer"
