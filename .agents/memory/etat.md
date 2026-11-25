# État courant — mis à jour le 2026-11-25 02:48

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R22 : Remédiation audit — nettoyage des déploiements confirmé par l'agent — statut : TERMINÉ (TERMINÉ (relecture : 1 Important corrigé, approuvée))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R22 : Remédiation audit — nettoyage des déploiements confirmé par l'agent
- Task R23 : Remédiation audit — retrait complet d'un projet supprimé sur ses serveurs
- Task R17 : Remédiation audit — variables, import .env, masquage multi-motifs
- Task R21 : Remédiation audit — GitHub, dépôt identifié par son id, jeton restreint
- Task R14 : Remédiation audit — origine des liens de réinitialisation, remise à zéro du second facteur
- Task R15 : Remédiation audit — vérification de l'adresse e-mail
- Task R12 : Remédiation audit — 2FA imposée aux sessions ouvertes, sécurité réservée au propriétaire
- Task R46 : Remédiation audit — utilisateur et racine en lecture seule des conteneurs

## Prochaine étape exacte
Intégrer R4 (conflit R14) ; R19, R24 en cours ; lancer R55

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/agent/src/deploy.rs`
- `crates/agent/src/main.rs`
- `crates/control-plane/src/orchestrator.rs`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

