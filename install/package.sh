#!/bin/sh
# Construit l'archive de release ciphera-<version>-<cible>.tar.gz :
#   ciphera-<version>-<cible>/bin/        ciphera, ciphera-agent, ciphera-proxy, ciphera-cli
#   ciphera-<version>-<cible>/systemd/    unites de deploy/systemd/
#   ciphera-<version>-<cible>/containers/ images epinglees de deploy/containers/
#   ciphera-<version>-<cible>/apparmor/   profil de buildkitd (deploy/apparmor/)
#   ciphera-<version>-<cible>/tools.manifest  outils tiers epingles pour <cible>
#                                         (install.sh --tools-manifest), lus par
#                                         ciphera-cli updater
#   ciphera-<version>-<cible>/migration   derniere migration de la base
#                                         (sauvegardes restaurables par l'updater)
#
#   install/package.sh <version> <cible> <dossier des binaires> <dossier de sortie>
#
# Archive reproductible : ordre, proprietaire et date fixes (SOURCE_DATE_EPOCH).
# CIPHERA_PACKAGE_EXTRA_UNITS (tests seulement) : dossier d'unites ajoutees
# a systemd/ (release de test qui ajoute une unite, install/tests/update.sh).
set -eu

die() {
	printf 'package.sh : %s\n' "$*" >&2
	exit 1
}

main() {
	[ "$#" -eq 4 ] || die "usage : package.sh <version> <cible> <binaires> <sortie>"
	version=$1
	target=$2
	bin_dir=$3
	out_dir=$4
	case "$version" in
	v[0-9]*.[0-9]*.[0-9]*) ;;
	*) die "version invalide : $version" ;;
	esac
	case "$version" in *[!v0-9.]*) die "version invalide : $version" ;; esac
	case "$target" in
	x86_64-unknown-linux-musl | aarch64-unknown-linux-musl) ;;
	*) die "cible invalide : $target" ;;
	esac

	root=$(cd "$(dirname "$0")/.." && pwd)
	name="ciphera-$version-$target"
	stage=$(mktemp -d)
	trap 'rm -rf "$stage"' EXIT

	mkdir -p "$stage/$name/bin" "$stage/$name/systemd" "$stage/$name/containers" "$stage/$name/apparmor"
	for b in ciphera ciphera-agent ciphera-proxy ciphera-cli; do
		[ -f "$bin_dir/$b" ] || die "binaire absent : $bin_dir/$b"
		cp "$bin_dir/$b" "$stage/$name/bin/$b"
		chmod 0755 "$stage/$name/bin/$b"
	done
	for u in "$root"/deploy/systemd/*; do
		cp "$u" "$stage/$name/systemd/"
	done
	if [ -n "${CIPHERA_PACKAGE_EXTRA_UNITS:-}" ]; then
		for u in "$CIPHERA_PACKAGE_EXTRA_UNITS"/*; do
			[ -f "$u" ] || die "unite de test introuvable : $u"
			cp "$u" "$stage/$name/systemd/"
		done
	fi
	cp "$root"/deploy/containers/*.digest "$stage/$name/containers/"
	cp "$root"/deploy/apparmor/* "$stage/$name/apparmor/"
	cp "$root/LICENSE" "$stage/$name/"
	sh "$root/install/install.sh" --tools-manifest "$target" >"$stage/$name/tools.manifest" ||
		die "manifeste des outils impossible"
	# Derniere migration de la base (numero du dernier fichier de
	# crates/control-plane/migrations) : l'updater refuse de restaurer une
	# sauvegarde prise avec d'autres migrations.
	last=$(find "$root/crates/control-plane/migrations" -maxdepth 1 -name '[0-9]*_*.sql' |
		sed 's#.*/##; s#_.*##' | sort -n | tail -n 1)
	case "$last" in '' | *[!0-9]*) die "migrations introuvables" ;; esac
	last=$(printf '%s' "$last" | sed 's/^0*//')
	printf '%s\n' "${last:-0}" >"$stage/$name/migration"
	chmod 0644 "$stage/$name"/systemd/* "$stage/$name"/containers/* "$stage/$name"/apparmor/* \
		"$stage/$name/LICENSE" "$stage/$name/tools.manifest" "$stage/$name/migration"

	mkdir -p "$out_dir"
	tar --sort=name --owner=0 --group=0 --numeric-owner \
		--mtime="@${SOURCE_DATE_EPOCH:-0}" -C "$stage" -cf - "$name" |
		gzip -n -9 >"$out_dir/$name.tar.gz"
}

main "$@"
