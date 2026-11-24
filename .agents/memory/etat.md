# État courant — mis à jour le 2026-11-24 16:43

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R9 : Remédiation audit — Compose : redémarrage et étiquettes réservées — statut : TERMINÉ (relecture propre ; agent-docker à confirmer)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R9 : Remédiation audit — Compose : redémarrage et étiquettes réservées
- Task R3 : Remédiation audit — ressources du build
- Task R2 : Remédiation audit — cloisonnement des caches de build
- Task R1 : Remédiation audit — protocole v6
- Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md
- Task 35 : Préparation de la publication (signature de production, cosign, SLSA, site get.ciphera.dev)
- Task N : E-mails de notification et sources Git hors GitHub en v0.2 (décisions utilisateur)
- Task 34c-2 : Durcissement du plan de contrôle, de l'agent et des mises à jour

## Prochaine étape exacte
Intégrer R5 et R8 après relecture ; puis R3b, R45, R4, R10

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/proxy/src/forward.rs`
- `crates/proxy/src/routes.rs`
- `crates/proxy/src/control.rs`
- `crates/proxy/src/conn_limit.rs`
- `crates/agent/src/proxy_client.rs`
- `deploy/systemd/ciphera-proxy.service`
- `deploy/systemd/ciphera-proxy.socket`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

