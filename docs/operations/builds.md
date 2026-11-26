# Builds : règles et limites

Les builds sont exécutés par l'agent du nœud avec BuildKit (`buildkitd`) : Railpack par défaut, Dockerfile ou Compose. Ce document reprend les règles et les limites qui s'appliquent à chaque build.

## Directive `# syntax=` des Dockerfile

Builds par Dockerfile (ou Compose) : la directive `# syntax=` n'accepte que `docker/dockerfile` en version `1`, `latest` ou `1.4` et plus (`1.N`, `1.N.M`), sans empreinte (`@sha256:…`) ni variante `-labs`. Un autre frontend, ou une version ancienne, pourrait ignorer le cloisonnement des caches de build par projet et par environnement. Le build échoue alors avec un message clair : retirer la ligne (frontend intégré à BuildKit) ou écrire `# syntax=docker/dockerfile:1`.

## Limites

- Un seul build à la fois par nœud : les suivants attendent leur tour, ce qui est indiqué dans leur journal.
- Le dépôt cloné ne dépasse pas 2 Gio et l'image construite 10 Gio.
- BuildKit est limité à 4096 processus, à la moitié de la mémoire du nœud (1 Gio au moins) et à tous les CPU sauf un.
- Chaque projet a son propre buildkitd et son propre état : le premier build d'un projet est plus lent (cache vide). Un seul buildkitd tourne à la fois ; il est arrêté après 10 min d'inactivité, et son état est supprimé avec le projet ou après 30 jours.
- Le ramasse-miettes d'un état garde 10 % du disque (entre 5 et 50 Gio) divisés par le nombre de projets (1 Gio au moins chacun) : le total des états peut donc dépasser 10 %.
- Un outil de build qui dépasse son délai est arrêté avec tous ses processus.

La sortie réseau des builds est décrite dans [installation.md](installation.md#sortie-réseau-des-conteneurs).
