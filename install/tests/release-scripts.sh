#!/usr/bin/env bash
# Tests des scripts de publication (sans conteneur ni reseau) :
#   - install/release-script.sh : cles publiques injectees dans install.sh, blocs
#     mode-test (surcharges de test) retires du script publie ;
#   - install.sh du depot (sans cle) refuse de s'executer hors mode test ;
#   - install/sign-release.sh : SHA256SUMS, signatures minisign, latest.json,
#     cle chiffree (mot de passe sur l'entree standard) ou non ;
#   - install/sign-latest.sh et check-latest.sh : latest.json signe (version
#     et echeance de 120 jours dans le commentaire de confiance), puis
#     verifie ; re-signature sans changer la version (refresh-latest.yml) ;
#     fichier altere, version differente ou echeance passee refuses ;
#   - plusieurs cles de confiance (rotation) : n'importe laquelle verifie,
#     une cle inconnue est refusee (install.sh publie, verify-any.sh) ;
#   - install/pages-site.sh : site get.ciphera.ch construit depuis les
#     artefacts signes du run et les releases publiees precedentes (gh
#     remplace par un bouchon), install.sh identique au fichier signe,
#     release alteree refusee, fichiers hors de la liste signee retires.
#     Page d'accueil et 404 (install/site/) : fichiers publies sous
#     /assets/, version et liens de signature injectes, aucune URL externe
#     hors de github.com/ciphera-dev/deploy-hub et mailto:info@ciphera.ch.
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
# Le script publie = le script du depot sans les blocs mode-test, cle injectee.
awk '
	/^[ \t]*# >>> mode-test[ \t]*$/ { in_block = 1; next }
	/^[ \t]*# <<< mode-test[ \t]*$/ { in_block = 0; next }
	!in_block { print }
' "$root/install/install.sh" >expected-body.sh
check "le depot porte un bloc mode-test" grep -q '# >>> mode-test' "$root/install/install.sh"
check "script publie : plus aucune mention du mode test" \
	bash -c "! grep -q -e CIPHERA_INSTALL_TEST -e 'mode-test' -e 'MODE TEST' released.sh"
check "script publie : ignore les surcharges de test" \
	bash -c "! grep -q -e CIPHERA_RELEASE_BASE_URL -e CIPHERA_TOOLS_BASE_URL -e CIPHERA_TEST_MINISIGN_PUBKEY released.sh"
check "script publie : identique au depot hors bloc et cle" \
	test "$(diff expected-body.sh released.sh | grep -c '^[<>]')" -eq 2
check "script publie : syntaxe valide" sh -n released.sh
cp "$root/install/install.sh" nomarker.sh
sed -i '/# >>> mode-test/d' nomarker.sh
check "marqueurs de mode test mal formes refuses" \
	fails_with "mode-test" sh "$root/install/release-script.sh" plain.pub nomarker.sh x.sh
cp "$root/install/install.sh" leak.sh
sed -i "s|^DEFAULT_VERSION=|# CIPHERA_INSTALL_TEST fuite\nDEFAULT_VERSION=|" leak.sh
check "mention du mode test hors bloc refusee" \
	fails_with "CIPHERA_INSTALL_TEST subsiste" sh "$root/install/release-script.sh" plain.pub leak.sh x.sh
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

# --- uninstall.sh : conteneurs d'infrastructure seulement (docker bouchonne) ----
# Fixture « id nom etiquettes » ; le bouchon filtre sur label=K=V et name=^/x$
# et retire de la fixture ce que `docker rm` supprime.
mkdir -p ubin
cat >ubin/docker <<'DK'
#!/bin/sh
set -eu
case "$1" in
ps)
	shift
	filters=
	while [ "$#" -gt 0 ]; do
		case "$1" in --filter) filters="$filters $2"; shift 2 ;; *) shift ;; esac
	done
	# shellcheck disable=SC2086
	awk -v f="$filters" '
		BEGIN { n = split(f, fs, " ") }
		{
			ok = 1
			for (i = 1; i <= n; i++) {
				if (fs[i] ~ /^label=/) {
					if (index("," $3 ",", "," substr(fs[i], 7) ",") == 0) ok = 0
				} else if (fs[i] ~ /^name=/) {
					pat = substr(fs[i], 6); gsub(/[\^$]/, "", pat)
					if (("/" $2) != pat) ok = 0
				}
			}
			if (ok) print $1
		}' "$DK_FIXTURE"
	;;
stop) ;;
rm)
	shift
	for id in "$@"; do
		case "$id" in -*) continue ;; esac
		grep -v "^$id " "$DK_FIXTURE" >"$DK_FIXTURE.n" || true
		mv "$DK_FIXTURE.n" "$DK_FIXTURE"
	done
	;;
*) exit 0 ;;
esac
DK
chmod +x ubin/docker
# shellcheck disable=SC2016
sed '/^main "\$@"$/d' "$root/install/uninstall.sh" >uninstall-lib.sh
cat >uninstall-run.sh <<'RUN'
. ./uninstall-lib.sh
REMOVE_ATTEMPTS=1
REMOVE_DELAY=0
purge=$1
remove_managed_containers
RUN
M=dev.ciphera.managed=true
mkfix() {
	cat >fixture.txt <<FIX
id1 ciphera-postgres $M,dev.ciphera.role=
id2 ciphera-buildkitd-aaaaaaaaaaaa $M,dev.ciphera.role=buildkitd
id3 ciphera-d-1111 $M,dev.ciphera.role=,dev.ciphera.deployment=d1
id4 mon-app-compose $M,dev.ciphera.role=,dev.ciphera.compose=web
id5 ciphera-postgres-copie $M,dev.ciphera.role=
id6 hors-ciphera dev.ciphera.role=buildkitd
FIX
}
mkfix
DK_FIXTURE=$work/fixture.txt PATH="$work/ubin:$PATH" sh uninstall-run.sh no
check "uninstall sans purge : PostgreSQL et buildkitd retires" \
	bash -c "! grep -q -e '^id1 ' -e '^id2 ' fixture.txt"
check "uninstall sans purge : conteneurs d'application conserves" \
	bash -c "grep -q '^id3 ' fixture.txt && grep -q '^id4 ' fixture.txt"
check "uninstall sans purge : nom voisin et conteneur non gere conserves" \
	bash -c "grep -q '^id5 ' fixture.txt && grep -q '^id6 ' fixture.txt"
mkfix
DK_FIXTURE=$work/fixture.txt PATH="$work/ubin:$PATH" sh uninstall-run.sh yes
check "uninstall --purge : tous les conteneurs geres retires" \
	bash -c "test \"\$(cut -d' ' -f1 fixture.txt)\" = id6"


# --- sign-release.sh ----------------------------------------------------------
mkdir dist
printf 'archive amd64\n' >"dist/ciphera-$version-x86_64-unknown-linux-musl.tar.gz"
printf 'archive arm64\n' >"dist/ciphera-$version-aarch64-unknown-linux-musl.tar.gz"
for d in dist-enc dist-wrong dist-next dist-unknown dist-other; do cp -r dist "$d"; done

sh "$root/install/sign-release.sh" "$version" plain.key plain.pub dist "release de test" >/dev/null
for f in SHA256SUMS SHA256SUMS.minisig install.sh install.sh.minisig install.sh.sha256 latest.json latest.json.minisig; do
	check "sign-release : $f" test -s "dist/$f"
done
check "sign-release : SHA256SUMS = les deux archives" test "$(wc -l <dist/SHA256SUMS)" -eq 2
check "sign-release : install.sh publie = cle injectee" cmp -s dist/install.sh released.sh
check "sign-release : signature verifiee" minisign -Vq -p plain.pub -m dist/SHA256SUMS
check "sign-release : commentaire de confiance" \
	grep -q "trusted comment: ciphera $version SHA256SUMS (release de test)" dist/SHA256SUMS.minisig
check "sign-release : latest.json" grep -qx "{\"version\":\"$version\"}" dist/latest.json

# --- sign-latest.sh / check-latest.sh -------------------------------------------
check "latest : signature verifiee (minisign)" minisign -Vq -p plain.pub -m dist/latest.json
check "latest : check-latest accepte le fichier de la release" \
	sh "$root/install/check-latest.sh" plain.pub dist "$version"
comment=$(sed -n '3s/^trusted comment: //p' dist/latest.json.minisig)
issued=$(printf '%s' "$comment" | sed -n 's/.* issued=\([0-9]*\) .*/\1/p')
check "latest : commentaire de confiance (version, date, echeance a 120 jours)" \
	test "$comment" = "ciphera-latest version=$version issued=$issued expires=$((issued + 10368000))"
check "latest : date de signature recente" test $(($(date -u +%s) - issued)) -lt 600
check "latest : version differente refusee" \
	fails_with "contenu inattendu" sh "$root/install/check-latest.sh" plain.pub dist v9.9.9
check "latest : autre cle refusee" \
	fails_with "aucune cle" sh "$root/install/check-latest.sh" other.pub dist "$version"
# Re-signature (refresh-latest.yml) : meme version, nouvelle signature.
mkdir latest-refresh
cp dist/latest.json latest-refresh/
sh "$root/install/sign-latest.sh" "$version" plain.key plain.pub latest-refresh dist/SHA256SUMS >/dev/null
check "latest : re-signature sans changer la version" \
	bash -c "cmp -s dist/latest.json latest-refresh/latest.json && sh '$root/install/check-latest.sh' plain.pub latest-refresh '$version' >/dev/null"
# Jamais une autre version : latest.json present d'une autre version.
mkdir latest-other
printf '{"version":"v9.9.9"}\n' >latest-other/latest.json
check "latest : re-signature d'une autre version refusee" \
	fails_with "version jamais changee" sh "$root/install/sign-latest.sh" "$version" plain.key plain.pub latest-other dist/SHA256SUMS
check "latest : rien n'est signe apres un refus" test ! -e latest-other/latest.json.minisig
# Rotation activee apres la release (secret = cle suivante, ensemble de
# confiance a deux cles) : la release a ete signee par la cle courante, la
# re-signature par une autre cle est refusee.
mkdir latest-rotated
cp dist/latest.json latest-rotated/
check "latest : re-signature par une autre cle que celle de la release refusee" \
	fails_with "publier la release suivante" sh "$root/install/sign-latest.sh" "$version" next.key set.pub latest-rotated dist/SHA256SUMS
check "latest : aucune signature laissee apres le refus de rotation" test ! -e latest-rotated/latest.json.minisig
check "latest : meme ensemble, cle de la release : accepte" \
	sh "$root/install/sign-latest.sh" "$version" plain.key set.pub latest-rotated dist/SHA256SUMS
# Version annoncee modifiee apres la signature.
cp -r latest-refresh latest-tampered
printf '{"version":"v9.9.9"}\n' >latest-tampered/latest.json
check "latest : fichier altere refuse" \
	fails_with "aucune cle" sh "$root/install/check-latest.sh" plain.pub latest-tampered v9.9.9
# Signature valide, commentaire d'une autre version (ou mal forme, ou echu).
latest_signed() { # <dossier> <commentaire>
	mkdir -p "$1"
	printf '{"version":"%s"}\n' "$version" >"$1/latest.json"
	minisign -S -s plain.key -m "$1/latest.json" -t "$2" </dev/null >/dev/null 2>&1
}
now=$(date -u +%s)
latest_signed latest-mismatch "ciphera-latest version=v9.9.9 issued=$now expires=$((now + 3600))"
check "latest : version du commentaire differente refusee" \
	fails_with "commentaire de confiance mal forme" sh "$root/install/check-latest.sh" plain.pub latest-mismatch "$version"
latest_signed latest-plain "timestamp:$now file:latest.json"
check "latest : commentaire par defaut de minisign refuse" \
	fails_with "commentaire de confiance mal forme" sh "$root/install/check-latest.sh" plain.pub latest-plain "$version"
old_ts=$((now - 200 * 86400))
latest_signed latest-expired "ciphera-latest version=$version issued=$old_ts expires=$((old_ts + 10368000))"
check "latest : echeance passee refusee a la publication" \
	fails_with "echeance passee" sh "$root/install/check-latest.sh" plain.pub latest-expired "$version"
latest_signed latest-long "ciphera-latest version=$version issued=$now expires=$((now + 10368001))"
check "latest : echeance au-dela de 120 jours refusee" \
	fails_with "echeance invalide" sh "$root/install/check-latest.sh" plain.pub latest-long "$version"
future_ts=$((now + 2 * 86400))
latest_signed latest-future "ciphera-latest version=$version issued=$future_ts expires=$((future_ts + 10368000))"
check "latest : date de signature a plus d'un jour dans le futur refusee" \
	fails_with "date de signature dans le futur" sh "$root/install/check-latest.sh" plain.pub latest-future "$version"

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
# Fichiers annexes ajoutes a une release publiee apres sa signature : hors de
# la liste signee, ils ne doivent jamais etre servis par le site.
printf '<p>annexe</p>\n' >"releases/$kept/annexe.html"
printf 'x\n' >"releases/$kept/.cache"
# Nom avec saut de ligne dont la seconde ligne est un nom signe : retire.
nl_name="x
ciphera-$version-x86_64-unknown-linux-musl.tar.gz"
printf 'x\n' >"releases/$kept/$nl_name"
# Release v9.9.5 (bonne cle) dont SHA256SUMS signe un nom avec espace et un
# nom echappe (ligne commencant par \) ; un fichier non signe porte le debut
# du nom avec espace. sign-release refuse ces noms (verifie plus bas) : les
# deux lignes sont donc ecrites a la main, au format de sha256sum, puis
# SHA256SUMS est resigne directement par minisign (meme commentaire de
# confiance). Pas de sha256sum -c ici : seule la lecture par pages-site compte.
sp=v9.9.5
mkdir -p "dist-$sp" "releases/$sp" dist-badname
cp dist/ciphera-*.tar.gz "dist-$sp/"
sh "$root/install/sign-release.sh" "$sp" plain.key plain.pub "dist-$sp" release >/dev/null
printf 'espace\n' >"dist-$sp/ciphera-a b.tar.gz"
printf 'echappe\n' >"dist-$sp/ciphera-x\\y.tar.gz"
{
	printf '%s  %s\n' "$(sha256sum <"dist-$sp/ciphera-a b.tar.gz" | cut -d' ' -f1)" 'ciphera-a b.tar.gz'
	printf '\\%s  %s\n' "$(sha256sum <"dist-$sp/ciphera-x\\y.tar.gz" | cut -d' ' -f1)" 'ciphera-x\\y.tar.gz'
} >>"dist-$sp/SHA256SUMS"
rm -f "dist-$sp/SHA256SUMS.minisig"
minisign -S -s plain.key -m "dist-$sp/SHA256SUMS" -t "ciphera $sp SHA256SUMS (release)" </dev/null >/dev/null 2>&1
cp "dist-$sp"/* "releases/$sp/"
# sign-release refuse d'emblee les noms hors du jeu sur (antislash, espace).
cp dist/ciphera-*.tar.gz dist-badname/
printf 'x\n' >'dist-badname/ciphera-x\y.tar.gz'
check "sign-release : nom d'archive avec antislash refuse" \
	fails_with "nom d'archive non autorise" sh "$root/install/sign-release.sh" "$sp" plain.key plain.pub dist-badname release
rm -f 'dist-badname/ciphera-x\y.tar.gz'
printf 'x\n' >'dist-badname/ciphera-a b.tar.gz'
check "sign-release : nom d'archive avec espace refuse" \
	fails_with "nom d'archive non autorise" sh "$root/install/sign-release.sh" "$sp" plain.key plain.pub dist-badname release
printf 'x\n' >"releases/$sp/ciphera-a"
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
	cp -R "$GH_STUB_RELEASES/$tag/." "$dir/"
	;;
*) echo "gh (bouchon) : $*" >&2; exit 2 ;;
esac
GH
chmod +x bin/gh
export GH_STUB_RELEASES="$work/releases" GH_STUB_LATEST="$version"
export GH_STUB_TAGS="$version $old $kept $ret $sp"

pages() { # <cles retirees> <dist> <version> <sortie>
	env PATH="$work/bin:$PATH" sh "$root/install/pages-site.sh" ciphera-dev/deploy-hub plain.pub "$@"
}
pages retired.pub dist "$version" site >/dev/null 2>pages.err
check "site : install.sh = fichier signe du run" cmp -s site/install.sh dist/install.sh
check "site : install.sh.minisig" cmp -s site/install.sh.minisig dist/install.sh.minisig
check "site : install.sh.sha256" cmp -s site/install.sh.sha256 dist/install.sh.sha256
check "site : signature verifiee" minisign -Vq -p plain.pub -m site/install.sh -x site/install.sh.minisig
check "site : releases/latest.json" cmp -s site/releases/latest.json dist/latest.json
check "site : releases/latest.json.minisig" cmp -s site/releases/latest.json.minisig dist/latest.json.minisig
check "site : latest.json du site verifie" sh "$root/install/check-latest.sh" plain.pub site/releases "$version"
check "site : archives de la release" \
	cmp -s "site/releases/$version/ciphera-$version-x86_64-unknown-linux-musl.tar.gz" \
	"dist/ciphera-$version-x86_64-unknown-linux-musl.tar.gz"
check "site : release precedente gardee" test -f "site/releases/$kept/SHA256SUMS"
check "site : release signee par une cle retiree gardee" test -f "site/releases/$ret/SHA256SUMS"
check "site : release signee par une cle inconnue ignoree" test ! -e "site/releases/$old"
check "site : fichier annexe d'une release precedente retire" test ! -e "site/releases/$kept/annexe.html"
check "site : fichier cache d'une release precedente retire" test ! -e "site/releases/$kept/.cache"
check "site : avertissement pour le fichier annexe retire" grep -q "$kept.*annexe.html" pages.err
check "site : nom avec saut de ligne retire" test ! -e "site/releases/$kept/$nl_name"
# La release v9.9.5 n'est lue que si sha256sum -c accepte une ligne echappee
# (coreutils : oui, c'est le cas des runners de release ubuntu-24.04). uutils
# (ubuntu 26.04) la rejette : pages-site refuse alors la release en entier,
# ce qui est plus strict et reste sur ; seule la liste signee compte ici.
mkdir -p probe
printf '\\%s  %s\n' "$(printf x | sha256sum | cut -d' ' -f1)" 'a\\b' >probe/S
printf x >'probe/a\b'
if (cd probe && sha256sum -c --strict --quiet S >/dev/null 2>&1); then
	check "site : nom signe avec espace garde intact" test -f "site/releases/$sp/ciphera-a b.tar.gz"
	check "site : debut d'un nom signe avec espace retire" test ! -e "site/releases/$sp/ciphera-a"
	check "site : ligne echappee de SHA256SUMS : fichier retire" test ! -e "site/releases/$sp/ciphera-x\y.tar.gz"
	check "site : avertissement pour la ligne echappee" grep -q "$sp : 1 ligne(s) de SHA256SUMS echappee" pages.err
	check "site : archives ordinaires de v9.9.5 gardees" \
		test -f "site/releases/$sp/ciphera-$version-x86_64-unknown-linux-musl.tar.gz"
else
	printf 'note : sha256sum -c rejette les lignes echappees (uutils) : v9.9.5 refusee en entier\n'
	check "site : ligne echappee, sha256sum -c strict : release refusee" test ! -e "site/releases/$sp"
fi
entries() { # <dossier> : noms des entrees (fichiers caches compris), tries
	find "$1" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort
}
signed_list() { # <dossier> : fichiers de la liste signee, tries
	{
		awk '{ print $2 }' "$1/SHA256SUMS"
		printf '%s\n' SHA256SUMS SHA256SUMS.minisig install.sh install.sh.minisig install.sh.sha256
	} | sort
}
check "site : release precedente = liste signee exactement" \
	test "$(entries "site/releases/$kept")" = "$(signed_list "site/releases/$kept")"
check "site : release du run = liste signee exactement" \
	test "$(entries "site/releases/$version")" = "$(signed_list "site/releases/$version")"
check "site : avertissement pour la release ignoree" grep -q "$old" pages.err
check "site : CNAME" grep -qx get.ciphera.ch site/CNAME
check "site : cles publiques = ensemble courant seulement" cmp -s site/ciphera.pub plain.pub
check "site : index et 404 vers GitHub Releases" \
	grep -q 'https://github.com/ciphera-dev/deploy-hub/releases' site/index.html site/404.html
# Page d'accueil (install/site/) : fichiers publies sous /assets/, version
# et liens de signature injectes, aucune ressource ni URL tierce.
check "site : fichiers statiques de la page d'accueil publies" \
	diff -r --exclude=brand "$root/install/site/assets" site/assets
check "site : logo officiel" cmp -s "$root/web/src/assets/brand/logo.svg" site/assets/brand/logo.svg
check "site : favicon officiel" cmp -s "$root/web/public/favicon.svg" site/assets/brand/favicon.svg
check "site : sources du site non publiees (CREDITS.md)" test ! -e site/CREDITS.md
check "site : aucun fichier de la page d'accueil sous /releases/" \
	test -z "$(find site/releases \( -name '*.css' -o -name '*.js' -o -name '*.webp' -o -name '*.svg' -o -name '*.html' -o -name assets \) -print)"
check "site : capture du panneau < 400 Ko" test "$(wc -c <site/assets/dashboard.webp)" -lt 409600
check "site : capture du panneau sans EXIF ni XMP" bash -c "! grep -aq -e EXIF -e 'XMP ' site/assets/dashboard.webp"
for p in index.html 404.html; do
	check "site : $p : version publiee injectee" grep -q "Version publiée : $version" "site/$p"
	check "site : $p : aucun marqueur de gabarit restant" bash -c "! grep -q '@@' site/$p"
	check "site : $p : CSP stricte" grep -q "Content-Security-Policy\" content=\"default-src 'none'; script-src 'self'; style-src 'self'" "site/$p"
	# Toute URL absolue est une page du depot, le site lui-meme (commande
	# d'installation) ou un exemple en texte (example.com, <IP>).
	check "site : $p : aucune URL externe hors du depot" bash -c "! grep -Eo '(https?:)?//[^\"<> ]+' site/$p |
		grep -Ev '^https://(github\.com/ciphera-dev/deploy-hub(/[^\"<> ]*)?|get\.ciphera\.ch/install\.sh|app\.example\.com|&lt;IP&gt;:8443/setup#token=…)\$'"
	check "site : $p : liens absolus vers le depot seulement" bash -c "! grep -Eo '(href|src)=\"(https?:)?//[^\"]*\"' site/$p |
		grep -Ev '^(href|src)=\"https://github\.com/ciphera-dev/deploy-hub(/[^\"]*)?\"\$'"
	check "site : $p : seul mailto:info@ciphera.ch" bash -c "! grep -Eo 'mailto:[^\"<> ]*' site/$p | grep -vx 'mailto:info@ciphera.ch'"
	check "site : $p : scripts, styles et images locaux" bash -c "! grep -Eo '<(script|link|img)[^>]*(src|href)=\"[^\"]*\"' site/$p | grep -Ev '(src|href)=\"/assets/'"
	check "site : $p : aucun style ni script en ligne" bash -c "! grep -Eq '<script>|<style|style=\"| on[a-z]+=\"' site/$p"
	# Chaque fichier local reference existe dans le site construit.
	missing=$(grep -Eo '(src|href)="/[^"#]*"' "site/$p" | sed 's/^[a-z]*="\///; s/"$//' | sort -u |
		while IFS= read -r f; do [ -z "$f" ] || [ -f "site/$f" ] || printf '%s ' "$f"; done)
	check "site : $p : liens locaux presents ($missing)" test -z "$missing"
done
for f in install.sh install.sh.minisig install.sh.sha256 ciphera.pub releases/latest.json; do
	check "site : index.html : lien vers /$f" grep -q "href=\"/$f\"" site/index.html
done
for u in 'https://github.com/ciphera-dev/deploy-hub"' 'deploy-hub/releases"' 'deploy-hub/deployments"' \
	'deploy-hub/packages"' 'href="mailto:info@ciphera.ch"'; do
	check "site : index.html : lien $u" grep -qF -- "$u" site/index.html
done
check "site : index.html : commande d'installation" grep -q 'curl -fsSL https://get.ciphera.ch/install.sh | sudo sh' site/index.html
check "site : 404.html : contact" grep -q 'mailto:info@ciphera.ch' site/404.html

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
# latest.json echu (refresh-latest.yml en retard) : le site n'est pas construit.
cp -r dist dist-latest-expired
cp latest-expired/latest.json latest-expired/latest.json.minisig dist-latest-expired/
check "site : latest.json echu refuse" \
	fails_with "latest.json de $version" pages retired.pub dist-latest-expired "$version" site7
cp -r dist dist-latest-nosig
rm dist-latest-nosig/latest.json.minisig
check "site : latest.json sans signature refuse" \
	fails_with "latest.json de $version" pages retired.pub dist-latest-nosig "$version" site8
GH_STUB_TAGS='v1;rm' check "site : tag publie invalide refuse" fails_with "tag invalide" pages retired.pub dist "$version" site4
GH_STUB_LATEST=v9.9.10 check "site : run plus ancien que la derniere release refuse" \
	fails_with "derniere release publiee est v9.9.10" pages retired.pub dist "$version" site6

if [ "$fails" -ne 0 ]; then
	printf '%d echec(s)\n' "$fails"
	exit 1
fi
printf 'tous les tests passent\n'
