# État courant — mis à jour le 2026-11-24 17:22

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R5 : Remédiation audit — gestion de l'équipe (API) — statut : TERMINÉ (relecture : 1 Important corrigé (invitations du membre retiré), lien de réinitialisation borné par can_manage ; relecture ciblée approuvée)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R5 : Remédiation audit — gestion de l'équipe (API)
- Task R10 : Remédiation audit — limites des services Compose
- Task R8 : Remédiation audit — proxy : échéance d'en-têtes par requête, plafonds de connexions, unité bornée
- Task R9 : Remédiation audit — Compose : redémarrage et étiquettes réservées
- Task R3 : Remédiation audit — ressources du build
- Task R2 : Remédiation audit — cloisonnement des caches de build
- Task R1 : Remédiation audit — protocole v6
- Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md

## Prochaine étape exacte
Relire R45 et R4 ; puis R6 (écran Équipe), R3b, R7

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane`
- `crates/api-types`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

