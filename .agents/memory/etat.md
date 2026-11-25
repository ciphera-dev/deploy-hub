# État courant — mis à jour le 2026-11-25 12:06

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R35 : Variables globales : portée explicite des secrets — statut : TERMINÉ (TERMINÉ (relecture approuvée, 1 tour))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R35 : Variables globales : portée explicite des secrets
- Task fix/ci-team-hooks : CI : empreintes de hooks distinctes dans le test d'équipe
- Task R34 : Écoute HTTP du panneau : SPA, en-têtes, santé, délais
- Task R36 : Autorisations par objet, contraintes de schéma, OpenAPI
- Task fix/csp-fonts : CSP : polices servies en fichiers
- Task R47 : Lien agent limité à la boucle locale, ADR 0004
- Task R33 : Panneau web : journaux, CSP des styles, copie, 401, jetons d'URL
- Task R32 : Proxy : socket de pilotage, certificats expirés

## Prochaine étape exacte
R38 ; R48, R51 en cours ; R4 en rebasage

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/api-types/src/variables.rs`
- `crates/control-plane/migrations/0036_global_secret_scope.sql`
- `crates/control-plane/src/domain/variables.rs`
- `crates/control-plane/src/http/variables.rs`
- `crates/control-plane/tests/migration_0036.rs`
- `crates/control-plane/tests/settings.rs`
- `crates/control-plane/tests/variables.rs`
- `docs/superpowers/specs/2026-11-21-ciphera-v0.1-design.md`
- `openapi.json`
- `web/src/features/variables/GlobalVariables.test.tsx`
- `web/src/features/variables/GlobalVariables.tsx`
- `web/src/features/variables/ProjectVariables.test.tsx`
- `web/src/features/variables/ProjectVariables.tsx`
- `web/src/lib/api/schema.d.ts`
- `web/src/mocks/fixtures/variables.ts`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

