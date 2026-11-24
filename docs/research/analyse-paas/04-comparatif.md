# 04 — Comparatif détaillé Dokploy vs Coolify

## 1. Tableau général

| Critère | Dokploy v0.31.4 | Coolify v4.6 |
|---|---|---|
| Langage / framework | TypeScript — Next.js 19, tRPC, Hono | PHP 8.4/8.5 — Laravel 12, Livewire 3 |
| Taille du code | ~65 400 lignes TS/TSX | ~80 700 lignes PHP (+ Blade) |
| Base de données | PostgreSQL (Drizzle ORM) | PostgreSQL (Eloquent, 530 migrations) + Redis |
| Temps réel | WebSocket `ws` + node-pty | Reverb (6001) + terminal Node (6002) |
| Files de tâches | BullMQ + Inngest | Laravel Horizon (Redis) |
| Reverse proxy | Traefik v3.6.7 (HTTP/3) | Traefik, **Caddy ou NGINX au choix** |
| Orchestration | Docker + **Swarm natif** (cluster) | Docker standalone (+ Swarm minimal) |
| Multi-serveur | SSH (lib ssh2) ou Swarm join | **SSH multiplexé, sans agent** (+ Sentinel optionnel en Rust) |
| Builds | Nixpacks, Railpack, Paketo, Dockerfile, Compose, static, drop | Railpack, Dockerfile, Compose, static (Nixpacks déprécié) |
| Bases one-click | 6 (PG, MySQL, MariaDB, Mongo, Redis, LibSQL) | **9** (+ ClickHouse, KeyDB, Dragonfly, SQLite) |
| Templates one-click | Catalogue en croissance (limité) | **386 services** via CDN |
| Previews de PR | Oui (`preview-deployments`) | Oui (`ApplicationPreview`, Compose de PR) |
| Auth | better-auth : org, 2FA, passkey, API key ; **SSO/SCIM propriétaires** | Fortify/Sanctum + Socialite (7 providers OAuth), abilities API |
| Licence | Apache-2.0 + **parties DSAL propriétaires** | **Apache-2.0 intégral** |
| Télémétrie | **PostHog activé** | Pas de télémétrie externe équivalente |
| Extras | Intégration IA multi-providers, sandbox, vault providers | Scan d'images (Syft/Grype), DNS managé, runners GitHub, transfert de serveur entre instances, serveur MCP, Stripe (cloud) |

## 2. Forces comparées

**Dokploy gagne sur :**
- DX développeur contributeur (typage e2e tRPC/Drizzle/Zod, monorepo propre).
- Rythme d'itération UI : Next.js SPA = interface plus réactive que Livewire.
- Clustering : le mode Swarm (réplicas multi-nœuds, load balancing Traefik) est au-delà de ce que Coolify fait nativement.
- Légèreté relative du code (65k vs 80k+ lignes pour un périmètre plus petit).
- Vision produit "next-gen" : IA intégrée, sandbox, vault providers.

**Coolify gagne sur :**
- **Couverture fonctionnelle** : c'est un écosystème complet (backups S3 multi-destinations, planificateurs, scan de vulnérabilités, DNS, runners CI, monitoring Sentinel, transfert inter-instances).
- **Résilience** : découplage plan de contrôle / trafic — le dashboard peut tomber, les apps restent en ligne. Dokploy concentre UI + Traefik + apps sur le même nœud par défaut.
- Maturité opérationnelle : threat model documenté, conventions d'autorisation explicites, tests de non-régression golden pour le parseur Compose, politique de release disciplinée (change files, branches main/next).
- Licence 100 % libre — aucune fonctionnalité derrière un paywall.
- Choix du proxy (Traefik/Caddy/NGINX) et serveurs SSH non-root gérés proprement.

## 3. Faiblesses comparées

| Faiblesse | Dokploy | Coolify |
|---|---|---|
| Injections shell | Pattern structurel (chaînes `exec` partout), 5+ CVE 9.9 | Idem historiquement, désormais largement durci |
| Isolation hôte | docker.sock monté dans le conteneur principal | Idem, mais propose Sysbox |
| Lourdeur | Modérée | **Élevée** (job 7,5k lignes, 530 migrations) |
| HA / clustering | Swarm (technologie en déclin) | Quasi absent |
| Fiabilité des updates | Jeune, à prouver | **Problèmes documentés** (updates parallèles destructrices) |
| Fonctionnalités payantes | SSO/SCIM/audit/whitelabel en DSAL | Aucune |
| Dette UI | Faible | Tickets UI récurrents (Livewire) |

## 4. Performance et empreinte

- **Dokploy** : un conteneur Node + PostgreSQL + Traefik (+ Redis si schedules, + app Go monitoring). Empreinte de base plus faible ; le point chaud est le process Node unique qui fait UI + API + exécutions.
- **Coolify** : 3 conteneurs de base + Horizon (N workers) + Reverb + terminal. Plus gourmand en RAM au repos, mais l'exécution distante est déportée sur les serveurs cibles via SSH — le plan de contrôle ne build pas par défaut.
- **Goulot commun** : les builds (Nixpacks/Railpack) dominent largement tout le reste en CPU/temps, sur les deux.

## 5. Verdict

- Pour **apprendre l'architecture produit** : Coolify est la référence — son découplage contrôle/trafic, son modèle de domaine et ses conventions d'autorisation sont le meilleur matériau pédagogique.
- Pour **apprendre une codebase TS moderne** : Dokploy est plus lisible, plus jeune, plus simple à suivre de bout en bout.
- Pour **forker et rebrand** : Coolify (Apache-2.0 intégral) est légalement plus simple ; Dokploy vous priverait du SSO/whitelabel sauf licence commerciale. Mais aucun des deux ne répond à vos critères "plus rapide, moins lourd, plus sécurisé" sans réécriture profonde — d'où le blueprint du fichier suivant.
