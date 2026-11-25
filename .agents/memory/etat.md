# État courant — mis à jour le 2026-11-25 09:10

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R30 : PKI et lien agent — statut : TERMINÉ (TERMINÉ (relecture approuvée, 1 tour))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R30 : PKI et lien agent
- Task R31 : Proxy : délais d'écriture et de corps, en-têtes, cibles
- Task R29 : Hôtes du panneau, X-Forwarded-For, RP ID des passkeys
- Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent
- Task fix/ci-loglines : Journaux d'agent : aucune ligne partielle, aucun morceau de secret
- Task fix/R55-ci : CI : test d'annulation Compose de R55
- Task R27 : Remédiation audit — bac à sable de railpack prepare, askpass, .env de Compose
- Task R26 : Remédiation audit — images internes sous un registre non résoluble

## Prochaine étape exacte
R47 ; R32 et R33 en cours ; R4 en rebasage

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/agent/src/config.rs`
- `crates/control-plane/src/agents/certs.rs`
- `crates/control-plane/src/agents/transport.rs`
- `crates/control-plane/src/cli/admin.rs`
- `crates/control-plane/src/main.rs`
- `crates/control-plane/tests/agent_link.rs`
- `crates/control-plane/tests/certs.rs`
- `crates/pki/Cargo.toml`
- `crates/pki/src/lib.rs`
- `crates/pki/tests/ca.rs`
- `docs/operations/certificats.md`
- `docs/security/threat-model.md`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

