# Stack et architecture — Ciphera

## Intention confirmée
- **Produit :** un PaaS auto-hébergé, **100 % open source**, avec une seule édition, celle qu'on installe soi-même (pas d'offre cloud, pas de licence, pas de modèle open-core).
- **Ce qu'on construit :**
  - un dashboard web d'administration (intention principale) ;
  - un backend (plan de contrôle) ;
  - un agent de nœud ;
  - un proxy ;
  - une CLI.
- **Installation :** `curl -fsSL https://get.ciphera.ch/install.sh | sh` (ADR 0003).
- **Référence fonctionnelle :** l'analyse de Dokploy et Coolify dans `docs/research/analyse-paas/`. On en reprend les fonctionnalités en les réimplémentant avec la stack ci-dessous. On ne copie aucun code : le code de Dokploy sous licence DSAL est exclu.

## Stack retenue (choix de l'utilisateur, 2026-11-21)

| Composant | Choix | Pourquoi |
|---|---|---|
| Interface | React 19 + Vite + TypeScript + TanStack Router/Query + Tailwind v4 + shadcn/ui | Les maquettes sont déjà en React : environ 45 écrans et 32 composants sont portés sans réécriture. Une SPA statique suffit pour une console. |
| Livraison de l'interface | `rust-embed` dans le binaire du plan de contrôle | Un seul exécutable, rien à installer côté Node en production |
| API | Axum (REST + WebSocket), utoipa → OpenAPI | Fait partie de l'écosystème Tokio ; le temps réel (logs, terminal, métriques) passe par le même binaire |
| Base de données | PostgreSQL 17 + SQLx (requêtes vérifiées à la compilation, migrations) | Les requêtes sont vérifiées dès la compilation |
| Files de tâches | Postgres `SELECT … FOR UPDATE SKIP LOCKED` ou apalis | Pas de Redis, donc un composant de moins à opérer |
| Authentification | webauthn-rs (passkeys) + TOTP + jetons API à permissions | Pas de mot de passe seul pour l'admin, MFA natif |
| Secrets | Chiffrement enveloppe avec `age`, écriture seule dans l'API | Un secret n'est jamais renvoyé au client |
| Observabilité | OpenTelemetry + endpoint Prometheus | Standard, branchable sur Grafana |
| Agent de nœud | Tokio + QUIC (`quinn`) + mTLS (mini-PKI `rcgen`), `bollard`, `rusqlite` | Connexions persistantes, reprise après perte réseau ; API fermée sans exécution arbitraire |
| Builds | BuildKit (`buildkitd`) piloté par l'agent : Railpack par défaut, Dockerfile, Compose | Les trois formats attendus ; cache, build secrets et multi-arch |
| Proxy / TLS | Pingora + `instant-acme` | Proxy programmable en Rust ; certificats ACME gérés par Ciphera (remplace Caddy/Traefik) |
| CLI | Rust `clap` + client généré (progenitor) depuis le même OpenAPI | La CLI et l'interface suivent le même contrat |
| Client TS | openapi-typescript (ou orval) + TanStack Query | Typage de bout en bout |

**Framework des maquettes :** React est **gardé** et porté. Les globaux `window.*` deviennent des imports ESM, les données en dur deviennent TanStack Query + MSW en dev, et Babel exécuté dans le navigateur est remplacé par Vite.

## Arborescence cible

```
ciphera-deploy/
├── Cargo.toml                  [workspace] members = crates/*, resolver = "3"
├── rust-toolchain.toml · deny.toml · justfile
├── crates/
│   ├── control-plane/          binaire `ciphera` : Axum, SQLx, auth, jobs, secrets, sert la SPA (rust-embed)
│   │   ├── src/{main.rs, http/{routes,ws,middleware}, domain/<projects|servers|deployments|domains|…>, auth/, jobs/, secrets/, telemetry/, embed.rs}
│   │   ├── migrations/
│   │   └── tests/              intégration (testcontainers Postgres)
│   ├── agent/                  binaire `ciphera-agent` : quinn + mTLS, bollard, buildkit, rusqlite
│   ├── proxy/                  binaire `ciphera-proxy` : Pingora + instant-acme
│   ├── cli/                    binaire `ciphera-cli` : clap + client généré
│   ├── proto/                  schéma agent ↔ plan de contrôle, versionné (deploy/stop/logs/metrics/pull + actions typées pare-feu/scan/terminal)
│   ├── api-types/              DTO REST + génération OpenAPI (utoipa)
│   ├── pki/                    mini-CA rcgen, émission et rotation des certificats d'agent
│   └── shared/                 erreurs, config, ids, télémétrie commune
├── web/                        SPA (déjà présents : src/design-system, src/styles, src/assets, public/)
│   └── src/{main.tsx, app/, routes/, features/<domaine>/, design-system/, components/ui/, styles/, lib/, assets/, mocks/}
├── install/                    install.sh, uninstall.sh, ciphera.pub (minisign), tests/
├── deploy/systemd/             ciphera.service, ciphera-agent.service, ciphera-proxy.service
├── .github/workflows/          ci.yml, release.yml (musl multi-arch → SHA256SUMS → minisign + cosign)
├── prototypes/                 maquettes d'origine (référence visuelle, jamais importées)
├── docs/                       adr/, architecture/, design/, design-system/, research/, security/, cleanup/
└── scripts/                    gen-openapi, gen-clients, dev
```

**Conventions :**
- **Nommage :** crates en kebab-case ; composants en PascalCase `.tsx` ; hooks en `useX.ts` ; routes en kebab-case.
- **Composant visuel générique :** va dans `web/src/design-system/components/<cat>/`.
- **Primitive à comportement :** passe par la CLI shadcn dans `components/ui/`, puis le design system l'enveloppe.
- **Composant propre à un domaine :** va dans `features/<domaine>/components/`.
- **Route :** elle reste fine (loader + layout) ; l'écran vit dans `features/`.
- **Indépendance des maquettes :** rien de `prototypes/` n'est importé par `web/` ni par `crates/`.
- **En production :** pas de `window.*`, pas de CDN, pas de Babel dans le navigateur.
- **Contrat :** toute modification part de `crates/api-types`, puis `just gen`.
- **Protocole agent :** il est figé et versionné dans `crates/proto`.

## Contrat backend (résumé)

Le détail par écran est dans `docs/design/screens.md`.

**Entités :**
- organisation et espaces de travail, utilisateurs, sessions, rôles et permissions, invitations, événements d'audit ;
- projets, services, déploiements, lignes de logs, variables d'environnement, webhooks Git et deploy hooks ;
- domaines, enregistrements DNS (Cloudflare) ;
- serveurs, jetons d'enrôlement, clés SSH, règles de pare-feu, scans et menaces malware, sauvegardes et snapshots ;
- conteneurs, images et volumes Docker, bases de données, métriques, alertes ;
- événements de sécurité et règles IP, connecteurs, jetons API, webhooks sortants ;
- notifications et préférences, tickets, réglages de l'instance.

**Authentification :**
- Le premier admin est créé par l'assistant d'installation, avec un jeton de setup à usage unique.
- On se connecte par e-mail + mot de passe, ou par passkey.
- Deuxième facteur : TOTP, WebAuthn ou codes de secours. La 2FA peut être rendue obligatoire pour toute l'organisation. SSO SAML/OIDC, inclus sans licence.
- Rôles : owner, admin, ops, developer, viewer. Permissions : matrice ressource × {none, read, write, admin}, plus un périmètre par projet.

**Temps réel (SSE / WebSocket) :**
- logs de build et logs runtime ;
- terminal, en WS via l'agent ;
- métriques, événements de sécurité, statut des agents ;
- propagation DNS et émission ACME ;
- notifications, progression des tâches, streaming du brouillon de ticket IA.

**Agent :**
- Enrôlement par jeton join à usage unique (15 min), puis certificat mTLS à rotation automatique.
- API fermée définie dans `crates/proto`.
- Pare-feu, scan malware et terminal passent par des **actions typées** (ADR 0002) : aucune commande arbitraire.

**Hors périmètre :** licences, facturation, console Ciphera Cloud (supprimées le 2026-11-21).
