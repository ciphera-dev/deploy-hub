# Journal (append-only)

Ne jamais réécrire ni supprimer une entrée : pour corriger, ajouter une nouvelle entrée.
Chaque entrée indique l'agent qui l'a écrite.

## 2026-11-21 12:37 — Session préproduction — claude-code — Étape 0 : Cadrage
- fait : cadrage — source ciphera-deploy-main, cible Projets/ciphera-deploy, mode complet, stack à recommander, mémoire partagée claude+codex
- résultat : périmètre limité à ciphera-deploy-main (les autres dossiers ciphera de Projets sont hors sujet)

## 2026-11-21 12:37 — Session préproduction — claude-code — Étape 1 : Inventaire
- fait : inventory + scan — 545 fichiers (uploads 271 = 53 %, assets 85, ui_kits 50, components 105)
- résultat : mode fresh, intention dashboard web + API, aucun framework, 36 pages (35 static, 1 wired), 92 doublons, 181 pasted, 1 référence cassée (Avatar.jsx)
- vigilance : avatars = personnages sous droits (GTA, Naruto, Suits, Solo Leveling…), 87 fichiers personal-data?, Lucide via CDN

## 2026-11-21 12:46 — Session préproduction — claude-code — Étape 2 : Sous-agents
- fait : 6 sous-agents (uploads, assets/scraps, code+components, docs-tooling, architecte+intent+stack, écrans+data-contract)
- résultat : intention dashboard + backend (confiance 0.95), React gardé et porté, ~45 écrans réutilisables, 14 écrans manquants, contrat ~90 endpoints, avatars sous droits, PII dans 13 captures + side.png + data.js

## 2026-11-21 12:48 — Session préproduction — claude-code — Étape 3 : Plan de nettoyage
- fait : plan.json fusionné, plan-check 0 erreur
- résultat : 223 gardés/déplacés, 322 supprimés (avatars, PII, uploads, cloud), 0 review

## 2026-11-21 12:51 — Session préproduction — design-to-dev — Étape 4 : Application — installation de la mémoire
- Objectif : rendre le projet reprenable par n'importe quel agent
- Fait : mémoire `.agents/memory/`, graphe `.agents/graph/`, hooks et fichiers d'instructions installés
- Commandes importantes lancées : scaffold_memory.py
- Problèmes : voir problemes.md
- Décisions : voir decisions.md

## 2026-11-21 12:51 — Session préproduction — claude-code — Étape 4 : Application
- fait : apply (234 copiés, 311 supprimés, 297 chemins réécrits), chemins dynamiques corrigés, sections Licence + Plan/factures retirées de Paramètres (Pages2.jsx), SSO sans mention de plan, Efferd → Ciphera (namespace CipheraDesignSystem, CIPHERA_PAGES/NAV, préfixe cph_), identité de l'auteur du template anonymisée (Alex Martin / AM)
- résultat : verify = 1 seul signalement, faux positif (+bg.img+ dans Avatar.jsx) ; galerie d'avatars sans images (supprimées volontairement) ; bundle vendor contient encore une copie compilée périmée des écrans (écrasée par Babel au chargement)

## 2026-11-21 12:51 — Session préproduction — claude-code — Étape 4 : 4b Traces d'IA
- fait : ai_traces clean (dry-run puis réel)
- résultat : 183 fichiers analysés, 0 trace retirable, 0 à vérifier

## 2026-11-21 12:55 — Session préproduction — claude-code — Étape 5 : Fichiers racine
- fait : README.md, .gitignore complété, .env.example (noms seuls), docs/design/screens.md (contrat ~45 écrans + 14 manquants), docs/architecture/stack.md, ADR 0001 structure / 0002 terminal / 0003 installation curl, docs/research/analyse-paas (copie), README DS + oxlint + skill ciphera-design aux nouveaux chemins
- résultat : LICENSE non créé — choix de licence à demander

## 2026-11-21 12:57 — Session préproduction — claude-code — Étape 6 : Fondations et sécurité
- fait : foundations (9 ✅, 15 ⚠️, 0 ❌), scan (4 candidats + 2 à valider au déploiement), requirements dashboard+backend, baseline.md trié, exigences Ciphera ajoutées
- résultat : aucun ❌ ; ⚠️ = bundle maquette, manifestes C2PA dans les SVG de marque (sans danger), commandes curl|sh voulues ; 38 dépendances non vérifiées en ligne

## 2026-11-21 12:59 — Session préproduction — claude-code — Étape 6 : Fondations et sécurité
- fait : fondations relancées après retrait C2PA (10 ✅, 12 ⚠️, 0 ❌), deps --online 0 faille, kit sécurité 9 skills installé, LICENSE AGPL-3.0, baseline.md + exigences.md
- résultat : base saine, aucun ❌ — tâches backend autorisées

## 2026-11-21 13:13 — Session préproduction — claude-code — Étape 7 : Plan superpowers
- fait : plan v0.1 en 35 tâches et 10 phases (Phase 0 fondations : aucun ❌), Review Focus en 5 points, feuille de route v0.2/v0.3
- résultat : docs/superpowers/plans/2026-11-21-ciphera-v0.1.md

## 2026-11-21 13:15 — Session préproduction — claude-code — Étape 8 : Audit Cloudflare
- fait : guidance security-audit sur stack.md, exigences.md et le plan
- résultat : 7 manques de conception confirmés, reportés avec leurs tests (Tasks 12, 19, 24, 25, 30, 31-32, 33) ; lignes génériques sans objet signalées

## 2026-11-21 13:19 — Session préproduction — claude-code — Étape 9 : Clôture
- fait : git init, .gitattributes, commit 71cb2ec, push vers le dépôt privé
- résultat : projet agent-ready, plan prêt, exécution par sous-agents

## 2026-11-21 13:30 — Session v0.1-sdd — claude-code (orchestrateur) — Task 1 : Workspace Cargo et CI
- fait : 8 crates (bins ciphera, ciphera-agent, ciphera-proxy, ciphera-cli), rust-toolchain, deny.toml, justfile, ci.yml (actions épinglées par SHA, actionlint vérifié par SHA-256) ; Task 0 vérifiée (foundations --strict exit 0)
- résultat : commits 8d3ee41, c65e009 ; relecture OK après 1 correction

## 2026-11-21 13:41 — Session v0.1-sdd — claude-code (orchestrateur) — Task 2 : Initialisation de web/
- fait : Vite + React 19 + TS, Tailwind v4 (@theme sur tokens), fontsource Geist (plus de Google Fonts), Vitest, MSW (dev seulement, worker hors dist + contrôle postbuild), Playwright config, components.json shadcn ; versions dans decisions.md
- résultat : commits 464e488, 4a9acca ; relecture OK après 1 correction

## 2026-11-21 13:55 — Session v0.1-sdd — claude-code (orchestrateur) — Task 3 : Composants core, forms, overlay en TSX
- fait : 12 composants portés en TSX (.jsx/.d.ts retirés), Menu, Toast, CopyButton, Avatar, index.ts ; primitives Radix (shadcn) restylées ; Icon via lucide-react (aucune requête) ; web/.oxlintrc.json dérivé + npm run lint
- résultat : commits 348acbd, f90b9e4, 2561350 ; relecture OK après 2 corrections

## 2026-11-21 14:03 — Session v0.1-sdd — claude-code (orchestrateur) — Task 4 : Composants data, navigation, cards en TSX
- fait : 20 composants portés (data, navigation, cards), Tile/Spark/Bar créés, DataTable loading (5 lignes skeleton) et empty ; Tabs natif avec tabindex mobile
- résultat : commits 19d32f1, 0f1a329 ; relecture OK après 1 correction

## 2026-11-21 14:12 — Session v0.1-sdd — claude-code (orchestrateur) — Task 5 : Contrat d'API et génération des clients
- fait : shared (ids Uuid, ApiError + IntoResponse derrière feature axum, sqlx derrière feature sqlx, Config CIPHERA_* avec Debug masqué), api-types (utoipa, /api/health), gen-openapi, openapi.json + schema.d.ts générés, client openapi-fetch à la racine de l'origine, justfile windows-shell, job CI gen-is-fresh. Phase 0 terminée.
- résultat : commits 8b6b9c3, 1e2d753 ; relecture approuvée

## 2026-11-21 14:31 — Session v0.1-sdd — claude-code (orchestrateur) — Task 6 : Plan de contrôle HTTPS, SPA embarquée, santé, en-têtes
- fait : axum + rustls (ring, TLS 1.2/1.3) sur 8443, redirection 301 vers HTTPS (Host validé), SPA rust-embed (503 sans interface), /api/* inconnu en 404 JSON, CSP/HSTS/nosniff/no-referrer partout, migration 0001 (instance, users, sessions, audit_events), harnais de test PG17 (CIPHERA_TEST_DATABASE_URL local, testcontainers en CI)
- résultat : commits 6da1105, 5ebdc4f ; relecture approuvée

## 2026-11-21 14:51 — Session v0.1-sdd — claude-code (orchestrateur) — Task 7 : ciphera init — PKI, secrets d'instance, jeton de setup
- fait : crate pki (CA P-256, certs serveur/agent, séries aléatoires, clés Zeroizing), SecretBox age 0.11 (0.12 écarté : RUSTSEC-2026-0387), ciphera init (fichiers 0600 atomiques sans écrasement, dossiers vérifiés, chown ciphera si root, URL de setup + empreinte SHA-256), AppState.secrets, arrêt SIGTERM
- résultat : commits fd98934, 5f71a3c, e644b95 ; relecture OK après 1 correction

## 2026-11-21 15:22 — Session v0.1-sdd — claude-code (orchestrateur) — Task 8 : install.sh signé, unités systemd, release de test
- fait : install.sh POSIX (minisign + SHA-256 avant toute installation, surcharges de test verrouillées par CIPHERA_INSTALL_TEST=1), unités systemd durcies (test automatique), digests postgres:17 / buildkit, Config DB par champs (CIPHERA_DB_* + fichier de mot de passe), package.sh, release.yml (musl x86_64/aarch64, artefacts seulement), tests e2e Podman systemd Debian/Ubuntu/Fedora 48/48 en local ; redirection HTTP du panneau sur 8080, le port 80 ira au proxy (Task 20)
- résultat : commit f4c6512 ; relecture approuvée

## 2026-11-21 15:37 — Session v0.1-sdd — claude-code (orchestrateur) — Task 9 : Shell de l'application et écrans d'erreur
- fait : routes TanStack (fichiers), AppSidebar/DashboardHeader/404 portés, NAV typée (v0.2/v0.3 désactivées « Bientôt » : Serveurs, Clés API, Domaines globaux en v0.2), écrans 500/hors ligne/session expirée/403, politique d'erreurs (requêtes plein écran, mutations en toast, 401 → /login?expired=1 sauf /api/auth/*, cache vidé), e2e Playwright
- résultat : commits ecc0bc3, 6384575 ; relecture OK après 1 correction

## 2026-11-21 16:01 — Session v0.1-sdd — claude-code (orchestrateur) — Task 11 : Assistant d'installation branché
- fait : GET /api/setup/status, POST /api/setup/checks, POST /api/setup/complete (SERIALIZABLE + INSERT WHERE NOT EXISTS, 410 au rejeu, jeton à temps constant, mort dès qu'un utilisateur existe, fichier supprimé), auth::password argon2id + auth::session (cookie __Host-), migration 0002_setup, audit setup.completed, écran /setup porté (Tailscale masqué)
- résultat : commit 9e328d0 ; relecture approuvée (Review Focus 5 tenu)

## 2026-11-21 16:38 — Session v0.1-sdd — claude-code (orchestrateur) — Task 12 : Connexion, sessions, CSRF, limitation de débit
- fait : POST /api/auth/login (lower(email), hachage leurre, 401 générique), sessions tournantes (cookie __Host-, 12 h / 30 j), CSRF lié à la session (exemptions exactes), limitation atomique 5/15 min par (IP, e-mail) + 50/IP, X-Forwarded-For seulement depuis loopback, /api/me, logout, audit, migration 0003_auth ; LoginFlow porté, shell branché sur /api/me
- résultat : commits 9ff9802, 941d02f ; relecture OK après 1 correction

## 2026-11-21 17:16 — Session v0.1-sdd — claude-code (orchestrateur) — Task 13 : TOTP, codes de secours, enrôlement
- fait : TOTP (totp-rs), 10 codes de secours argon2id, enrôlement forcé /2fa-setup, rotation de session après 2FA, verrou de session contre les validations concurrentes
- résultat : commit d101ce1 ; relecture OK après 1 correction ; commit fait par l'orchestrateur avec accord utilisateur (commit refusé au sous-agent)

## 2026-11-21 19:54 — Session v0.1-sdd — claude-code (orchestrateur) — Task 14 : Passkeys WebAuthn
- fait : webauthn-rs 0.6.2 : enregistrement, suppression avec ré-auth, assertion en 2FA et connexion sans mot de passe (UV requise), cérémonies côté serveur à usage unique, indisponible sur IP ; codes de secours au premier facteur passkey ; verrou par compte sur les facteurs ; exception RUSTSEC-2023-0071 validée par l'utilisateur
- résultat : commits 9c985a4, 0353875 ; relecture OK après 1 correction

## 2026-11-21 20:20 — Session v0.1-sdd — claude-code (orchestrateur) — Task 15 : Rôles fixes, autorisation, invitations
- fait : matrice par défaut de Team.jsx (authz), extracteur Require<R, L> (403 forbidden), invitations à usage unique 72 h (jeton haché), écran d'acceptation /invite/$token, inscription publique fermée par défaut
- résultat : commits cad4cb1, ff3ef0b, a41b1f0 ; relecture OK après 2 corrections

## 2026-11-21 21:01 — Session v0.1-sdd — claude-code (orchestrateur) — Task 16 : SMTP, mot de passe oublié, réinitialisation
- fait : lettre (rustls), réglages SMTP admin avec mot de passe scellé en écriture seule, mot de passe oublié sans énumération (traitement en tâche de fond, délai 60 s), lien 30 min à usage unique qui révoque toutes les sessions, lien admin et CLI ciphera admin reset-password, CIPHERA_PUBLIC_URL, e-mail d'invitation
- résultat : commits a5838f9, d8b7978 ; relecture OK après 1 correction

## 2026-11-21 21:59 — Session v0.1-sdd — claude-code (orchestrateur) — Task 17 : Profil
- fait : GET/PATCH /api/me (langue, fuseau via jiff, e-mail avec ré-auth), avatar ré-encodé PNG 256 avec limites de décodage, changement de mot de passe gardé contre les courses, sessions listées et révocables, préférences e-mail, écrans ProfilePage/ProfileSecurity/ProfileNotif portés ; fin de la phase 2
- résultat : commits 287a2c4, 40796b6 ; relecture OK après 1 correction

## 2026-11-21 22:44 — Session v0.1-sdd — claude-code (orchestrateur) — Task 18 : Protocole v1 et transport QUIC + mTLS
- fait : crates/proto (messages, codec postcard 4 Mo), lien QUIC mTLS plan de contrôle ↔ agent (CA de l'instance seule, empreinte en base, limites de flux et de connexions), registre des agents, enrôlement de l'agent local par ciphera init, purge de /etc/ciphera-agent
- résultat : commits f5d8bfd, 26dc48f ; relecture OK après 1 correction ; e2e d'installation à confirmer en CI

## 2026-11-22 00:16 — Session v0.1-sdd — claude-code (orchestrateur) — Task 19 : Agent : Docker, état local, métriques, conteneurs d'infrastructure
- fait : bollard (étiquettes de propriété), état SQLite, métriques en dernière valeur toutes les 30 s, ciphera-postgres par socket Unix sans TCP, buildkitd rootless sur ciphera-build avec bac à sable, réseaux de projet, attente de la base avec causes ; registre des agents dans AppState ; jobs CI agent-docker et install-docker
- résultat : commits 3b49e68, 004f7e7, 2e8358e ; relecture OK après 2 corrections ; BLOQUANT avant Task 24 : uid dédié pour buildkitd

## 2026-11-22 03:23 — Session v0.1-sdd — claude-code (orchestrateur) — Task 20 : Proxy : table de routes pilotée par l'agent
- fait : ciphera-proxy Pingora (rustls) sur 80/443, table de routes atomique (ArcSwap) appliquée par l'agent via socket Unix (activation systemd), redirection 301 vers :8443 pour les hôtes IP, 404 sobre, en-têtes et chemins validés, plafonds de connexions (global, par IP, attentes TLS à part), journaux sans jetons ; côté plan de contrôle : IPv6 /64 et espace de débit séparé pour mot de passe oublié ; fin de la phase 3
- résultat : commits d8c524a, 78ce6db, 4e35de3 ; relecture OK après 2 corrections

## 2026-11-22 04:49 — Session v0.1-sdd — claude-code (orchestrateur) — Task 21 : File de tâches PostgreSQL
- fait : File SKIP LOCKED (migration 0010), bail de 5 min + heartbeat, dédup par index unique partiel, travailleur à concurrence bornée lancé par serve (commit 4229a9a)
- résultat : Relecture OK sans correction ; 11/11 tests jobs ; suite complète à confirmer en CI. Report Task 25 : arrêter le handler si le bail est perdu

## 2026-11-22 05:18 — Session v0.1-sdd — claude-code (orchestrateur) — Task 22 : Projets, environnements, services
- fait : Migration 0011, API /api/projects (CRUD, authz Read/Write/Admin, validation stricte), écrans liste/détail/nouveau projet portés (commit 98e9d4d)
- résultat : Relecture OK sans correction ; 8/8 tests projects + 176 tests web. Éléments de maquette sans backend reportés (sélecteur de dépôt T23, auto-deploy/animation T25, variables T29, colonne Domaine T30/31)

## 2026-11-22 06:07 — Session v0.1-sdd — claude-code (orchestrateur) — Task 23 : GitHub App par instance et webhooks entrants
- fait : Flux manifest (state lié à la session, usage unique), clé et secret scellés en écriture seule, JWT RS256 par ring, jeton d'installation en mémoire, webhook HMAC + anti-rejeu 24 h + refus des push périmés, carte GitHub et sélecteur de dépôt portés (commits 9f8824c, 6fe4997)
- résultat : Relecture : 1 Important corrigé (rejeu après purge) ; 19 tests github, 266 Rust, 182 web. rsa en dev-dependency pour clés de test (exception élargie aux tests, à signaler)

## 2026-11-22 08:15 — Session v0.1-sdd — claude-code (orchestrateur) — Task 24a : buildkitd sous un compte système dédié (prérequis Task 24)
- fait : Compte système ciphera-build + plage de sous-uids/gids dédiée sans collision, buildkitd lancé sous cet uid avec SETUID/SETGID seulement, état dans /var/lib/ciphera-buildkit, agent dégradé sans buildkitd (commits c2c6654, e4fe2eb, 790c387)
- résultat : CI réelle : uid de buildkitd = ciphera-build (ni 1000 ni root), build OK, install-e2e OK sur 3 distributions. BLOQUANT levé

## 2026-11-22 10:41 — Session v0.1-sdd — claude-code (orchestrateur) — Task 24 : Pipeline de build (clone, Railpack, Dockerfile, Compose)
- fait : Build sans shell : ValidatedArg, git durci (askpass, https seul), compose_policy, railpack prepare dans un conteneur isolé (réseau ciphera-prepare, mise épinglé), buildctl -> Docker, nettoyage du répertoire ; railpack/buildctl/mise épinglés par SHA-256 dans install.sh (commits 787ec52, b44cd1a, 2d91a4a)
- résultat : CI réelle : builds des 3 fixtures et prepare isolé OK ; install-e2e OK sur 3 distributions. Reports : réseau compose et promotion d'image (T25), filtrage de sortie (T34), outils dans l'archive signée (T35)

## 2026-11-22 12:37 — Session v0.1-sdd — claude-code (orchestrateur) — Task 25a : Agent : déploiement sans interruption et annulation
- fait : Démarrage sur ciphera-p-<projet>, healthcheck 5×5 s, bascule ApplyRoutes puis drainage 10 s, échec sans toucher l'ancien, Compose sur le réseau du projet, annulation durable, issues persistées (state.db v3), balayage au démarrage par étiquettes, protocole v2 (commits 5261e02, 8f40b2d, 937c547)
- résultat : CI réelle : v2 en échec garde v1, v3 saine bascule et arrête v1 ; Compose forcé sur le réseau du projet ; mise épinglé prouvé

## 2026-11-22 14:05 — Session v0.1-sdd — claude-code (orchestrateur) — Task 25b : Plan de contrôle des déploiements
- fait : Migration 0013, orchestrateur (job Deploy) avec verrou par environnement, jeton GitHub au moment de l'envoi, annulation, rollback sans rebuild, rétention 5 + Cleanup, chien de garde 10 min (échec présumé), réconciliation depuis les issues de l'agent, API /api/deployments (commits 3ab597a, a640c89)
- résultat : Relecture : 1 Important corrigé ; 18 tests deployments, 262 au total ; CI verte

## 2026-11-22 14:28 — Session v0.1-sdd — claude-code (orchestrateur) — Task 25c : Écrans des déploiements
- fait : Liste globale, onglet Déploiements du projet (timeline 6 étapes), détail partageable /deployments/:id, Redéployer avec garde serveur et verrou au clic, annulation/rollback selon statut et rôle (commits 5cb2e86, 2016f5a, 0db2736)
- résultat : Relecture : 2 tours de corrections ; 219 tests web

## 2026-11-22 16:36 — Session v0.1-sdd — claude-code (orchestrateur) — Task 26 : Logs en direct (build et exécution)
- fait : WS /api/ws/logs (session + Origin exacte, Logs Read), StopLogs dans proto, compteur d'abonnés, tampon 1024 lignes par client, masquage des secrets (multi-lignes, cache 10 s invalidable), canal dédié aux logs, migration 0014 (10 000 dernières lignes), logs.txt 4 Mio, LogViewer porté depuis ProjectLogs.jsx avec reconnexion ; dépôt déplacé vers ciphera-dev/deploy-hub
- résultat : TERMINÉ (relecture : 1 tour de corrections ; 445+ tests cargo, 234 tests web)

## 2026-11-22 18:10 — Session v0.1-sdd — claude-code (orchestrateur) — Task 27 : Git du projet, deploy hooks, redémarrage
- fait : GET/PATCH /api/projects/:id/git (branche, répertoire racine, chemins surveillés, déclencheurs, auto_deploy), deploy hooks dh_ (haché, affiché une fois, 404 identique à temps constant, débit limité) via le chemin de redéploiement de la Task 25, chemins surveillés appliqués au webhook push, redémarrage (nouvelle variante proto), répertoire racine appliqué au build (BuildSpec.root_directory), ProjectGit.jsx porté, TokenCreatedModal ; CI : espace disque libéré sur les runners
- résultat : TERMINÉ (relecture : 1 tour de corrections ; 473 tests cargo, 239 tests web)

## 2026-11-22 19:30 — Session v0.1-sdd — claude-code (orchestrateur) — Task 28 : Aperçu et services du projet
- fait : GET /api/projects/:id/overview et /services, redémarrage et arrêt par service (variante proto StopContainers, DeploySpec.service_name), alias réseau <service>.internal, statut du projet (servi → en ligne), ProjectOverview.jsx porté (aperçu + services), terminal et réplicas désactivés (v0.2), sparklines vides jusqu'à la Task 31
- résultat : TERMINÉ (relecture propre ; 483 tests cargo, 247 tests web)

## 2026-11-22 21:34 — Session v0.1-sdd — claude-code (orchestrateur) — Task 29 : Variables du projet et globales (age, écriture seule)
- fait : Variables chiffrées (age, clé de l'instance), secrets jamais renvoyés, export sans secrets, priorité projet > global, bannière « non appliquées » (suppressions incluses, verrou de résolution), injection dans DeploySpec.env (Compose compris, valeurs jamais sur disque via CIPHERA_VAR_*), masquage des logs branché et fermé en cas d'erreur, ProjectVariables.jsx et GlobalVariables.jsx portés
- résultat : TERMINÉ (relecture : 2 tours de corrections ; 461+ tests cargo, 260 tests web)

## 2026-11-23 00:29 — Session v0.1-sdd — claude-code (orchestrateur) — Task 30 : ACME et domaines
- fait : Certificats ACME HTTP-01 émis et gardés chiffrés par le proxy (instant-acme, Pebble en CI), domaines par projet (hôte unique dans l'instance, panneau réservé, sslip.io/nip.io refusés), vérification DNS A/AAAA stricte, émission limitée (un job par domaine, pas de réémission d'un certificat valide, DNS revérifié, reprise 5 min → 24 h), redirection 308 et 80→443 hors défi ACME, internal_port requis, clés supprimées seulement avec le domaine, ProjectDomains.jsx porté ; profil de dev allégé (target 244 Gio → 5,5 Gio)
- résultat : TERMINÉ (relecture : 2 tours de corrections)

## 2026-11-23 02:11 — Session v0.1-sdd — claude-code (orchestrateur) — Task 31 : Domaine du panneau, métriques agrégées, Vue d'ensemble
- fait : Métriques de l'agent (CPU/RAM par conteneur, santé) brutes 24 h et agrégats horaires 30 j (MetricsRollup), sparklines du projet et barre latérale des logs alimentées, statut du projet tenant compte de la santé, Vue d'ensemble (OverviewV2 porté), domaine du panneau (owner, DNS + ACME, route panel servie seulement avec certificat, 8443 gardé jusqu'à confirmation depuis le domaine, échéance 24 h, webhook GitHub migré, Setup URL à mettre à jour à la main)
- résultat : TERMINÉ (relecture : 1 tour de corrections ; 563 tests cargo, 273 tests web)

## 2026-11-23 09:42 — Session v0.1-sdd — claude-code (orchestrateur) — Task 32 : Paramètres (sections v0.1)
- fait : Sections Général, Domaine du panneau (bascule, confirmation, retrait avec retour du webhook GitHub vers 8443), Déploiements, Sécurité de l'organisation (mfa_required à la prochaine connexion, liste d'IP autorisées avec garde-fou anti-blocage et commande clear-ip-allowlist, routes de webhooks et ACME exemptées), Notifications montée, export JSON sans secrets, suppression de l'organisation selon la décision utilisateur du 2026-11-23 (contenu effacé, propriétaire et configuration gardés, projets restants listés), flux de logs coupés à la révocation
- résultat : TERMINÉ (relecture : 1 petit tour ; 583 tests cargo, 280 tests web)

## 2026-11-23 11:28 — Session v0.1-sdd — claude-code (orchestrateur) — Task 33 : Mises à jour de Ciphera
- fait : Mises à jour signées (minisign + SHA-256, version strictement supérieure, --allow-downgrade root avec --restore-db vérifié), demande HMAC écrite par le panneau (owner + ré-auth), sauvegarde pg_dump vérifiée avec métadonnées, remplacement par renommage, retour arrière binaires + base, reprise au démarrage non bloquante, configuration root isolée (/etc/ciphera-updater), programmes lancés par chemin absolu sans environnement, outils tiers mis à jour depuis le manifeste signé, PROTO_VERSION 3 avec refus clair à la poignée de main
- résultat : TERMINÉ sous réserve de la CI (relecture : 3 tours de corrections ; update.sh 42/42 en local)

## 2026-11-23 13:15 — Session v0.1-sdd — claude-code (orchestrateur) — Task 34a : Matrice d'autorisation, fuzzing du codec, threat model
- fait : authz_matrix.rs (tous les endpoints x 5 rôles + sans session), cible cargo-fuzz decode + job CI fuzz-proto, docs/security/threat-model.md
- résultat : relecture propre ; aucune incohérence d'autorisation ; 6 M exécutions de fuzzing sans plantage

## 2026-11-23 14:49 — Session v0.1-sdd — claude-code (orchestrateur) — Task 34b : Parcours complet e2e en CI
- fait : web/e2e/golden-path.spec.ts + job CI e2e-golden-path (installation réelle, faux GitHub, Pebble) ; correction : repli de CIPHERA_PUBLIC_IP sur l'IP du certificat du panneau (jamais implémenté)
- résultat : CI verte, parcours complet setup → TOTP → GitHub → déploiement → HTTPS → rollback

## 2026-11-23 16:56 — Session v0.1-sdd — claude-code (orchestrateur) — Task 34c-1 : Filtrage réseau des builds et durcissement du proxy
- fait : table nftables inet ciphera_egress (builds et prepare sans accès aux métadonnées cloud, à l'hôte ni aux réseaux privés), délai total de lecture des en-têtes, éviction de la plus ancienne poignée TLS en attente, contrôle SNI/Host (421), vérification clé/SAN des certificats, contrôle du dossier d'état, prérequis nft vérifié par le updater
- résultat : relecture propre après 2 tours ; CI et Release (test) vertes ; résiduels notés dans problemes.md (DNS 53 ouvert, réponses certificat appariées par hôte)

## 2026-11-23 19:53 — Session v0.1-sdd — claude-code (orchestrateur) — Task 34c-2 : Durcissement du plan de contrôle, de l'agent et des mises à jour
- fait : chiffrement age lié à la ligne (migration), logs et métriques liés au serveur, limites de débit par catégorie avec éviction, resynchronisation d'état avec anti-rebond à front descendant, AppShell robuste, updater (corps borné, verrou 0600, restauration atomique), PROTO_VERSION 4, foundations 0 ❌
- résultat : relecture propre après 2 tours ; CI et Release (test) vertes ; résiduels R8 (secrets GitHub/TOTP/SMTP non liés), ordre des lignes rejouées, compteurs remis à zéro au redémarrage

## 2026-11-24 01:18 — Session v0.1-sdd — claude-code (orchestrateur) — Task N : E-mails de notification et sources Git hors GitHub en v0.2 (décisions utilisateur)
- fait : Relecture finale (4 zones) et une vague de corrections : cycle systemd au démarrage, renouvellement des certificats, updater piloté par l'archive avec registre des unités, santé réelle avant succès, réinstallation sûre, commandes admin, limites du panneau, rotation des logs, routes persistées, 2FA après rechargement ; e-mails de déploiement et de certificats (file de tâches, idempotence, 2 budgets de 10/h, récapitulatif) ; sources Git hors GitHub déplacées en v0.2 dans le spec
- résultat : relectures propres ; CI et Release (test) vertes jusqu'à 77fd3ca ; résiduels dans problemes.md

## 2026-11-24 03:26 — Session v0.1-sdd — claude-code (orchestrateur) — Task 35 : Préparation de la publication (signature de production, cosign, SLSA, site get.ciphera[.]dev)
- fait : release.yml signé et approuvé (environnements release et github-pages), script de clé scripts/release/new-signing-key.ps1, docs/operations/release.md et docs/security/release-signing.md ; relecture propre après 2 tours
- résultat : CI, CodeQL et Release (test) verts sur 38b4e2b ; mineurs reportés dans le registre SDD

## 2026-11-24 12:22 — Session v0.1-sdd — claude-code (orchestrateur) — Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md
- fait : domaine passé à get.ciphera.ch ; clé publique de production poussée ; 62 alertes CodeQL fermées, #63 corrigée ; signalement privé activé ; SECURITY.md, CONTRIBUTING.md
- résultat : relectures propres ; CI à confirmer sur le dernier commit

## 2026-11-24 — Session remediation audit — claude-code (implementeur) — Task R1 : Protocole v6
- fait : PROTO_VERSION 6 ; RemoveProject, DeploymentRemoved, ProjectRemoved (variantes en dernier) ; champs de fin BuildSpec.project_id/environment_id, DeploySpec.environment_id/run_as/read_only_rootfs, RunningDeployment.runs_as_root ; visiteur strict (champ absent seulement a la frontiere exacte, coupe au milieu d'un champ ou octets en trop = erreur) ; plan de controle remplit les identifiants, journalise les accuses ; agent ignore RemoveProject
- resultat : cargo fmt, clippy, tests proto/agent verts ; tests control-plane (Docker) en CI

## 2026-11-24 14:48 — Session v0.1-sdd — claude-code (orchestrateur) — Task R1 : Remédiation audit — protocole v6
- fait : PROTO_VERSION 6 ; variantes et champs de fin pour les tâches suivantes ; charges tronquées refusées
- résultat : relecture propre après 1 tour ; CI à confirmer

## 2026-11-24 15:21 — Session v0.1-sdd — claude-code (orchestrateur) — Task R2 : Remédiation audit — cloisonnement des caches de build
- fait : espace de cache par environnement, frontends limités, configuration buildkitd
- résultat : relecture propre après 1 tour ; job agent-docker à confirmer

## 2026-11-24 — Session remediation audit — claude-code (implementeur) — Task R3 : buildkitd, builds en serie, clone borne
- fait : buildkitd 4096 processus / 50 % RAM / CPU-1 (ContainerSpec.memory, nano_cpus) ; gckeepstorage (10 % disque, 5-50 Gio) dans buildkitd.toml ; semaphore de builds (1 par noeud, build::BUILD_SLOTS) ; image chargee plafonnee a 10 Gio ; groupe de processus tue au delai ; clone plafonne a 2 Gio (clone::exec_capped) ; AppArmor inchange (repli documente, R13)
- resultat : fmt, clippy, tests agent verts sous Windows ; tests Linux (agent-docker : builds_are_serialized, build_timeout_kills_children, clone_over_cap_fails, limites dans docker_it) a confirmer en CI
## 2026-11-24 16:22 — Session v0.1-sdd — claude-code (orchestrateur) — Task R3 : Remédiation audit — ressources du build
- fait : limites buildkitd, builds en série, plafonds d'image et de clone, groupe de processus
- résultat : relecture propre après 1 tour ; agent-docker à confirmer

## 2026-11-24 16:43 — Session v0.1-sdd — claude-code (orchestrateur) — Task R9 : Remédiation audit — Compose : redémarrage et étiquettes réservées
- fait : portée du redémarrage par projet, étiquettes réservées posées par l'agent
- résultat : relecture propre ; agent-docker à confirmer

## 2026-11-24 17:06 — Session v0.1-sdd — claude-code (orchestrateur) — Task R8 : Remédiation audit — proxy : échéance d'en-têtes par requête, plafonds de connexions, unité bornée
- fait : rebasé sur 31b0ef3, intégré (5272ae5, 68e14da)
- résultat : relecture approuvée (6 mineurs, consignés) ; tests proxy locaux sous podman verts

## 2026-11-24 17:13 — Session v0.1-sdd — claude-code (orchestrateur) — Task R10 : Remédiation audit — limites des services Compose
- fait : rebasé sur 289aeca, intégré (cf7675a, b604c6b)
- résultat : relecture : 1 Important corrigé (tmpfs de taille nulle), relecture ciblée approuvée

## 2026-11-24 17:22 — Session v0.1-sdd — claude-code (orchestrateur) — Task R5 : Remédiation audit — gestion de l'équipe (API)
- fait : rebasé sur bc6d067, intégré (c0bc89e, f3c6219)
- résultat : relecture : 1 Important corrigé (invitations du membre retiré), lien de réinitialisation borné par can_manage ; relecture ciblée approuvée

## 2026-11-24 18:32 — Session v0.1-sdd — claude-code (orchestrateur) — Task R6 : Remédiation audit — écran Équipe
- fait : intégré (277155a)
- résultat : relecture approuvée (écart accepté : nombre de hooks annoncé après le retrait) ; vitest 297 verts

## 2026-11-24 18:58 — Session v0.1-sdd — claude-code (orchestrateur) — Task R45 : Remédiation audit — un état BuildKit par projet
- fait : rebasé sur a3cfdd2, intégré (b37298e, ffe8491), tests de l'agent verts après rebase
- résultat : relecture : 1 Important corrigé (outil d'installation sorti du binaire), relecture ciblée approuvée

## 2026-11-24 20:04 — Session v0.1-sdd — claude-code (orchestrateur) — Task fixci : CI rouge après R45 et R6
- fait : corrigé (a30be27, 714aa39) : socket de test trop long, shellcheck, clippy, test Linux, sélecteur e2e
- résultat : reproduit sous Podman ; relectures approuvées

## 2026-11-24 20:17 — Session v0.1-sdd — claude-code (orchestrateur) — Task R7 : Remédiation audit — ré-authentification avant un second facteur
- fait : rebasé sur 97c89dc, intégré (bace1a6, 8812dc0)
- résultat : relecture : 1 Important corrigé (session d'enrôlement périmée), relecture ciblée approuvée

## 2026-11-24 20:48 — Session v0.1-sdd — claude-code (orchestrateur) — Task R3b : Profil AppArmor appliqué au buildkitd
- fait : intégré (fc3f797, ffb435e, 68293ee)
- résultat : relectures : 1 Important corrigé (variante AppArmor 3), rebase vérifié ; preuve d'exécution en CI

## 2026-11-24 20:54 — Session v0.1-sdd — claude-code (orchestrateur) — Task R16 : Remédiation audit — secrets liés à leur ligne, contrôle d'age.key
- fait : rebasé sur 4269329, intégré, compilation vérifiée après rebase
- résultat : relecture : 1 Critique corrigé (clé lue par l'updater root), relecture ciblée approuvée

## 2026-11-24 21:06 — Session v0.1-sdd — claude-code (orchestrateur) — Task R20 : Remédiation audit — anti-rejeu des webhooks sur l'empreinte du corps
- fait : Empreinte du corps des push GitHub stockée 24 h 5 min, doublon refusé ; tests à corps unique ; correctif actionlint du job AppArmor
- résultat : relecture : 1 Important corrigé, relecture ciblée approuvée

## 2026-11-24 21:23 — Session v0.1-sdd — claude-code (orchestrateur) — Task R11 : Remédiation audit — domaine du panneau, DNS et bascule sûre
- fait : Verdicts DNS négatifs comptés en transaction, bascule du panneau résistante à l'annulation, recheck limité, cert_error effacé au recheck
- résultat : relecture : 1 Important corrigé, relecture ciblée approuvée, tests DB panel_domain 12/12 et domains 23/23

## 2026-11-24 21:27 — Session v0.1-sdd — claude-code (orchestrateur) — Task R18 : Remédiation audit — secrets masqués dans les journaux avant toute coupe
- fait : Rassemblage des fragments Docker à 64 Kio, nettoyage puis masquage sur tous les chemins, coupe après masquage, lignes retenues bornées en octets
- résultat : relecture : 2 tours de correctifs, relecture ciblée approuvée ; tests Docker et DB en CI

## 2026-11-24 23:01 — Session v0.1-sdd — claude-code (orchestrateur) — Task fix-ci-zombie : CI : buildkitd zombie à la désinstallation
- fait : Arrêt propre avant suppression, init en PID 1, essais bornés, uninstall sort en erreur si des conteneurs restent, avertissement docker-init
- résultat : relecture approuvée après un tour

## 2026-11-24 23:10 — Session v0.1-sdd — claude-code (orchestrateur) — Task R13 : Remédiation audit — limites du second facteur, de la ré-authentification et de la connexion
- fait : Compteurs par compte et IP, plafond quotidien TOTP, un seul calcul Argon2 par compte sans blocage du titulaire, migration 0040
- résultat : relecture : 3 Importants corrigés en 2 tours, approuvée

## 2026-11-24 23:26 — Session v0.1-sdd — claude-code (orchestrateur) — Task R46 : Remédiation audit — utilisateur et racine en lecture seule des conteneurs
- fait : run_as et read_only_rootfs par service, migration 0039, option sans root à la création, badge root rafraîchi après déploiement, capacités réduites
- résultat : relecture : 3 Importants puis 1, corrigés en 2 tours, approuvée

## 2026-11-24 23:30 — Session v0.1-sdd — claude-code (orchestrateur) — Task R12 : Remédiation audit — 2FA imposée aux sessions ouvertes, sécurité réservée au propriétaire
- fait : Sessions ouvertes basculées en enrôlement, mot de passe toujours exigé pour un facteur, réglages de sécurité réservés au propriétaire
- résultat : relecture : 1 Important corrigé, approuvée ; vérifiée avec R13

## 2026-11-25 00:55 — Session v0.1-sdd — claude-code (orchestrateur) — Task R15 : Remédiation audit — vérification de l'adresse e-mail
- fait : Changement d'adresse et inscription ouverte confirmés par lien, mot de passe choisi à la confirmation, comptes en attente encadrés et purgés, délai mesuré sur les demandes
- résultat : relecture : 4 Importants corrigés en 3 tours, approuvée

## 2026-11-25 01:08 — Session v0.1-sdd — claude-code (orchestrateur) — Task R14 : Remédiation audit — origine des liens de réinitialisation, remise à zéro du second facteur
- fait : Origine des liens côté serveur, délai limité aux liens forgot, reset-mfa réservé aux gestionnaires avec mot de passe et CLI
- résultat : relecture : 2 Importants corrigés, approuvée

## 2026-11-25 01:12 — Session v0.1-sdd — claude-code (orchestrateur) — Task R21 : Remédiation audit — GitHub, dépôt identifié par son id, jeton restreint
- fait : Id du dépôt retenu, pas de repli anonyme, jeton limité au dépôt et en lecture, réponses bornées
- résultat : relecture approuvée, finitions relues

## 2026-11-25 01:14 — Session v0.1-sdd — claude-code (orchestrateur) — Task R17 : Remédiation audit — variables, import .env, masquage multi-motifs
- fait : Automate aho-corasick linéaire hors verrou, formes encodées et nettoyées masquées, import .env, plafonds sous verrou par projet
- résultat : relecture : 1 Important corrigé, approuvée

## 2026-11-25 02:28 — Session v0.1-sdd — claude-code (orchestrateur) — Task R23 : Remédiation audit — retrait complet d'un projet supprimé sur ses serveurs
- fait : Retrait par serveur jusqu'à l'accusé, purge des volumes, BuildKit du projet retiré, état gardé si Docker échoue
- résultat : TERMINÉ (relecture : 1 Important corrigé, approuvée)

## 2026-11-25 02:48 — Session v0.1-sdd — claude-code (orchestrateur) — Task R22 : Remédiation audit — nettoyage des déploiements confirmé par l'agent
- fait : Accusé de l'agent avant cleaned_at, renvois, rapprochement, balayage prudent des restes
- résultat : TERMINÉ (relecture : 1 Important corrigé, approuvée)

## 2026-11-25 03:03 — Session v0.1-sdd — claude-code (orchestrateur) — Task R19 : Remédiation audit — quotas des flux de journaux et bornes mémoire
- fait : Quota par compte, coupe et débit par agent avant la diffusion, origine du build
- résultat : TERMINÉ (relecture : 2 Importants corrigés, approuvée)

## 2026-11-25 03:33 — Session v0.1-sdd — claude-code (orchestrateur) — Task R24 : Remédiation audit — ordre des services Compose autour de la sonde, annulation tardive
- fait : Ordre en trois temps, retrait des services nouveaux, notes en état local v6 avec garde « servi », annulation relue avant l'envoi
- résultat : TERMINÉ (relecture : 1 Important corrigé, approuvée)

## 2026-11-25 04:18 — Session v0.1-sdd — claude-code (orchestrateur) — Task R55 : Remédiation audit — restauration des services Compose dépendants
- fait : Configuration relue en mémoire, recréation à l'identique en échec ou annulation, issue explicite si incomplète, verrou et garde par projet
- résultat : TERMINÉ (relecture : 2 Importants corrigés, approuvée)

## 2026-11-25 04:29 — Session v0.1-sdd — claude-code (orchestrateur) — Task R25 : Remédiation audit — file de tâches bornée, déploiements jamais orphelins
- fait : Notifications plafonnées, priorités, plafond de 10 en file, échecs GitHub et tâches mortes terminés, purge, déduplication, ordres après COMMIT
- résultat : TERMINÉ (relecture : 1 Important corrigé, approuvée)

## 2026-11-25 05:28 — Session v0.1-sdd — claude-code (orchestrateur) — Task R26 : Remédiation audit — images internes sous un registre non résoluble
- fait : ciphera.invalid/, références internes et par identifiant refusées, image Compose liée à son build, tirages bornés
- résultat : TERMINÉ (relecture : 1 Important corrigé, approuvée)

## 2026-11-25 05:32 — Session v0.1-sdd — claude-code (orchestrateur) — Task R27 : Remédiation audit — bac à sable de railpack prepare, askpass, .env de Compose
- fait : Sortie de prepare par l'API d'archive sans dossier de l'hôte, askpass strict, compose config sans .env ni COMPOSE_*
- résultat : TERMINÉ (relecture : 2 Importants corrigés, approuvée)

## 2026-11-25 05:44 — Session v0.1-sdd — claude-code (orchestrateur) — Task fix/R55-ci : CI : test d'annulation Compose de R55
- fait : Nombre d'essais de sonde admis, attente de l'étape signalée par l'agent
- résultat : TERMINÉ (relecture approuvée)

## 2026-11-25 07:28 — Session v0.1-sdd — claude-code (orchestrateur) — Task fix/ci-loglines : Journaux d'agent : aucune ligne partielle, aucun morceau de secret
- fait : Test corrigé ; ligne partielle jamais envoyée, première ligne de toute lecture remplacée, TTY sans horodatage Docker
- résultat : TERMINÉ (relecture approuvée, 4 tours)

## 2026-11-25 07:42 — Session v0.1-sdd — claude-code (orchestrateur) — Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent
- fait : Pont cphp par projet, table nftables générée en Rust et rechargée toutes les 5 min, DNS vers les résolveurs de l'hôte, métadonnées rejetées, unité de l'agent durcie
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 07:52 — Session v0.1-sdd — claude-code (orchestrateur) — Task R29 : Hôtes du panneau, X-Forwarded-For, RP ID des passkeys
- fait : 421 aux hôtes inconnus sur 8443, X-Forwarded-For seulement via le proxy local, noms réservés refusés aux domaines, passkeys à l'origine publique exacte
- résultat : TERMINÉ (relecture approuvée, 2 tours)

## 2026-11-25 09:07 — Session v0.1-sdd — claude-code (orchestrateur) — Task R31 : Proxy : délais d'écriture et de corps, en-têtes, cibles
- fait : Délai d'écriture 30 s, corps 300 s et 1 Kio/s, WebSocket exempté, amonts locaux refusés sur IP canonique, en-têtes d'identité retirés, Host normalisé
- résultat : TERMINÉ (relecture approuvée, 1 tour ; forward.rs compilé pour la première fois en CI)

## 2026-11-25 09:10 — Session v0.1-sdd — claude-code (orchestrateur) — Task R30 : PKI et lien agent
- fait : renew-certs limité au serveur local et à la CA, révocation d'agent, retry QUIC, écriture atomique de la paire avec verrou, CA:false, vérification côté agent
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 10:15 — Session v0.1-sdd — claude-code (orchestrateur) — Task fix/ci-panel-witness : CI : témoin du panneau sondé avant son démarrage
- fait : panel_up avant les sondes de sortie dans run-docker-host.sh
- résultat : TERMINÉ

## 2026-11-25 10:16 — Session v0.1-sdd — claude-code (orchestrateur) — Task R32 : Proxy : socket de pilotage, certificats expirés
- fait : SO_PEERCRED root/ciphera-agent, délai par message, certificats expirés ni chargés ni servis
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 10:17 — Session v0.1-sdd — claude-code (orchestrateur) — Task R33 : Panneau web : journaux, CSP des styles, copie, 401, jetons d'URL
- fait : Pas de reconnexion sur 1008, sonde /api/me, nonce CSP par réponse, Copié seulement si réussi, 401 distingués, jetons retirés par le routeur
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 10:19 — Session v0.1-sdd — claude-code (orchestrateur) — Task R47 : Lien agent limité à la boucle locale, ADR 0004
- fait : Refus au chargement de la config hors boucle locale, ADR de l'enrôlement distant v0.2
- résultat : TERMINÉ (relecture approuvée)

## 2026-11-25 10:48 — Session v0.1-sdd — claude-code (orchestrateur) — Task fix/csp-fonts : CSP : polices servies en fichiers
- fait : assetsInlineLimit sans polices, garde check-dist contre les URI data: hors image
- résultat : TERMINÉ

## 2026-11-25 10:58 — Session v0.1-sdd — claude-code (orchestrateur) — Task R36 : Autorisations par objet, contraintes de schéma, OpenAPI
- fait : Migration 0037, chevauchement des globales, garde du port pour Ops, vues d'ensemble restreintes, sécurité OpenAPI des routes d'authentification
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 11:37 — Session v0.1-sdd — claude-code (orchestrateur) — Task R34 : Écoute HTTP du panneau : SPA, en-têtes, santé, délais
- fait : Repli SPA GET/HEAD, CSP complétée, version de /api/health réservée, délai de corps, inactivité HTTP/2 par connexion
- résultat : TERMINÉ (relecture approuvée, 2 tours)

## 2026-11-25 11:46 — Session v0.1-sdd — claude-code (orchestrateur) — Task fix/ci-team-hooks : CI : empreintes de hooks distinctes dans le test d'équipe
- fait : Test team.rs adapté à la contrainte UNIQUE de 0037
- résultat : TERMINÉ

## 2026-11-25 12:06 — Session v0.1-sdd — claude-code (orchestrateur) — Task R35 : Variables globales : portée explicite des secrets
- fait : Secret + tous les projets interdit, migration 0036 à valeurs conservées, Appliquer à ce projet
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 12:58 — Session v0.1-sdd — claude-code (orchestrateur) — Task R48 : Signataire de la CA séparé (root)
- fait : Signataire root séparé, dossier ca/ protégé, demandes mises de côté, docs
- résultat : TERMINÉ (relecture approuvée, 3 tours)

## 2026-11-25 13:08 — Session v0.1-sdd — claude-code (orchestrateur) — Task R4 : Réglages SMTP réservés au propriétaire
- fait : Propriétaire seul, adresses privées contrôlées à chaque connexion, refus classés en erreur de connexion, essai de 3 adresses
- résultat : TERMINÉ (relecture approuvée)

## 2026-11-25 13:20 — Session v0.1-sdd — claude-code (orchestrateur) — Task R38 : Hooks de déploiement, expiration, plafonds de domaines
- fait : Jeton par en-tête, expiration 90 j (plafond 365 j), mot de passe pour créer et rallonger, 20/100 domaines
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 14:01 — Session v0.1-sdd — claude-code (orchestrateur) — Task R51 : Nom du panneau caché, HSTS des sous-domaines
- fait : 404 par IP après confirmation, alerte unrecognized_name, HSTS selon la PSL avec option apex, psl 2.1.263
- résultat : TERMINÉ (relecture approuvée, 3 tours)

## 2026-11-25 19:08 — Session v0.1-sdd — claude-code (orchestrateur) — Task R37 : Setup, step-up, organisation, audit
- fait : Jeton de setup 24 h en fragment, step-up mot de passe + second facteur, suppression d'organisation protégée, purge d'audit, env root
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-25 19:29 — Session v0.1-sdd — claude-code (orchestrateur) — Task R39 : Profil release et dépendances non maintenues
- fait : overflow-checks en release, arithmétique ACME saturante, avis acceptés et documentés
- résultat : TERMINÉ (relecture approuvée)

## 2026-11-26 14:50 — Session v0.1-sdd — claude-code (orchestrateur) — Task R50 : Domaine généré en HTTPS
- fait : ACME pour le domaine sslip.io, repli HTTP, reprise quotidienne, certificats retirés
- résultat : TERMINÉ (relecture approuvée, 1 tour)

## 2026-11-26 15:02 — Session v0.1-sdd — claude-code (orchestrateur) — Task R49 : Rotation de la CA de l'instance
- fait : rotate-ca / --finish, confiance double, TrustedCas, migration 0041, /etc/ciphera root:ciphera 0750
- résultat : TERMINÉ (relecture approuvée, 3 tours)

## 2026-11-26 15:10 — Session v0.1-sdd — claude-code (orchestrateur) — Task R42 : Chaîne de release
- fait : site limité à la liste signée, design-to-dev figé, Assert-GitHubReady + -CheckOnly (2 relecteurs, prevent self-review, ruleset de tags, releases immuables), archives comparées avant signature
- résultat : TERMINÉ (relecture approuvée, 2 tours ; N1 mineur laissé)

## 2026-11-26 15:27 — Session v0.1-sdd — claude-code (orchestrateur) — Task R40 : install.sh / uninstall.sh durcis
- fait : script publié sans mode test, plafonds curl, compte ciphera existant vérifié, désinstallation ciblée, gardes des tests root
- résultat : TERMINÉ (relecture approuvée, 2 tours)

## 2026-11-26 15:47 — Session v0.1-sdd — claude-code (orchestrateur) — Task R43 : Hygiène du dépôt avant publication
- fait : données d'exemple réservées, masques du hook mémoire, CDN des prototypes, EXIF, CODEOWNERS @ciphera-dev/maintainers
- résultat : TERMINÉ (relecture approuvée après 2 tours de correction)

## 2026-11-26 16:22 — Session v0.1-sdd — claude-code (orchestrateur) — Task R54 : rsa 0.10.0, garde-fous Pingora
- fait : rsa 0.10.0 corrigée, exemption retirée, clippy interdit chargeurs YAML et clap Parser::parse, unmaintained = all
- résultat : TERMINÉ (relecture approuvée, 1 petit tour)

## 2026-11-26 16:23 — Session v0.1-sdd — claude-code (orchestrateur) — Task R41 : Updater root durci
- fait : téléchargements bornés, clé HMAC update-hmac.key atomique, restore.sql effacé, CA en lecture seule pour l'updater
- résultat : TERMINÉ (relecture approuvée, 2 tours)

## 2026-11-26 16:44 — Session v0.1-sdd — claude-code (orchestrateur) — Task R52 : Mode test retiré du binaire livré
- fait : feature install-test, CLI livré et ciphera-cli-test séparés, contrôle strings bloquant fail-closed, tests dans les deux configurations
- résultat : TERMINÉ (relecture approuvée, 1 petit tour)

## 2026-11-26 19:55 — Session v0.1-sdd — claude-code (orchestrateur) — Task R53 : latest.json signé, avec échéance
- fait : shared::latest, updater --latest, sign-latest/check-latest, refresh-latest.yml (clé de la release seulement), latest_expired dans l'API et le panneau
- résultat : TERMINÉ (relecture approuvée après 1 tour de correction)
