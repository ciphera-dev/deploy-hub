# État courant — mis à jour le 2026-11-25 11:37

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R34 : Écoute HTTP du panneau : SPA, en-têtes, santé, délais — statut : TERMINÉ (TERMINÉ (relecture approuvée, 2 tours))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R34 : Écoute HTTP du panneau : SPA, en-têtes, santé, délais
- Task R36 : Autorisations par objet, contraintes de schéma, OpenAPI
- Task fix/csp-fonts : CSP : polices servies en fichiers
- Task R47 : Lien agent limité à la boucle locale, ADR 0004
- Task R33 : Panneau web : journaux, CSP des styles, copie, 401, jetons d'URL
- Task R32 : Proxy : socket de pilotage, certificats expirés
- Task fix/ci-panel-witness : CI : témoin du panneau sondé avant son démarrage
- Task R30 : PKI et lien agent

## Prochaine étape exacte
R51 ; R35, R48 en cours ; R4 en rebasage

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `Cargo.lock`
- `crates/api-types/src/health.rs`
- `crates/control-plane/Cargo.toml`
- `crates/control-plane/src/app.rs`
- `crates/control-plane/src/embed.rs`
- `crates/control-plane/src/http/health.rs`
- `crates/control-plane/src/http/listener.rs`
- `crates/control-plane/src/http/security_headers.rs`
- `crates/control-plane/src/main.rs`
- `crates/control-plane/tests/common/mod.rs`
- `crates/control-plane/tests/headers.rs`
- `crates/control-plane/tests/health.rs`
- `crates/control-plane/tests/listener.rs`
- `crates/control-plane/tests/spa.rs`
- `openapi.json`
- `web/src/lib/api/schema.d.ts`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

