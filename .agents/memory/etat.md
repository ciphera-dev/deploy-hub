# État courant — mis à jour le 2026-11-25 11:46

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task fix/ci-team-hooks : CI : empreintes de hooks distinctes dans le test d'équipe — statut : TERMINÉ (TERMINÉ)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task fix/ci-team-hooks : CI : empreintes de hooks distinctes dans le test d'équipe
- Task R34 : Écoute HTTP du panneau : SPA, en-têtes, santé, délais
- Task R36 : Autorisations par objet, contraintes de schéma, OpenAPI
- Task fix/csp-fonts : CSP : polices servies en fichiers
- Task R47 : Lien agent limité à la boucle locale, ADR 0004
- Task R33 : Panneau web : journaux, CSP des styles, copie, 401, jetons d'URL
- Task R32 : Proxy : socket de pilotage, certificats expirés
- Task fix/ci-panel-witness : CI : témoin du panneau sondé avant son démarrage

## Prochaine étape exacte
Vérifier la CI ; R51

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane/tests/team.rs`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

