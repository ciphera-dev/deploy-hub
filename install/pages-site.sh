#!/bin/sh
# Construit le site get.ciphera.ch (GitHub Pages) dans le run de release
# approuve (job `pages-site` de release.yml), a partir des artefacts signes
# de CE run (<dist>, version <version>) :
#   /install.sh (+ .minisig, .sha256)  copies exactes des fichiers signes
#   /ciphera.pub                       cles publiques minisign de confiance
#   /releases/latest.json              derniere version
#   /releases/<version>/...            assets de cette release, plus ceux des
#                                      4 releases publiees precedentes
#                                      (chemins lus par install.sh et
#                                      ciphera-cli updater)
#   /index.html, /404.html             liens vers GitHub Releases
#   /CNAME                             get.ciphera.ch
# Chaque release est verifiee (minisign de SHA256SUMS et de install.sh, puis
# SHA-256). Celle du run : par une cle de <cles publiques> (ensemble de
# confiance courant), sinon rien n'est construit. Les precedentes : par une
# cle de <cles publiques> ou de <cles retirees> (fichier facultatif, lu
# seulement ici : une release signee par une cle retiree reste servie aux
# serveurs anciens, qui la verifient avec leurs propres cles) ; une release
# qui echoue est ignoree. Une release verifiee ne garde que sa liste signee
# (fichiers de SHA256SUMS, SHA256SUMS, install.sh, leurs .minisig,
# install.sh.sha256) : tout autre fichier est retire, avec un avertissement
# (les bundles cosign restent sur GitHub Releases). Le run doit porter la
# derniere release publiee : un run plus ancien n'ecrase jamais le site d'une
# release plus recente.
#
#   install/pages-site.sh <depot owner/nom> <cles publiques> <cles retirees> <dist> <version> <sortie>
#
# Exige gh (GH_TOKEN, releases precedentes), minisign et sha256sum.
set -eu

die() {
	printf 'pages-site.sh : %s\n' "$*" >&2
	exit 1
}
warn() {
	printf 'pages-site.sh : attention : %s\n' "$*" >&2
}

[ "$#" -eq 6 ] || die "usage : pages-site.sh <depot> <cles publiques> <cles retirees> <dist> <version> <sortie>"
repo=$1
pub=$2
retired=$3
dist=$4
latest=$5
out=$6
here=$(cd "$(dirname "$0")" && pwd)
domain=get.ciphera.ch
keep_older=4
case "$repo" in
*[!A-Za-z0-9._/-]* | */*/* | /* | */) die "depot invalide : $repo" ;;
*/*) ;;
*) die "depot invalide : $repo" ;;
esac

check_tag() {
	case "$1" in
	v[0-9]*.[0-9]*.[0-9]*) ;;
	*) die "tag invalide : $1" ;;
	esac
	case "$1" in *[!v0-9.]* | *.*.*.*) die "tag invalide : $1" ;; esac
}

# verify <cles> <dossier> : signatures minisign et empreintes d'une release.
verify() {
	[ -f "$2/SHA256SUMS" ] && [ -f "$2/install.sh" ] || return 1
	sh "$here/verify-any.sh" "$1" "$2/SHA256SUMS" >/dev/null || return 1
	sh "$here/verify-any.sh" "$1" "$2/install.sh" >/dev/null || return 1
	(cd "$2" && sha256sum -c --strict --quiet SHA256SUMS install.sh.sha256 >/dev/null 2>&1)
}

# is_signed <nom> : vrai si <nom> est exactement une ligne de $signed
# (comparaison de chaines ligne a ligne : un nom qui contient un saut de
# ligne ne correspond jamais).
is_signed() {
	while IFS= read -r s; do
		[ "$s" = "$1" ] && return 0
	done <<EOF
$signed
EOF
	return 1
}

# keep_signed <dossier> <release> : apres verify, ne garde que la liste
# signee (fichiers nommes dans SHA256SUMS, SHA256SUMS, install.sh, leurs
# .minisig et install.sh.sha256). Toute autre entree (fichier annexe, cache,
# dossier, lien) est supprimee, avec un avertissement.
# Lignes de SHA256SUMS : <64 hex><espace><espace ou *><nom>, le nom garde
# ses espaces. Une ligne echappee (commencant par \, nom avec \ ou saut de
# ligne) ou mal formee ne garde aucun fichier.
keep_signed() {
	signed=$(sed -n 's/^[0-9a-f]\{64\} [ *]//p' "$1/SHA256SUMS")
	rejected=$(grep -c -v '^[0-9a-f]\{64\} [ *].' "$1/SHA256SUMS" || true)
	if [ "$rejected" -ne 0 ]; then
		warn "release $2 : $rejected ligne(s) de SHA256SUMS echappee(s) ou mal formee(s), ignoree(s)"
	fi
	removed=
	for p in "$1"/* "$1"/.[!.]* "$1"/..?*; do
		[ -e "$p" ] || [ -L "$p" ] || continue
		name=${p##*/}
		if [ -f "$p" ] && [ ! -L "$p" ]; then
			case "$name" in
			SHA256SUMS | SHA256SUMS.minisig | install.sh | install.sh.minisig | install.sh.sha256) continue ;;
			esac
			if is_signed "$name"; then
				continue
			fi
		fi
		rm -rf -- "$p"
		removed="$removed $name"
	done
	if [ -n "$removed" ]; then
		warn "release $2 : retire(s), hors de la liste signee :$removed"
	fi
}

check_tag "$latest"
published=$(gh release view --repo "$repo" --json tagName --jq .tagName)
[ "$published" = "$latest" ] ||
	die "la derniere release publiee est $published, pas $latest : site non construit (un run plus ancien n'ecrase pas le site)"
[ ! -e "$out" ] || die "$out existe deja"
mkdir -p "$out/releases"
# Cles des releases precedentes : ensemble courant + cles retirees.
older_keys="$out.keys"
cat "$pub" >"$older_keys"
if [ -f "$retired" ]; then
	cat "$retired" >>"$older_keys"
fi
trap 'rm -f "$older_keys"' EXIT

cur="$out/releases/$latest"
mkdir "$cur"
cp "$dist"/* "$cur/"
verify "$pub" "$cur" || die "release $latest : signature ou empreinte invalide, site non construit"
keep_signed "$cur" "$latest"

tags=$(gh release list --repo "$repo" --exclude-drafts --exclude-pre-releases \
	--limit "$((keep_older + 1))" --json tagName --jq '.[].tagName')
n=0
for tag in $tags; do
	check_tag "$tag"
	[ "$tag" != "$latest" ] || continue
	[ "$n" -lt "$keep_older" ] || break
	n=$((n + 1))
	gh release download "$tag" --repo "$repo" --dir "$out/releases/$tag"
	if ! verify "$older_keys" "$out/releases/$tag"; then
		warn "release $tag ignoree (signature ou empreinte invalide pour $pub et $retired)"
		rm -rf "${out:?}/releases/$tag"
	else
		keep_signed "$out/releases/$tag" "$tag"
	fi
done

for f in install.sh install.sh.minisig install.sh.sha256; do
	cp "$dist/$f" "$out/$f"
	cmp -s "$dist/$f" "$out/$f" || die "copie de $f alteree"
done
cp "$dist/latest.json" "$out/releases/latest.json"
cp "$pub" "$out/ciphera.pub"
printf '%s\n' "$domain" >"$out/CNAME"

releases_url="https://github.com/$repo/releases"
page() { # <titre> <corps html>
	cat <<EOF
<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>$1</title>
</head>
<body>
$2
</body>
</html>
EOF
}
page "Ciphera : installation" "<h1>Ciphera</h1>
<p>Installation (Linux, root) :</p>
<pre>curl -fsSL https://$domain/install.sh | sudo sh</pre>
<p>Version publiee : $latest. Script signe : <a href=\"/install.sh.minisig\">install.sh.minisig</a>,
empreinte : <a href=\"/install.sh.sha256\">install.sh.sha256</a>, cles publiques :
<a href=\"/ciphera.pub\">ciphera.pub</a>.</p>
<p>Toutes les releases, signatures cosign et attestations : <a href=\"$releases_url\">$releases_url</a>.</p>" \
	>"$out/index.html"
page "Ciphera : page introuvable" "<h1>Page introuvable</h1>
<p>Les releases de Ciphera sont publiees sur <a href=\"$releases_url\">$releases_url</a>.</p>" \
	>"$out/404.html"
printf 'site construit : %s (release %s)\n' "$out" "$latest"
