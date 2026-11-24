# ADR 0001 — Structure du projet Ciphera

- Statut : accepté
- Date : 2026-11-21

## Contexte

Ciphera est un PaaS auto-hébergé, 100 % open source, concurrent de Dokploy et Coolify, écrit en Rust. Il comprend un plan de contrôle, un agent de nœud, un proxy et une CLI.

Le point de départ est un export Claude Design, le « Ciphera Design System ». Il contient :
- 32 composants React (ESM, avec .d.ts) ;
- des tokens CSS ;
- une quarantaine d'écrans de dashboard en JSX global, qui reposent sur React 18 UMD, Babel exécuté dans le navigateur, un `_ds_bundle.js` de 1,4 Mo et des données factices en dur ;
- des pages HTML autonomes : connexion, inscription, 2FA, installation, 404.

Il n'existait ni package.json, ni bundler, ni tests, ni backend.

L'objectif est d'avoir un seul exécutable par rôle, dont un binaire de plan de contrôle qui sert aussi l'interface.

## Décision

1. **Monorepo** : un workspace Cargo dans `crates/` (`control-plane`, `agent`, `proxy`, `cli`, `proto`, `api-types`, `pki`, `shared`) et une SPA dans `web/`.
2. **Interface** : React 19 + Vite + TypeScript + TanStack Router/Query + Tailwind v4 + shadcn/ui, compilée en statique et embarquée dans `control-plane` via `rust-embed`.
3. **Design system** : il vit dans `web/src/design-system/` (composants repris de l'export). Les tokens vivent dans `web/src/styles/tokens/` et sont exposés à Tailwind via `@theme`. shadcn ne fournit que les primitives à comportement, restylées avec ces tokens.
4. **Écrans** : les écrans des maquettes sont portés, jamais redessinés, vers `web/src/features/<domaine>/`, avec des routes dans `web/src/routes/`. Les maquettes d'origine restent lançables dans `prototypes/` comme référence visuelle (contrat dans `docs/design/screens.md`).
5. **Contrat d'API** : il a une source unique, `crates/api-types` (utoipa → OpenAPI). On en génère le client TS de la SPA et le client Rust de la CLI.
6. **Protocole agent** : QUIC (quinn) avec mTLS obligatoire (mini-PKI rcgen, crate `pki`). L'API est fermée et définie dans `crates/proto` (deploy/stop/logs/metrics/pull + actions typées), sans exécution arbitraire.
7. **Données** :
   - Côté plan de contrôle : PostgreSQL 17 + SQLx (migrations dans `crates/control-plane/migrations`), files de tâches en Postgres, pas de Redis.
   - Côté agent : un état local en SQLite.
8. **Proxy** : Pingora + instant-acme.
9. **Installation** : `curl -fsSL https://get.ciphera.ch/install.sh | sh`, avec vérification signée des releases (ADR 0003).
10. **Open source intégral** : une seule édition, aucune fonctionnalité sous licence. Les écrans Ciphera Cloud (administration, facturation) et la section Licence ont été retirés.
11. **Documentation** : `docs/` (adr, architecture, design/screens.md, design-system, research, security, cleanup).

## Conséquences

- (+) Un binaire de plan de contrôle autonome, donc un déploiement simple pour les utilisateurs.
- (+) Environ 80 % des composants et écrans existants sont réutilisés. Le portage est mécanique : globaux `window` → imports ESM, données factices → TanStack Query + MSW.
- (+) Le typage va de bout en bout grâce à OpenAPI ; la CLI et la SPA ne divergent pas.
- (−) Les écrans portés gardent d'abord leurs styles inline. La conversion en Tailwind est une dette suivie écran par écran.
- (−) Le binaire dépend du build de `web/` : on lance `pnpm build` puis `cargo build`, l'ordre étant géré par `just`. Une feature `embed` désactivable permet un `cargo check` sans `web/dist`.
- (−) Une clé de signature des releases est à garder et à faire tourner.

## Alternatives écartées

- **SvelteKit** (stack par défaut de l'outil) : il faudrait réécrire environ 40 écrans et 32 composants, avec un risque de dérive visuelle.
- **Next.js** : le SSR est inutile pour une console embarquée, et Next.js ne permet pas un export purement statique servi par Axum sans runtime Node.
- **Garder React UMD + Babel dans le navigateur** : pas de typage, pas de tree-shaking, des CDN en production.
- **Dépôts séparés (front / back / agent)** : le contrat et le protocole divergeraient.
- **Caddy / Traefik** : remplacés par Pingora + instant-acme pour piloter le proxy depuis le code Rust.
- **Redis pour les files** : un composant de plus à opérer, alors que Postgres suffit.
- **Modèle open-core** (licence, fonctionnalités payantes) : refusé, Ciphera reste intégralement open source.
