# État courant — mis à jour le 2026-11-24 20:04

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task fixci : CI rouge après R45 et R6 — statut : TERMINÉ (reproduit sous Podman ; relectures approuvées)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task fixci : CI rouge après R45 et R6
- Task R45 : Remédiation audit — un état BuildKit par projet
- Task R6 : Remédiation audit — écran Équipe
- Task R5 : Remédiation audit — gestion de l'équipe (API)
- Task R10 : Remédiation audit — limites des services Compose
- Task R8 : Remédiation audit — proxy : échéance d'en-têtes par requête, plafonds de connexions, unité bornée
- Task R9 : Remédiation audit — Compose : redémarrage et étiquettes réservées
- Task R3 : Remédiation audit — ressources du build

## Prochaine étape exacte
Surveiller la CI ; intégrer R3b, R4, R7 ; relire R11, R16, R18, R20

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/agent`
- `install/tests`
- `web/e2e`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

