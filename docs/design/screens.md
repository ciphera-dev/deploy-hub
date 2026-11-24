# Écrans — contrat de production

Chaque écran existant est **porté et branché, jamais redessiné**.
- **Source** : la maquette d'origine, dans `prototypes/dashboard/`.
- **Route** : la route TanStack Router dans `web/src/routes/`.
- **Écran porté** : il vit dans `web/src/features/<domaine>/`.
- **Endpoints** : préfixe `/api` (REST Axum), décrits par `crates/api-types` (utoipa → OpenAPI).

**Statuts :**
- `static` : maquette sans données ;
- `mock` : données factices en dur ;
- `porté` : en TSX sur MSW ;
- `branché` : sur l'API réelle.

**Rôles** : `owner` › `admin` › `ops` / `developer` › `viewer`, plus un périmètre par projet.

**Règles transverses :**
- Re-auth + MFA pour les actions sensibles : reset root, révélation d'identifiants, création de jeton, restauration, suppression d'organisation, terminal.
- Secrets toujours en écriture seule.
- Aucun CDN en production : d3-geo, world-atlas, lucide et les drapeaux sont embarqués.

## Shell et transverse

| Écran | Source | Route / vue cible | Statut | Données | Endpoint(s) | Rôle | Action prod |
|---|---|---|---|---|---|---|---|
| Shell (sidebar, header, ⌘B) | `index.html`, `AppSidebar.jsx`, `DashboardHeader.jsx` | `routes/__root.tsx` + `routes/_app.tsx` | branché (Task 12 : /api/me, déconnexion) | nav, badges, utilisateur | `GET /api/me`, SSE `/api/events` | viewer+ | adapter : if/else → routes, localStorage → URL |
| Sélecteur d'espace | `Workspace.jsx` | `features/workspace/WorkspaceSwitcher.tsx` | mock | espaces, logo, couleurs | `GET/POST/PATCH /api/workspaces` | owner/admin | adapter : window.* → contexte |
| Centre de notifications | `Notifications.jsx` | `features/notifications/NotificationsBell.tsx` | mock | notifications lu/non lu | `GET/PATCH /api/notifications`, SSE | tous | brancher |
| 404 | `404.html` | `__root.tsx` › `notFoundComponent` | branché (Task 9) | — | — | public | porter |

## Authentification et installation

| Écran | Source | Route / vue cible | Statut | Données | Endpoint(s) | Rôle | Action prod |
|---|---|---|---|---|---|---|---|
| Connexion (+ mot de passe oublié) | `login.html` | `features/auth/LoginFlow.tsx (routes/_auth/login.tsx)` | branché (Task 12 ; mot de passe oublié Task 16, réinitialisation sur routes/_auth/reset.$token.tsx) | e-mail, mot de passe, se souvenir | `POST /api/auth/login`, `POST /api/auth/password/forgot` | public | porter (login, register et 2fa sont **un seul composant** : à fusionner) |
| Inscription | `register.html` | `routes/_auth/register.tsx` | branché (Task 15 ; fermée par défaut, adresse à confirmer par lien avant la première connexion, voir verify-email.$token.tsx ; acceptation d'invitation sur routes/_auth/invite.$token.tsx) | nom, e-mail, mot de passe | `POST /api/auth/register` | public si activée | porter ; ⚠️ politique d'inscription à décider (ouverte ou invitation seule) |
| Vérification 2FA | `2fa.html` | `routes/_auth/2fa.tsx` | branché | TOTP, passkey, code de secours, appareil de confiance 30 j | `POST /api/auth/mfa/verify`, `POST /api/auth/webauthn/assert/{options,verify}`, `POST /api/auth/recovery-code` | mfa_pending | porter ; remplacer « Simuler la validation » par webauthn-rs |
| Assistant d'installation | `install.html` | `routes/_setup/setup.tsx` | branché (Task 11) | prérequis, admin, organisation, domaine/Tailscale | `GET /api/setup/status`, `POST /api/setup/checks`, `POST /api/setup/complete` | jeton de setup à usage unique | porter ; route verrouillée dès qu'un admin existe |

## Pilotage

| Écran | Source | Route / vue cible | Statut | Données | Endpoint(s) | Rôle | Action prod |
|---|---|---|---|---|---|---|---|
| Vue d'ensemble | `Pages2.jsx#OverviewV2` | `routes/_app/index.tsx` → `features/overview/` | branché (Task 31 ; requêtes, latence p95 et taux d'erreur en état vide, sans source de données en v0.1) | à traiter, KPI, graphe 30 j, déploiements récents, charge serveurs | `GET /api/overview?period=` | viewer+ | adapter (extraire de Pages2) + brancher |
| Déploiements (global) | `Pages2.jsx#DeploymentsPage` | `routes/_app/deployments.tsx` | branché (Task 25) | liste tous projets | `GET /api/deployments` | viewer+ | brancher ; factoriser la ligne avec ProjectDeployments |
| Monitoring | `Pages2.jsx#MonitoringV2` (+ `Monitoring.jsx` pour MultiChart, top conteneurs, stockage) | `routes/_app/monitoring.tsx` | mock | CPU/RAM/disque/trafic, alertes, seuils | `GET /api/metrics?server=&kind=&range=`, `GET/PUT /api/alerts[/rules]`, SSE | viewer+ / ops | fusionner les deux versions puis brancher |

## Projets

| Écran | Source | Route / vue cible | Statut | Données | Endpoint(s) | Rôle | Action prod |
|---|---|---|---|---|---|---|---|
| Liste des projets | `Pages2.jsx#ProjectsListV2` | `routes/_app/projects/index.tsx` | branché (Task 22) | projets, env, statut | `GET /api/projects`, `GET /api/git/providers/:p/repos` | viewer+ | brancher |
| Nouveau projet | `Projects.jsx#NewProjectModal` | `features/projects/NewProjectModal.tsx` | branché (Task 22 ; GitLab, Gitea, Bitbucket et URL Git + clé de déploiement désactivés avec un badge v0.2, décision du 2026-11-23) | source Git/image, build Railpack/Dockerfile/Compose | `POST /api/projects` | developer+ | brancher ; ajouter l'état « échec de build » |
| Détail projet (onglets) | `Projects.jsx#ProjectDetail` | `routes/_app/projects/$projectId.tsx` (+ routes enfants) | branché (Task 22) | en-tête, Visiter/Redémarrer/Redéployer | `GET/PATCH/DELETE /api/projects/:id`, `POST /api/projects/:id/{restart,redeploy}` | périmètre projet | adapter : onglets → routes enfants |
| › Aperçu / Services | `ProjectOverview.jsx` | `$projectId/index.tsx`, `$projectId/services.tsx` | branché (Task 28 ; sparklines vides jusqu'à la Task 31, terminal et réplicas désactivés en v0.2) | prod actuelle, sparklines, services, réplicas | `GET /api/projects/:id/overview`, `/services[/:sid]`, `POST …/services/:sid/{restart,stop,scale}` | developer+ | brancher |
| › Déploiements | `ProjectDeployments.jsx` | `$projectId/deployments.tsx` | branché (Task 25 ; logs live Task 26) | timeline, pipeline 6 étapes, logs live | `GET /api/deployments?project=`, `POST /api/projects/:id/deployments`, `POST /api/deployments/:id/{cancel,rollback}`, WS logs | developer+ | brancher |
| › Logs | `ProjectLogs.jsx` | `$projectId/logs.tsx` | branché (Task 26) | lignes par conteneur, niveaux | `GET /api/projects/:id/logs`, WS `/api/ws/logs` | viewer+ | brancher |
| › Variables | `ProjectVariables.jsx` | `$projectId/variables.tsx` | branché (Task 29) | clés, env, secrets | `GET/PUT /api/projects/:id/variables`, `PUT …/variables/bulk` | developer+ | brancher ; secrets jamais renvoyés |
| › Domaines | `ProjectDomains.jsx` + `ProjectCloudflare.jsx` (garde) | `$projectId/domains.tsx` | branché (Task 30 ; Cloudflare annoncé v0.2) | domaines, DNS, SSL, redirection 308 | `/api/domains…`, `/api/connectors/cloudflare…` | developer+ | brancher ; ACME via le proxy |
| › Git | `ProjectGit.jsx` | `$projectId/git.tsx` | partiel (Task 27 : branche, répertoire racine, chemins surveillés, deploy hooks, redémarrage ; aperçus, commentaires de PR, secret de webhook et livraisons récentes désactivés ; webhook générique Gitea/GitLab en v0.2) | dépôt, branches, webhook HMAC, deploy hooks | `/api/projects/:id/webhook…`, `/deploy-hooks`, `POST /v1/webhooks/:provider/:projectId`, `POST /v1/hooks/deploy/:hookId` | admin / developer+ | brancher |

## Ressources

| Écran | Source | Route / vue cible | Statut | Données | Endpoint(s) | Rôle | Action prod |
|---|---|---|---|---|---|---|---|
| Serveurs (+ clés SSH) | `ServersPage.jsx` | `routes/_app/servers/index.tsx` | mock | cartes, globe, CPU/RAM/disque, agent | `GET /api/servers`, `GET/POST/DELETE /api/ssh-keys` | viewer+ / ops | brancher ; world-atlas embarqué ; génération de clé via WebCrypto (pas `Math.imul`) |
| Ajouter un serveur | `Servers.jsx#AddServerModal` | `features/servers/AddServerModal.tsx` | mock | commande curl + jeton join 15 min | `POST /api/servers/join-tokens`, SSE statut agent | ops/admin | extraire (le reste de `Servers.jsx` est legacy) + brancher ; ajouter l'état « délai dépassé » |
| Gérer un serveur | `ServerManage.jsx` | `routes/_app/servers/$serverId.tsx` | mock | 6 métriques, actions, SSH | `GET/PATCH/DELETE /api/servers/:id`, `POST …/{restart,system-update,agent-update,renew-cert,reset-root-password}` | ops/admin | brancher ; reset root = re-auth + MFA |
| › Pare-feu / Malware | `ServerSecurity.jsx` | `$serverId/security.tsx` | mock | règles nftables, scans, quarantaine | `/api/servers/:id/firewall…`, `/api/servers/:id/malware/…` | ops/admin | brancher via **actions typées** de `crates/proto` |
| › Sauvegardes | `Backups.jsx` | `$serverId/backups.tsx` | mock | destination S3/Supabase, rétention | `/api/servers/:id/backups`, `PUT …/backup-target`, `POST /api/backups/:id/restore` | ops/admin | brancher ; retirer le stockage des clés S3 en localStorage |
| Terminal | `Terminal.jsx` | `features/servers/TerminalModal.tsx` | simulé | session PTY | WS `/api/ws/terminal` (via l'agent) | owner/admin | porter selon l'**ADR 0002** : xterm.js, terminal désactivable, re-auth + MFA, session auditée |
| Conteneurs | `Docker.jsx` | `routes/_app/containers.tsx` | mock | stacks, images, volumes | `/api/servers/:id/{containers,images,volumes}`, `POST …/docker/prune` | viewer+ / ops | brancher (bollard côté agent) |
| Bases de données | `Databases.jsx` | `routes/_app/databases.tsx` | mock | moteurs, stockage, connexions | `/api/databases…`, `GET /api/databases/:id/credentials` | developer+ / ops | brancher ; identifiants = re-auth + audit |
| Domaines (global) | `DomainsPage.jsx` | `routes/_app/domains.tsx` | mock | domaines, zones DNS, SSL | `/api/domains…`, `/api/dns/zones/:zone/records` | developer+ / admin | brancher |

## Plateforme et réglages

| Écran | Source | Route / vue cible | Statut | Données | Endpoint(s) | Rôle | Action prod |
|---|---|---|---|---|---|---|---|
| Sécurité | `Security.jsx` | `routes/_app/security.tsx` | mock | carte d'attaques, flux, règles IP | `GET /api/security/{stats,events}`, `/api/security/ip-rules`, SSE | ops/admin | brancher ; d3 + countries-110m embarqués |
| Connecteurs | `Connectors.jsx#ConnectorsPageV2` | `routes/_app/connectors.tsx` | partiel (carte GitHub, Task 23) | GitHub, Cloudflare, CrowdSec, Supabase, SMTP, OpenAI, Anthropic, Groq, OpenRouter | `/api/connectors…` (OAuth côté serveur) | admin | brancher ; retirer les tokens stockés en localStorage |
| Variables globales | `GlobalVariables.jsx` | `routes/_app/variables.tsx` | branché (Task 29) | portée env × projets, .env | `/api/variables…`, `/import`, `/export` | admin | brancher ; export sans les secrets |
| Équipe | `Team.jsx#MembersTab` | `routes/_app/settings/$section.tsx` (section `team`) | branché (membres, invitations) ; rôles/audit v0.2 | membres, rôle, 2FA, activité, invitations en attente ; onglets Rôles et Journal désactivés | `/api/team/members…`, `/api/team/invitations…` (v0.2 : `/api/team/roles`, `GET /api/audit`) | admin/owner | branché ; lien affiché une seule fois ; actions limitées au rang |
| Clés API / agents | `ApiPage.jsx` | `routes/_app/api-keys.tsx` | mock | jetons à permissions, MCP, CLI | `GET/POST/DELETE /api/tokens` | tous (limité à ses droits) | brancher ; jeton affiché une seule fois |
| Webhooks sortants | `Pages3.jsx#WebhooksPage` | `routes/_app/webhooks.tsx` | mock | URL, événements, livraisons | `/api/webhooks…` | admin | brancher ; favicons sans google.com/s2 |
| Raccourcis | `Pages3.jsx#ShortcutsPage` | `routes/_app/shortcuts.tsx` | static | — | — | tous | porter |
| Documentation | `Pages3.jsx#DocsPage` | `routes/_app/docs.tsx` | ébauche | guides | — | tous | adapter |
| Paramètres | `Pages2.jsx#SettingsV2` (+ `ResourcePage.jsx#SecretModal`) | `routes/_app/settings/$section.tsx` | branché (Task 32 ; SSO, transfert, VPN, Cloudflare et aperçus désactivés en v0.2) | général, domaine du panel + Tailscale, 404, déploiements, notifications, sécurité de l'organisation, zone de danger | `/api/settings[/:section]`, `/api/settings/panel-domain/…`, `/api/settings/tailscale/connect`, `DELETE /api/organization` | admin/owner | découper par section ; **Licence et Plan/factures retirés** (100 % open source) ; SSO libre |
| Profil | `ProfilePage.jsx`, `ProfileSecurity.jsx`, `ProfileNotif.jsx` | `routes/_app/profile.tsx` (+ sécurité, notifications) | branché (Tasks 13, 14, 17 ; e-mails de déploiement et de certificats envoyés depuis le 2026-11-23) | infos, mot de passe, TOTP, passkeys, sessions | `/api/me…` | tous | brancher ; retirer le bloc sécurité redondant de ProfilePage |
| Avatar | `Avatar.jsx` | `features/profile/Avatar.tsx` | mock | upload, initiales | `PUT /api/me/avatar` | tous | adapter : galerie de personnages **retirée** (images supprimées), initiales + upload |
| Tickets | `Tickets.jsx` | `routes/_app/tickets.tsx` | mock | fil, statut, votes | `/api/tickets…` | tous | brancher |
| Ticket assisté par IA | `TicketAI.jsx` | `features/tickets/TicketAICompose.tsx` | mock | brouillon, pièces jointes | `POST /api/tickets/ai-draft` (streaming SSE) | tous (connecteur LLM requis) | adapter ; appel LLM via le serveur, filtrage des secrets, consentement |

## Legacy (gardé dans `prototypes/`, non porté)

`Overview.jsx` (remplacé par OverviewV2) · `Projects.jsx#ProjectsList` · `Servers.jsx#ServersList/ServerDetail` · `ResourcePage.jsx` (sauf SecretModal) · `pages.js` · `tweaks-panel.jsx`, `DashTweaks.jsx` (outillage de l'outil de design) · `vendor/ds-bundle.js` (contient une copie compilée périmée des écrans, écrasée au chargement par Babel).

## Écrans manquants (à dessiner avec les composants et tokens existants)

1. Nouveau mot de passe : cible du lien de réinitialisation, avec les cas jeton expiré ou invalide (`POST /api/auth/password/reset`).
2. Acceptation d'invitation, et première connexion avec mot de passe temporaire (changement forcé).
3. Enrôlement 2FA : QR TOTP, vérification et codes de secours, plus l'enregistrement d'une passkey.
4. ~~Vérification d'e-mail, si l'inscription est ouverte.~~ Fait : `features/auth/VerifyEmail.tsx` (`routes/_auth/verify-email.$token.tsx`), pour l'inscription ouverte et le changement d'adresse (`GET` puis `POST /api/auth/email-verification/{token}[/confirm]`).
5. Onboarding après installation : connecter GitHub, ajouter un premier serveur, créer un premier projet.
6. Erreurs 500, API injoignable et session expirée.
7. Accès refusé (403) et états en lecture seule selon le rôle.
8. États de chargement (skeletons), absents presque partout.
9. Détail d'un déploiement avec une URL partageable (`/deployments/:id`).
10. Journal d'audit global.
11. Modale standard « jeton créé, copiez-le maintenant » (API, deploy hook, secret de webhook). (composant `TokenCreatedModal` créé en Task 27, utilisé pour les deploy hooks)
12. Maintenance et mise à jour de l'instance (version, changelog).
13. Recherche globale ⌘K (le bouton existe mais ne fait rien).
14. Détail d'une alerte et gestion des règles d'alerte.

## Composants à créer au portage (copiés-collés dans les maquettes)

- Menu contextuel « ⋯ » : 7 copies.
- Carte KPI : 8.
- Barre de progression / jauge : 6 + 2.
- Sparkline : 4.
- Bouton copier : 3.
- Avatar : 3.
- Formulaire de connexion : 3.
- Toast : 2.

Ils vont dans `web/src/design-system/components/` :
- `overlay/Menu` ;
- `overlay/Toast` ;
- `data/Tile` ;
- `data/Spark` ;
- `data/Bar` ;
- `core/CopyButton` ;
- `core/Avatar`.

État au 2026-11-21 : tous créés en TSX et exportés par `@/design-system` (Menu, Toast, CopyButton, Avatar : Task 3 ; Tile, Spark, Bar : Task 4). Le skeleton de chargement est fourni par `DataTable loading` (écran manquant 8, partiel).
