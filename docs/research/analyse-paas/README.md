# Analyse complète : Dokploy vs Coolify — Reverse Engineering de deux PaaS open source

**Date de l'analyse :** 18 novembre 2026
**Sources analysées :**
- Dokploy `v0.31.4` (commit `da6282cc`, 17 nov. 2026) — dépôt cloné et analysé dans `../sources/dokploy`
- Coolify `v4.6.x` (commit `10d1968f`, 17 nov. 2026) — dépôt cloné et analysé dans `../sources/coolify`
- CVE/NVD, rapports de sécurité (Aikido, Versa, SolidPoint, Censys), documentation officielle

## Contenu du dossier

| Fichier | Contenu |
|---|---|
| `01-dokploy.md` | Analyse technique complète de Dokploy : architecture, stack, flux de déploiement, modèle de données, multi-serveur, licence |
| `02-coolify.md` | Analyse technique complète de Coolify : architecture, stack, moteur de déploiement, Sentinel, templates, licence |
| `03-securite-et-cve.md` | Toutes les failles connues des deux plateformes, patterns de vulnérabilité, leçons de sécurité |
| `04-comparatif.md` | Comparatif détaillé : forces, faiblesses, fonctionnalités, performance, maintenabilité |
| `05-blueprint-votre-paas.md` | Plan d'architecture recommandé pour construire VOTRE PaaS : plus rapide, plus léger, plus sécurisé |

## Résumé exécutif (30 secondes)

**Dokploy** = TypeScript/Next.js + tRPC + Drizzle/PostgreSQL + Traefik + Docker/Swarm. Moderne, rapide à l'UI, mais jeune : une douzaine de CVE critiques en 2026 (injection de commandes en série, identifiants BDD codés en dur). Cœur Apache-2.0, mais SSO/SCIM/audit-log/rôles custom/whitelabel en licence propriétaire (DSAL).

**Coolify** = PHP/Laravel 12 + Livewire + PostgreSQL/Redis + Traefik + Docker via SSH. Beaucoup plus complet fonctionnellement (386 templates, 9 bases de données, previews de PR, runners GitHub, scan d'images, DNS managé, transfert de serveurs), code mature avec un threat model documenté — mais lourd (job de déploiement de 7 483 lignes, 530 migrations, ~50 conteneurs par déploiement complexe) et un historique de CVE 2025-2026 très chargé (dont 3 × CVSS 10.0).

**Verdict pour votre projet :** aucun des deux n'est à forker tel quel. La bonne stratégie est un **noyau neuf et minimal** (Go ou TypeScript selon votre affinité) qui reprend le meilleur des deux : le plan de contrôle découplé de Coolify, la simplicité d'UX de Dokploy, l'exécution par API Docker plutôt que par shell, et une sécurité conçue dès le départ (aucune interpolation shell, sandboxing, RBAC natif). Le détail est dans `05-blueprint-votre-paas.md`.
