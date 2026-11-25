# État courant — mis à jour le 2026-11-25 10:16

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R32 : Proxy : socket de pilotage, certificats expirés — statut : TERMINÉ (TERMINÉ (relecture approuvée, 1 tour))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R32 : Proxy : socket de pilotage, certificats expirés
- Task fix/ci-panel-witness : CI : témoin du panneau sondé avant son démarrage
- Task R30 : PKI et lien agent
- Task R31 : Proxy : délais d'écriture et de corps, en-têtes, cibles
- Task R29 : Hôtes du panneau, X-Forwarded-For, RP ID des passkeys
- Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent
- Task fix/ci-loglines : Journaux d'agent : aucune ligne partielle, aucun morceau de secret
- Task fix/R55-ci : CI : test d'annulation Compose de R55

## Prochaine étape exacte
R33

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/proxy/src/acme.rs`
- `crates/proxy/src/control.rs`
- `crates/proxy/src/lib.rs`
- `crates/proxy/src/main.rs`
- `crates/proxy/tests/acme.rs`
- `crates/proxy/tests/forward.rs`
- `crates/proxy/tests/routes.rs`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

