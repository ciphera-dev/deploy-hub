# 01 — Dokploy : analyse technique complète

Version analysée : **v0.31.4** (commit `da6282cc`, 17 nov. 2026)
Licence : **Apache-2.0 pour le cœur** + **DSAL propriétaire** pour tout ce qui est dans un dossier `/proprietary`.

---

## 1. Vue d'ensemble

Dokploy est un PaaS self-hosted écrit entièrement en **TypeScript**, organisé en **monorepo pnpm**. Il se positionne comme l'alternative "moderne et légère" : une seule machine suffit, interface Next.js réactive, déploiements Git push-to-deploy, bases de données en un clic, et clustering via **Docker Swarm**.

## 2. Structure du monorepo

```
dokploy/
├── apps/
│   ├── dokploy/        # Application principale : Next.js (UI + serveur tRPC)
│   ├── api/            # API publique séparée : Hono + Inngest (files de tâches)
│   ├── monitoring/     # Service de métriques en Go (Fiber + SQLite)
│   └── schedules/      # Planificateur : Hono + BullMQ (Redis/ioredis)
├── packages/
│   └── server/         # Bibliothèque partagée : tout le domaine métier
└── scripts/
```

Point clé : **`packages/server` contient tout le cœur métier** (services, schéma DB, builders, setup Traefik, utils). `apps/dokploy` est une coquille Next.js qui l'importe. C'est une bonne séparation, réutilisable.

## 3. Stack technique détaillée

| Couche | Choix de Dokploy |
|---|---|
| Frontend | Next.js (React 19), Tailwind, shadcn-style components |
| API interne | **tRPC 11** (typée de bout en bout) |
| API publique | **Hono** (REST, OpenAPI généré) + Inngest pour les tâches |
| ORM / DB | **Drizzle ORM** + PostgreSQL (driver `postgres.js`) |
| Auth | **better-auth** (plugins : admin, organization, twoFactor, api-key, passkey, SSO, SCIM) |
| Docker | **dockerode** (API Docker en JS) + CLI docker dans le conteneur |
| SSH | **ssh2** (librairie JS native) pour les serveurs distants |
| Terminal web | **node-pty** + WebSocket (`ws`) |
| Builds | Nixpacks 1.41, Railpack 0.15.4, Paketo/`pack` 0.39.1, Dockerfile, Docker Compose, static, Heroku-ish |
| Reverse proxy | **Traefik v3.6.7** (HTTP/3 inclus, UDP 443) |
| Planification | node-schedule + BullMQ (app `schedules`) |
| Métriques | App Go dédiée (Fiber + SQLite + rétention cron) |
| Backups | **rclone** intégré dans l'image Docker |
| Télémétrie | PostHog (`trackDeploymentResult` dans le flux de déploiement) |
| IA | SDK multi-providers : Anthropic, OpenAI, Azure, Cohere, Mistral, DeepInfra, Ollama |

L'image Docker de production est parlante : `node:24.22.1-bookworm-slim` + Docker CLI 28.5.2 + rclone + Nixpacks + Railpack + `pack` + tsx global. Tout tourne dans **un seul conteneur qui monte le socket Docker de l'hôte** — c'est là la racine de la plupart de ses problèmes de sécurité (voir `03-securite-et-cve.md`).

## 4. Modèle de données (Drizzle, ~47 modules de schéma)

`packages/server/src/db/schema/` :

- **Cœur** : `application`, `compose`, `deployment`, `project`, `environment`, `server`, `domain`, `mount`, `port`, `redirects`, `security` (basic auth), `certificate`
- **Bases de données managées** : `postgres`, `mysql`, `mariadb`, `mongo`, `redis`, `libsql`
- **Git** : `github`, `gitlab`, `gitea`, `bitbucket`, `git-provider` (générique), `ssh-key`
- **Ops** : `backups`, `volume-backups`, `schedule`, `rollbacks`, `preview-deployments`, `registry`, `destination`, `cluster`/swarm
- **Plateforme** : `user`, `session`, `notification`, `tag`, `audit-log`, `transfer`
- **Avancé** : `ai`, `sandbox`, `vault-provider`, `dns-provider`, `telemetry-provider`, `forward-auth`, `sso`, `scim`, `patch`, `web-server-settings`

Hiérarchie : **Organization → Project → Environment → (Application | Compose | Database)**.

## 5. Flux de déploiement (le chemin critique)

1. Push Git / clic UI → création d'une ligne `deployment` en DB.
2. Clone du repo dans `/etc/dokploy/...` (local) ou via SSH sur le serveur distant.
3. Build selon le type : `builders/nixpacks.ts`, `railpack.ts`, `paketo.ts`, `docker-file.ts`, `compose.ts`, `static.ts`, `heroku.ts`, `drop.ts` (upload d'archive).
4. Exécution des commandes : **`execAsync`** (local, `child_process.exec`) ou **`execAsyncRemote`** (SSH via ssh2). ⚠️ Historiquement, ces fonctions prennent des **chaînes shell interpolées** — c'est le pattern à l'origine de 5 CVE critiques CVSS 9.9 corrigées en 0.29.13. Le code récent utilise `shell-quote` (`quote()`) mais le pattern chaîne-de-commande reste structurellement fragile.
5. Routage : Dokploy écrit des fichiers YAML dans `/etc/dokploy/traefik/dynamic/` (**provider fichier** de Traefik) pour les apps standalone ; les labels Swarm pour le mode cluster.
6. Traefik (conteneur standalone depuis v0.20, ou service Swarm en mode cluster) route le trafic, gère Let's Encrypt.

**Faiblesse connue du design** : pour les apps standalone en mode Swarm, le provider fichier génère des URLs `http://service:port` qui reposent sur la résolution DNS VIP de Docker — source de 404 documentés (discussions #4228, #3536, #3980). Le contournement officiel est de passer en Docker Compose avec des labels persistants.

## 6. Multi-serveur et clustering

Trois modes (docs officielles) :

| Mode | Mécanisme | Registre requis | Isolation |
|---|---|---|---|
| Serveur Dokploy | Tout sur la machine du UI | Non | — |
| Serveurs distants | **SSH** (execAsyncRemote) | Seulement pour les build servers | Haute |
| Cluster Swarm | `docker swarm join` + services | **Oui** (registry obligatoire) | Faible (cluster partagé) |

En mode cluster, Traefik tourne en service Swarm contraint aux managers ; pas de nettoyage automatique du stockage sur les nœuds supplémentaires.

## 7. Surface propriétaire (DSAL)

Dans `apps/dokploy/server/api/routers/proprietary/` : `audit-log`, `custom-role`, `forward-auth`, `license-key`, `scim`, `sso`, `whitelabeling`. Conséquence directe pour vous : **le whitelabeling et le SSO sont payants/sous licence commerciale** — si vous voulez un PaaS "à votre branding", forker Dokploy ne suffit pas légalement pour ces parties.

## 8. Forces et faiblesses

**Forces**
- Stack 100 % TypeScript, typage de bout en bout (tRPC + Drizzle + Zod) → DX excellente.
- UI moderne et rapide ; Next.js unifie front et back.
- Monorepo propre, domaine métier isolé dans `packages/server`.
- Builders variés (Nixpacks, Railpack, Paketo, Dockerfile, Compose, statique).
- Intégration IA native (multi-providers) — aucun concurrent ne l'a.
- Métriques déportées dans un microservice Go léger.

**Faiblesses**
- **Sécurité historiquement mauvaise** : injection shell systémique (execAsync/execAsyncRemote), identifiants DB codés en dur dans install.sh (CVE-2026-24840), terminal WebSocket injectable (CVE-2026-24841). Le conteneur Dokploy a le socket Docker de l'hôte = RCE conteneur ⇒ RCE hôte.
- Télémétrie PostHog activée dans le flux de déploiement.
- Jeunesse : routing Traefik file-provider fragile en Swarm, pas de vraie HA.
- Fonctionnalités clés (SSO, audit, whitelabel) en licence propriétaire.
- Clustering Swarm : Docker Swarm est en maintenance déclinante dans l'industrie (pari technique risqué à long terme).
- Pas d'équivalent aux 386 templates one-click de Coolify.
