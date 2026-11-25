# État courant — mis à jour le 2026-11-25 10:58

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R36 : Autorisations par objet, contraintes de schéma, OpenAPI — statut : TERMINÉ (TERMINÉ (relecture approuvée, 1 tour))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R36 : Autorisations par objet, contraintes de schéma, OpenAPI
- Task fix/csp-fonts : CSP : polices servies en fichiers
- Task R47 : Lien agent limité à la boucle locale, ADR 0004
- Task R33 : Panneau web : journaux, CSP des styles, copie, 401, jetons d'URL
- Task R32 : Proxy : socket de pilotage, certificats expirés
- Task fix/ci-panel-witness : CI : témoin du panneau sondé avant son démarrage
- Task R30 : PKI et lien agent
- Task R31 : Proxy : délais d'écriture et de corps, en-têtes, cibles

## Prochaine étape exacte
R34 en cours ; R48 ; R35 ; R38

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/api-types/src/github.rs`
- `crates/api-types/src/lib.rs`
- `crates/api-types/src/logs.rs`
- `crates/api-types/src/overview.rs`
- `crates/api-types/src/project_overview.rs`
- `crates/control-plane/migrations/0037_object_constraints.sql`
- `crates/control-plane/src/auth/rate_limit.rs`
- `crates/control-plane/src/authz/mod.rs`
- `crates/control-plane/src/domain/domains/mod.rs`
- `crates/control-plane/src/domain/variables.rs`
- `crates/control-plane/src/http/domains.rs`
- `crates/control-plane/src/http/overview.rs`
- `crates/control-plane/src/http/project_overview.rs`
- `crates/control-plane/src/http/variables.rs`
- `crates/control-plane/tests/authz_matrix.rs`
- `crates/control-plane/tests/common/mod.rs`
- `crates/control-plane/tests/domains.rs`
- `crates/control-plane/tests/mfa.rs`
- `crates/control-plane/tests/migration_0037.rs`
- `crates/control-plane/tests/variables.rs`
- `crates/control-plane/tests/webauthn.rs`
- `openapi.json`
- `web/src/features/overview/OverviewPage.tsx`
- `web/src/features/projects/overview/ProjectOverview.tsx`
- `web/src/lib/api/schema.d.ts`
- `web/src/mocks/handlers.ts`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

