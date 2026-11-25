# État courant — mis à jour le 2026-11-25 01:12

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R21 : Remédiation audit — GitHub, dépôt identifié par son id, jeton restreint — statut : TERMINÉ (relecture approuvée, finitions relues)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R21 : Remédiation audit — GitHub, dépôt identifié par son id, jeton restreint
- Task R14 : Remédiation audit — origine des liens de réinitialisation, remise à zéro du second facteur
- Task R15 : Remédiation audit — vérification de l'adresse e-mail
- Task R12 : Remédiation audit — 2FA imposée aux sessions ouvertes, sécurité réservée au propriétaire
- Task R46 : Remédiation audit — utilisateur et racine en lecture seule des conteneurs
- Task R13 : Remédiation audit — limites du second facteur, de la ré-authentification et de la connexion
- Task fix-ci-zombie : CI : buildkitd zombie à la désinstallation
- Task R18 : Remédiation audit — secrets masqués dans les journaux avant toute coupe

## Prochaine étape exacte
Intégrer R14 (conflit en cours), R17, R4 ; R23 en cours

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane/src/connectors/github/app.rs`
- `crates/control-plane/src/orchestrator.rs`
- `crates/control-plane/migrations/0034_github_repo_id.sql`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

