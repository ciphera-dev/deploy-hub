# 02 — Coolify : analyse technique complète

Version analysée : **v4.6.x stable** (commit `10d1968f`, 17 nov. 2026 ; sortie stable v4.0.0 le 27 avril 2026)
Licence : **Apache-2.0 intégrale** — aucun composant propriétaire côté self-hosted.

---

## 1. Vue d'ensemble

Coolify est le PaaS self-hosted le plus complet du marché open source. Écrit en **PHP/Laravel 12**, il gère applications, bases de données, services one-click et serveurs multiples via **SSH** — sans agent obligatoire sur les serveurs gérés. ~80 700 lignes de PHP (hors vues Blade), 81 modèles Eloquent, **530 migrations**, 386 templates de services.

## 2. Architecture : plan de contrôle découplé du trafic

C'est LE concept architectural le plus important de Coolify (et le plus pertinent à reprendre) :

```
┌─────────────────────┐         SSH (chemin de contrôle)        ┌──────────────────┐
│  Instance Coolify   │ ──────────────────────────────────────► │ Serveur connecté │
│  (dashboard + API)  │   docker build / deploy / backups       │  Docker Engine   │
│                     │                                         │  Traefik/Caddy   │
└─────────────────────┘                                         │  Vos conteneurs  │
        ▲                                                       └────────▲─────────┘
        │ heartbeats + métriques (Sentinel, optionnel)                   │
        │                                          trafic utilisateur ───┘
        │                                          (ne transite JAMAIS par Coolify)
        └── coolify-sentinel (agent Rust, optionnel)
```

- **Chemin de contrôle** : dashboard/API → file d'attente (Horizon/Redis) → SSH → commandes Docker sur le serveur cible.
- **Chemin de trafic** : DNS → reverse proxy **sur le serveur qui héberge la ressource** → conteneur. Le dashboard ne relaye jamais le trafic applicatif.
- **Conséquence cruciale** : si l'instance Coolify tombe, **toutes les applications continuent de tourner**. Seuls les déploiements, webhooks et automatisations s'arrêtent. C'est un énorme avantage de résilience.

## 3. Conteneurs d'une installation standard

| Conteneur | Rôle |
|---|---|
| `coolify` | Dashboard + API + Horizon (queues) + Reverb (WebSocket, port 6001) + serveur de terminal Node (port 6002), orchestrés par **s6** dans un seul conteneur |
| `coolify-db` | PostgreSQL (configuration et état) |
| `coolify-redis` | Files d'attente et cache |
| `coolify-proxy` | Traefik (par serveur où il est activé) |
| `coolify-sentinel` | Agent Rust de monitoring (optionnel) |

## 4. Stack technique détaillée

| Couche | Choix de Coolify |
|---|---|
| Backend | Laravel 12 (structure de fichiers Laravel 10), PHP 8.4/8.5 |
| UI | **Livewire 3** + Alpine.js + Blade + Tailwind CSS v4 (pas de SPA) |
| Queues | **Laravel Horizon** sur Redis |
| Temps réel | **Laravel Reverb** (WebSocket privés par équipe) |
| Auth | Fortify + Sanctum (API tokens avec abilities read/write/deploy/root), Socialite (Google, Azure, Discord, Authentik, Zitadel, Clerk, Infomaniak...) |
| API | REST `/api/v1`, OpenAPI 3.0 auto-généré, middleware `ApiAbility` + `ApiSensitiveData` (masquage des champs sensibles) |
| SSH | CLI `ssh` via Symfony Process, **multiplexage** (`ControlMaster`, socket dans `/data/coolify/ssh/mux`), gestion sudo non-root (`parseCommandsByLineForSudo`), retry handler |
| Builds | Railpack 0.43.0 (recommandé), Dockerfile, Docker Compose, static ; **Nixpacks déprécié** |
| Reverse proxy | **Traefik par défaut**, mais aussi **Caddy et NGINX** au choix (`ProxyTypes`) |
| Backups | dumps BDD → S3/SFTP/WebDAV via image `rclone` épinglée par digest SHA-256 |
| Monitoring | **Sentinel** (agent Rust, heartbeat + métriques CPU/RAM/disque, push authentifié par token chiffré) |
| Scan d'images | Syft 1.55 (SBOM) + Grype 0.121 (vulnérabilités), épinglés par digest |
| Facturation cloud | Stripe (Coolify Cloud uniquement) |
| MCP | `laravel/mcp` — Coolify expose un serveur MCP |

## 5. Modèle de domaine (81 modèles Eloquent)

- **Ressources** : `Application`, `Service`, `ServiceApplication`, `ServiceDatabase`, `ApplicationPreview` (previews de PR), `ApplicationDeploymentQueue`
- **Bases standalone (9 types)** : PostgreSQL, MySQL, MariaDB, MongoDB, Redis, ClickHouse, KeyDB, Dragonfly, SQLite — chacune son modèle (`StandalonePostgresql`, etc.)
- **Infrastructure** : `Server`, `ServerSetting`, `StandaloneDocker` / `SwarmDocker` (destinations), `PrivateKey`, `CloudProviderToken` (Hetzner...), `GithubRunnerConfig` (runners CI auto-provisionnés !)
- **Backups/Planification** : `ScheduledDatabaseBackup(+Execution)`, `ScheduledVolumeBackup(+Execution)`, `ScheduledTask(+Execution)`, `S3Storage`
- **Sécurité/scan** : `ImageScan`, `ImageSbom`, `ImageScanPolicy`, `ImageScanIgnore`, `ServerHostScan`, `AuditEvent`
- **DNS managé** : `DnsProviderZone`, `ManagedDnsRecord`
- **Organisation** : `Team`, `TeamInvitation`, `Project`, `Environment`, `EnvironmentVariable`, `SharedVariable`, `Tag`
- **Sentinels d'instance** : les lignes système (root team, serveur localhost, BDD `coolify-db`, settings) vivent à **`id = 0`** — convention documentée comme "landmine" PHP/Eloquent (`empty(0)` est true).

## 6. Le moteur de déploiement (`ApplicationDeploymentJob` — 7 483 lignes)

Un seul job Laravel orchestre tout le cycle :

1. **Sélection du serveur de build** (build server dédié possible) + détection des capacités BuildKit.
2. `decide_what_to_do()` : build complet / rebuild / simple restart / skip si l'image du commit existe déjà (`should_skip_build`, fingerprint de build pour Compose).
3. Clone Git (dépôt, ou fichier Compose de la PR pour les previews).
4. Build selon le pack : `deploy_dockerfile_buildpack`, `deploy_railpack_buildpack`, `deploy_nixpacks_buildpack` (déprécié), `deploy_static_buildpack`, `deploy_docker_compose_buildpack`, `deploy_dockerimage_buildpack` (image pré-construite).
5. Gestion des secrets : variables d'env chiffrées en base, **références de secrets distants** (gestionnaires de vault), build secrets BuildKit.
6. Push optionnel vers un registry Docker.
7. Écriture des configurations, labels Traefik/Caddy générés, health checks, bascule zero-downtime, rollback en un clic (`GetRollbackImages`).
8. Post-déploiement : notifications (Discord, Slack, Telegram, Pushover, email, webhook), événements temps réel.

Points d'attention : ce fichier est un **monolithe de 7 483 lignes** — puissant mais difficile à maintenir, et chaque chemin de build a historiquement été une surface d'injection (CVE-2025-64419 via Compose, CVE-2025-64424 via Git).

## 7. Templates one-click

- **386 templates Compose** dans `templates/compose/`, agrégés en `service-templates-latest.json` (Compose encodés en base64), distribués par CDN (`cdn.coollabs.io`), rafraîchis par le job `PullTemplatesFromCDN`.
- Un **parseur de Compose maison** (`serviceParser` / `applicationParser`) avec test golden exhaustif (`ServiceParserReferenceTest` + snapshots) : il réécrit les variables magiques (`$SERVICE_FQDN_...`, `$SERVICE_PASSWORD_...`), génère les labels proxy, les volumes et le réseau.

## 8. Sécurité opérationnelle (côté code)

Pratiques notables lues dans le dépôt :
- `HasSafeStringAttribute`, `SensitiveDataRedactor`, middleware `ApiSensitiveData` : masquage systématique des secrets dans les réponses et logs.
- Politiques d'autorisation (~15 policies) + gates, hiérarchie MEMBER < ADMIN < OWNER, multi-tenant par `team_id`.
- Règle d'architecture documentée : après chargement d'une ressource, **toujours utiliser `resource.team_id`, jamais la team de session** (contre-mesure aux bypass cross-tenant de mai 2026).
- Le **threat model est documenté** dans `SECURITY.md`/`AGENTS.md` : il définit explicitement quelles actions sont "trusted by design" (un admin d'équipe a de facto root sur ses serveurs) et quelles sont les vraies frontières (membre read-only → write, team A → team B, utilisateur → hôte Coolify).

## 9. Forces et faiblesses

**Forces**
- Couverture fonctionnelle inégalée : 9 bases de données, 386 services one-click, previews de PR, runners GitHub auto-provisionnés, scan SBOM/vulnérabilités, DNS managé, transfert de serveur entre instances, backups multi-destinations.
- Résilience : le trafic ne dépend pas du plan de contrôle.
- Apache-2.0 à 100 %, threat model documenté, communauté énorme (~50k étoiles, 500+ contributeurs).
- Multi-proxy (Traefik/Caddy/NGINX), multi-arch, serveurs de build dédiés, support SSH non-root avec sudo.
- Sentinel (Rust) : monitoring léger et découplé.

**Faiblesses**
- **Lourd** : ~80 700 lignes PHP + 530 migrations + job de 7 483 lignes ; RAM/CPU du dashboard non négligeables ; 3 conteneurs de base minimum.
- Historique CVE 2025-2026 chargé (3 × CVSS 10.0 en janv. 2026 ; ~52 890 instances exposées publiquement à l'époque).
- Mises à jour risquées : double-clic sur "update" a déjà exécuté le script en parallèle et détruit des `.env` (données irrécupérables).
- UI Livewire : fonctionnelle mais moins "snappy" qu'une SPA moderne ; nombreux tickets de régressions UI (280+ issues ouvertes en permanence).
- Pas de clustering réel : le mode Swarm est minimal ; pas de HA du plan de contrôle.
- PHP/Laravel impose toute sa chaîne (Composer, Horizon, Reverb, s6) dans un seul conteneur `coolify` — densité de processus élevée.
