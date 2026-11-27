# État courant — mis à jour le 2026-11-27 13:13

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R66 : En-tête conforme à la maquette et navigation mobile — statut : TERMINÉ (Revue sonnet approuvée après un tour ; vitest 387/387)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R66 : En-tête conforme à la maquette et navigation mobile
- Task R60 : Audit de design de la landing
- Task bump011 : Version 0.1.1
- Task R59 : Landing page get.ciphera.ch
- Task R58 : Affichage soigné de l'installeur
- Task readme : README refait, code de conduite, remerciements
- Task fixe2e : Test golden-path aligné sur le step-up de R56
- Task R44 : Clôture de l'audit

## Prochaine étape exacte
R61 (seul en compilation), puis R62, R65, R63, R64, R67, R68

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `web/src/app/layout/AppShell.tsx`
- `web/src/app/layout/AppHeader.tsx`
- `web/src/app/layout/MobileTabBar.tsx`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

