# 05 — Blueprint : construire VOTRE PaaS (plus rapide, plus léger, plus complet, plus sécurisé)

Synthèse de tout ce que l'analyse de Dokploy et Coolify permet de recommander pour votre propre plateforme, avec votre branding.

---

## 1. Principes directeurs (hérités des erreurs des deux autres)

1. **Zéro interpolation shell.** Toute exécution passe par des tableaux d'arguments (`execve` sans shell) ou, mieux, par l'**API Docker** (socket/npipe via SDK), jamais par `sh -c "..."`. Ce seul principe élimine la cause de ~70 % des CVE des deux concurrents.
2. **Plan de contrôle découplé du trafic** (le meilleur concept de Coolify) : si votre dashboard tombe, les applications des clients restent en ligne.
3. **Sécurité par construction, pas par patch** : threat model écrit avant le code, secrets chiffrés au repos et write-only, RBAC + scoping tenant systématique et testé.
4. **Un seul binaire / un seul conteneur** côté serveur de contrôle ; agent minimal côté workers.
5. **Licence :** code 100 % vôtre (ou Apache-2.0/MIT) → branding total sans contrainte DSAL.

## 2. Architecture recommandée

```
┌────────────────────────────────────────────────────────────┐
│  PLAN DE CONTRÔLE (1 conteneur, ~50-80 Mo RAM)             │
│  ┌──────────────┐  ┌───────────────┐  ┌─────────────────┐  │
│  │ API + UI     │  │ Moteur de     │  │ PostgreSQL      │  │
│  │ (SPA servie  │  │ déploiement   │  │ (ou SQLite      │  │
│  │  statique)   │  │ (jobs, files) │  │  embarqué v1)   │  │
│  └──────────────┘  └───────────────┘  └─────────────────┘  │
└───────────┬────────────────────────────────────────────────┘
            │ mTLS + gRPC/HTTPS (pas de shell arbitraire !)
            ▼
┌─────────────────────┐   ┌─────────────────────┐
│ AGENT nœud (Rust/Go │   │ AGENT nœud          │
│ ~15 Mo RAM)         │   │                     │
│ API étroite :       │   │                     │
│ deploy/build/stop/  │   │                     │
│ logs/metrics/pull   │   │                     │
└──────────┬──────────┘   └──────────┬──────────┘
           ▼                         ▼
     Docker + Caddy/Traefik    Docker + Caddy/Traefik
     (apps des clients)        (apps des clients)
```

**Différence clé avec les deux concurrents :** ni SSH + chaînes shell (Coolify), ni docker.sock monté dans le conteneur principal (Dokploy). Un **agent à API étroite** : il expose des opérations précises (`deploy(spec)`, `logs(id)`, `metrics()`), jamais un shell. Même compromis, l'attaquant ne peut pas exécuter de commande arbitraire — seulement appeler des opérations validées par schéma.

## 3. Choix de stack recommandés

| Couche | Recommandation | Pourquoi |
|---|---|---|
| Langage serveur | **Go** (ou Rust si vous êtes à l'aise) | Binaire unique, ~20-50 Mo RAM, SDK Docker officiel excellent, concurrence native. C'est le choix "plus rapide, moins lourd" par excellence — Dokploy (Node) et Coolify (PHP) paient tous deux leur runtime |
| UI | SPA légère (React/Svelte + Vite) servie statiquement par le même binaire | Réactivité de Dokploy sans la lourdeur Next.js SSR |
| API | REST + OpenAPI généré, ou tRPC si vous partez en full-TS | Contrat clair = SDK clients gratuits |
| DB | **PostgreSQL** en production ; SQLite embarqué acceptable en v1 mono-utilisateur | Drizzle (TS) ou sqlc (Go) pour le typage |
| Reverse proxy | **Caddy** (config API dynamique native, HTTPS auto) ou Traefik | Le point fort de Traefik (labels) devient inutile si VOTRE plan de contrôle pousse la config par API — le modèle Caddy est plus simple et plus léger |
| Builds | **Railpack** ou BuildKit direct ; Dockerfile et Compose en première classe | Nixpacks est déprécié chez Coolify ; Railpack est le successeur maintenu |
| Auth | Passkeys/WebAuthn + TOTP dès la v1 ; tokens API à abilities (read/write/deploy) | Mieux que les deux, dès le départ |
| Secrets | Chiffrement enveloppe (clé maîtresse locale ou KMS), **champs write-only à l'API**, redaction dans tous les logs | Patterns 2 & 3 du fichier sécurité |
| Temps réel | WebSocket natif (Go : `coder/websocket`) | Pas besoin d'un serveur séparé (Reverb/node-pty) |
| Jobs | File en base (pattern `river` en Go, ou table `jobs` + SKIP LOCKED) | Pas de Redis obligatoire = installation plus légère |

## 4. Fonctionnalités : le noyau qui bat les deux

**Phase 1 — parité cœur (indispensable)**
- Déploiement Git push-to-deploy (webhooks GitHub/GitLab/Gitea/Bitbucket + deploy keys)
- Build packs : Dockerfile, Compose, Railpack, static, image pré-construite
- Apps + 6 bases managées (PostgreSQL, MySQL/MariaDB, Mongo, Redis) ; ClickHouse/KeyDB/Dragonfly/SQLite en phase 2
- Domaines + HTTPS automatique, redirections, basic-auth
- Variables d'environnement chiffrées, write-only, avec diff avant déploiement
- Logs streaming + terminal web (via l'agent, API étroite, jamais de paramètres interpolés)
- Backups planifiés → S3-compatible (rclone ou SDK S3 natif)
- Zero-downtime + rollback en un clic (conserver les N dernières images)
- Multi-serveur via agents ; notifications (Discord/Slack/Telegram/email/webhook)

**Phase 2 — différenciateurs**
- Previews de PR (Coolify le fait, Dokploy aussi — c'est devenu attendu)
- Catalogue de templates one-click (format Compose ouvert ; les 386 de Coolify sont en base64 dans un JSON — le format est trivial à répliquer avec votre branding)
- Métriques CPU/RAM/disque par agent (mini-Sentinel intégré)
- Scan d'images (Grype) en option
- RBAC complet + audit log **inclus gratuitement** (ce que Dokploy met en DSAL)

**Phase 3 — ce qu'aucun des deux n'a**
- HA du plan de contrôle (multi-instance active-active sur PostgreSQL)
- Clustering moderne : nomad/k3s léger plutôt que Swarm (technologie en déclin)
- Fédération : transfert de serveur entre instances (Coolify l'a, c'est excellent pour vos futurs clients qui migrent)
- Assistant IA de diagnostic de déploiement (Dokploy a l'IA, mais pas appliquée au debug de déploiement)

## 5. Check-list sécurité "dès le jour 1"

- [ ] Aucune exécution shell par chaîne interpolée — tableau d'arguments ou API Docker uniquement
- [ ] Agent nœud : API étroite + mTLS + allow-list d'opérations ; pas de socket Docker exposé au plan de contrôle
- [ ] Docker rootless ou Sysbox/gVisor pour les builds multi-tenant
- [ ] Toutes les requêtes DB scopées par `tenant_id` + tests cross-tenant automatisés (403 attendus)
- [ ] Secrets : chiffrés au repos, write-only, redactés des logs ; mot de passe DB d'installation **généré aléatoirement** (l'échec CVE-2026-24840 de Dokploy)
- [ ] Mises à jour : verrou anti-parallélisme (flock) + backup automatique avant upgrade (l'échec Coolify)
- [ ] Rate limiting login, 2FA, host header validation, invite links scopés
- [ ] Dashboard : binding IP privé/Tailscale documenté, allow-list IP optionnelle
- [ ] Threat model écrit et publié (comme Coolify) — il divise par deux le bruit des rapports de sécurité
- [ ] Images épinglées par digest SHA-256 (Coolify le fait pour rclone/syft/grype — excellent réflexe)
- [ ] Pas de télémétrie externe par défaut (opt-in uniquement)

## 6. Estimation de l'effort

| Composant | Complexité | Commentaire |
|---|---|---|
| Agent nœud + protocole | Moyenne | Le cœur différenciant ; ~2-3k lignes Go |
| Moteur de déploiement | **Élevée** | Le job Coolify fait 7,5k lignes pour une raison : PR previews, fingerprints, rollbacks, registries... Prévoyez itératif |
| UI | Élevée | Le plus long en absolu ; une SPA soignée |
| Backups/notifications/proxy | Moyenne | Briques éprouvées, à répliquer proprement |
| Sécurité (RBAC, secrets, tests) | Moyenne mais continue | Budget permanent, pas une phase |

**Conseil de route :** commencez mono-serveur (agent en local sur la même machine que le contrôleur), Docker + Compose + Caddy uniquement, 4 types de build, 4 bases de données. C'est déjà 80 % de la valeur de Dokploy avec une fraction de la surface d'attaque. Le multi-serveur par agent arrive ensuite, sans rien réécrire si le protocole agent est conçu dès le départ.
