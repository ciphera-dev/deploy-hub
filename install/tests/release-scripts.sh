#!/usr/bin/env bash
# Tests des scripts de publication (sans conteneur ni reseau) :
#   - install/release-script.sh : cles publiques injectees dans install.sh ;
#   - install.sh du depot (sans cle) refuse de s'executer hors mode test ;
#   - install/sign-release.sh : SHA256SUMS, signatures minisign, latest.json,
#     cle chiffree (mot de passe sur l'entree standard) ou non ;
#   - plusieurs cles de confiance (rotation) : n'importe laquelle verifie,
#     une cle inconnue est refusee (install.sh publie, verify-any.sh) ;
#   - install/pages-site.sh : site get.ciphera.ch construit depuis les
#     artefacts signes du run et les releases publiees precedentes (gh
#     remplace par un bouchon), install.sh identique au fichier signe,
#     release alteree refusee.
#
#   bash install/tests/release-scripts.sh      (exige minisign, sha256sum)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fails=0

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

fails_with() { # <motif attendu> <commande...> : echec + message attendu
	local pattern=$1 out
	shift
	if out=$("$@" 2>&1); then
		printf '%s\n' "$out" >&2
		return 1
	fi
	grep -q -- "$pattern" <<<"$out" || {
		printf '%s\n' "$out" >&2
		return 1
	}
}

command -v minisign >/dev/null || { echo "minisign requis" >&2; exit 1; }
cd "$work"
version=$(sed -n "s/^DEFAULT_VERSION='\(.*\)'\$/\1/p" "$root/install/install.sh")

# --- cles ---------------------------------------------------------------------
minisign -G -W -p plain.pub -s plain.key >/dev/null
printf 'mot de passe solide\nmot de passe solide\n' | minisign -G -p enc.pub -s enc.key >/dev/null 2>&1
minisign -G -W -p other.pub -s other.key >/dev/null
minisign -G -W -p next.pub -s next.key >/dev/null
plain_key=$(sed -n 2p plain.pub)
next_key=$(sed -n 2p next.pub)
# Ensemble de confiance d'une rotation : cle courante + cle suivante.
cat plain.pub next.pub >set.pub

# --- release-script.sh --------------------------------------------------------
sh "$root/install/release-script.sh" plain.pub "$root/install/install.sh" released.sh
check "cle injectee une fois" test "$(grep -c "^CIPHERA_PUBKEYS='$plain_key'\$" released.sh)" -eq 1
check "plus aucun marqueur" bash -c "! grep -q '@CIPHERA_RELEASE_PUBKEYS@' released.sh"
check "seule la ligne des cles change" \
	test "$(diff "$root/install/install.sh" released.sh | grep -c '^[<>]')" -eq 2
printf 'untrusted comment: x\nRWpas-une-cle\n' >bad.pub
check "cle publique invalide refusee" \
	fails_with "cle publique invalide" sh "$root/install/release-script.sh" bad.pub "$root/install/install.sh" x.sh
check "script deja injecte refuse" \
	fails_with "marqueur" sh "$root/install/release-script.sh" plain.pub released.sh x.sh
sh "$root/install/release-script.sh" set.pub "$root/install/install.sh" released-set.sh
check "deux cles injectees" test "$(grep -c "^CIPHERA_PUBKEYS='$plain_key $next_key'\$" released-set.sh)" -eq 1
cat plain.pub next.pub other.pub enc.pub >four.pub
check "plus de 3 cles refusees" \
	fails_with "1 a 3 cles" sh "$root/install/release-script.sh" four.pub "$root/install/install.sh" x.sh
printf 'untrusted comment: x\n%s %s\n' "$plain_key" "$next_key" >twoonline.pub
check "deux cles sur une ligne refusees" \
	fails_with "cle publique invalide" sh "$root/install/release-script.sh" twoonline.pub "$root/install/install.sh" x.sh

# --- install.sh du depot / publie ---------------------------------------------
check "install.sh du depot refuse hors mode test" \
	fails_with "cle publique des releases absente" sh "$root/install/install.sh" --non-interactive
check "install.sh du depot : mode test sans cle refuse" \
	fails_with "cle publique des releases absente" env CIPHERA_INSTALL_TEST=1 sh "$root/install/install.sh"
if [ "$(id -u)" -ne 0 ]; then
	check "install.sh publie : cle acceptee (s'arrete a root)" \
		fails_with "droits root requis" sh released.sh --non-interactive
	check "install.sh publie (deux cles) : accepte (s'arrete a root)" \
		fails_with "droits root requis" sh released-set.sh --non-interactive
fi

# --- sign-release.sh ----------------------------------------------------------
mkdir dist
printf 'archive amd64\n' >"dist/ciphera-$version-x86_64-unknown-linux-musl.tar.gz"
printf 'archive arm64\n' >"dist/ciphera-$version-aarch64-unknown-linux-musl.tar.gz"
for d in dist-enc dist-wrong dist-next dist-unknown dist-other; do cp -r dist "$d"; done

sh "$root/install/sign-release.sh" "$version" plain.key plain.pub dist "release de test" >/dev/null
for f in SHA256SUMS SHA256SUMS.minisig install.sh install.sh.minisig install.sh.sha256 latest.json; do
	check "sign-release : $f" test -s "dist/$f"
done
check "sign-release : SHA256SUMS = les deux archives" test "$(wc -l <dist/SHA256SUMS)" -eq 2
check "sign-release : install.sh publie = cle injectee" cmp -s dist/install.sh released.sh
check "sign-release : signature verifiee" minisign -Vq -p plain.pub -m dist/SHA256SUMS
check "sign-release : commentaire de confiance" \
	grep -q "trusted comment: ciphera $version SHA256SUMS (release de test)" dist/SHA256SUMS.minisig
check "sign-release : latest.json" grep -qx "{\"version\":\"$version\"}" dist/latest.json

MINISIGN_PASSWORD='mot de passe solide' \
	sh "$root/install/sign-release.sh" "$version" enc.key enc.pub dist-enc release >/dev/null
check "sign-release : cle chiffree" minisign -Vq -p enc.pub -m dist-enc/install.sh
check "sign-release : mauvais mot de passe refuse" \
	bash -c "! MINISIGN_PASSWORD=faux sh '$root/install/sign-release.sh' '$version' enc.key enc.pub dist-wrong release >/dev/null 2>&1"
check "sign-release : cle chiffree sans mot de passe refusee" \
	bash -c "! sh '$root/install/sign-release.sh' '$version' enc.key enc.pub dist-wrong release >/dev/null 2>&1"
check "sign-release : cle publique d'une autre paire refusee" \
	bash -c "! sh '$root/install/sign-release.sh' '$version' plain.key other.pub dist-wrong release >/dev/null 2>&1"
check "sign-release : version invalide refusee" \
	fails_with "version invalide" sh "$root/install/sign-release.sh" "v1;x" plain.key plain.pub dist-wrong release

# --- plusieurs cles de confiance (rotation) -------------------------------------
sh "$root/install/sign-release.sh" "$version" next.key set.pub dist-next release >/dev/null
sh "$root/install/sign-release.sh" "$version" other.key other.pub dist-other release >/dev/null
check "rotation : signature de la cle suivante acceptee" \
	test "$(sh "$root/install/verify-any.sh" set.pub dist-next/SHA256SUMS)" = "$next_key"
check "rotation : signature de la cle courante acceptee" \
	test "$(sh "$root/install/verify-any.sh" set.pub dist/SHA256SUMS)" = "$plain_key"
check "rotation : cle inconnue refusee (verify-any)" \
	bash -c "! sh '$root/install/verify-any.sh' set.pub dist-other/SHA256SUMS"
check "rotation : cle inconnue refusee (sign-release)" \
	bash -c "! sh '$root/install/sign-release.sh' '$version' other.key set.pub dist-unknown release >/dev/null 2>&1"
# Verification de l'install.sh publie : sa fonction signed_by_trusted_key.
{
	sed -n "s/^CIPHERA_PUBKEYS=/pubkeys=/p" released-set.sh
	sed -n '/^signed_by_trusted_key() {/,/^}/p' released-set.sh
	# shellcheck disable=SC2016 # $1 du script genere, pas de ce test
	printf '%s\n' 'signed_by_trusted_key "$1"'
} >verify-installsh.sh
check "install.sh publie : cle courante acceptee" sh verify-installsh.sh dist/SHA256SUMS
check "install.sh publie : cle suivante acceptee" sh verify-installsh.sh dist-next/SHA256SUMS
check "install.sh publie : cle inconnue refusee" bash -c "! sh verify-installsh.sh dist-other/SHA256SUMS"

check "verify-any : ligne de cle mal formee refusee" \
	bash -c "printf 'untrusted comment: x\nRWpas-une-cle\n' | cat - plain.pub >badset.pub; ! sh '$root/install/verify-any.sh' badset.pub dist/SHA256SUMS"

# --- pages-site.sh (gh remplace par un bouchon) --------------------------------
# Run de release : $version (dist, bonne cle). Releases deja publiees :
# $version (publiee par le run, ignoree), v9.9.7 (bonne cle, gardee),
# v9.9.6 (cle retiree : gardee seulement avec le fichier des cles retirees)
# et v9.9.8 (cle inconnue : ignoree). Le bouchon sert releases/<tag>/.
minisign -G -W -p retired.pub -s retired.key >/dev/null
old=v9.9.8
kept=v9.9.7
ret=v9.9.6
mkdir -p "releases/$version" bin
cp dist/* "releases/$version/"
for t in "$old:other" "$kept:plain" "$ret:retired"; do
	tag=${t%%:*}
	k=${t#*:}
	mkdir -p "dist-$tag" "releases/$tag"
	cp dist/ciphera-*.tar.gz "dist-$tag/"
	sh "$root/install/sign-release.sh" "$tag" "$k.key" "$k.pub" "dist-$tag" release >/dev/null
	cp "dist-$tag"/* "releases/$tag/"
done
cat >bin/gh <<'GH'
#!/bin/sh
# Bouchon de gh : release list / release view / release download.
set -eu
case "$1 $2" in
"release list") printf '%s\n' $GH_STUB_TAGS ;;
"release view") printf '%s\n' "$GH_STUB_LATEST" ;;
"release download")
	tag=$3
	shift 3
	while [ "$#" -gt 0 ]; do
		case "$1" in --dir) dir=$2; shift 2 ;; *) shift ;; esac
	done
	mkdir -p "$dir"
	cp "$GH_STUB_RELEASES/$tag/"* "$dir/"
	;;
*) echo "gh (bouchon) : $*" >&2; exit 2 ;;
esac
GH
chmod +x bin/gh
export GH_STUB_RELEASES="$work/releases" GH_STUB_LATEST="$version"
export GH_STUB_TAGS="$version $old $kept $ret"

pages() { # <cles retirees> <dist> <version> <sortie>
	env PATH="$work/bin:$PATH" sh "$root/install/pages-site.sh" ciphera-dev/deploy-hub plain.pub "$@"
}
pages retired.pub dist "$version" site >/dev/null 2>pages.err
check "site : install.sh = fichier signe du run" cmp -s site/install.sh dist/install.sh
check "site : install.sh.minisig" cmp -s site/install.sh.minisig dist/install.sh.minisig
check "site : install.sh.sha256" cmp -s site/install.sh.sha256 dist/install.sh.sha256
check "site : signature verifiee" minisign -Vq -p plain.pub -m site/install.sh -x site/install.sh.minisig
check "site : releases/latest.json" cmp -s site/releases/latest.json dist/latest.json
check "site : archives de la release" \
	cmp -s "site/releases/$version/ciphera-$version-x86_64-unknown-linux-musl.tar.gz" \
	"dist/ciphera-$version-x86_64-unknown-linux-musl.tar.gz"
check "site : release precedente gardee" test -f "site/releases/$kept/SHA256SUMS"
check "site : release signee par une cle retiree gardee" test -f "site/releases/$ret/SHA256SUMS"
check "site : release signee par une cle inconnue ignoree" test ! -e "site/releases/$old"
check "site : avertissement pour la release ignoree" grep -q "$old" pages.err
check "site : CNAME" grep -qx get.ciphera.ch site/CNAME
check "site : cles publiques = ensemble courant seulement" cmp -s site/ciphera.pub plain.pub
check "site : index et 404 vers GitHub Releases" \
	grep -q 'https://github.com/ciphera-dev/deploy-hub/releases' site/index.html site/404.html
check "site : aucune ressource tierce" bash -c "! grep -Eq '<(script|link|img)' site/index.html site/404.html"

pages absent.pub dist "$version" site-noretired >/dev/null 2>&1
check "site : sans cles retirees, release de cle retiree ignoree" test ! -e "site-noretired/releases/$ret"

# Release du run signee par une cle retiree seulement : refusee.
GH_STUB_LATEST="$ret" check "site : release du run par cle retiree refusee" \
	fails_with "$ret" pages retired.pub "dist-$ret" "$ret" site5
# Release du run alteree : le site n'est pas construit.
cp -r dist dist-tampered
printf 'x' >>"dist-tampered/ciphera-$version-x86_64-unknown-linux-musl.tar.gz"
check "site : release du run alteree refusee" fails_with "$version" pages retired.pub dist-tampered "$version" site2
check "site : version invalide refusee" fails_with "tag invalide" pages retired.pub dist 'v1;rm' site3
GH_STUB_TAGS='v1;rm' check "site : tag publie invalide refuse" fails_with "tag invalide" pages retired.pub dist "$version" site4
GH_STUB_LATEST=v9.9.10 check "site : run plus ancien que la derniere release refuse" \
	fails_with "derniere release publiee est v9.9.10" pages retired.pub dist "$version" site6

if [ "$fails" -ne 0 ]; then
	printf '%d echec(s)\n' "$fails"
	exit 1
fi
printf 'tous les tests passent\n'
