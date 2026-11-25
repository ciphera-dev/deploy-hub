# État courant — mis à jour le 2026-11-25 09:07

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R31 : Proxy : délais d'écriture et de corps, en-têtes, cibles — statut : TERMINÉ (TERMINÉ (relecture approuvée, 1 tour ; forward.rs compilé pour la première fois en CI))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R31 : Proxy : délais d'écriture et de corps, en-têtes, cibles
- Task R29 : Hôtes du panneau, X-Forwarded-For, RP ID des passkeys
- Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent
- Task fix/ci-loglines : Journaux d'agent : aucune ligne partielle, aucun morceau de secret
- Task fix/R55-ci : CI : test d'annulation Compose de R55
- Task R27 : Remédiation audit — bac à sable de railpack prepare, askpass, .env de Compose
- Task R26 : Remédiation audit — images internes sous un registre non résoluble
- Task R25 : Remédiation audit — file de tâches bornée, déploiements jamais orphelins

## Prochaine étape exacte
Surveiller la CI (proxy) ; R30 en correction ; R4 en rebasage

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `Cargo.lock`
- `crates/proxy/Cargo.toml`
- `crates/proxy/src/forward.rs`
- `crates/proxy/src/main.rs`
- `crates/proxy/src/routes.rs`
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

