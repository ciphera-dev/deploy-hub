# État courant — mis à jour le 2026-11-27 01:07

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task bump011 : Version 0.1.1 — statut : TERMINÉ (Vérifié par l'orchestrateur (diff mécanique, cargo check))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task bump011 : Version 0.1.1
- Task R59 : Landing page get.ciphera.ch
- Task R58 : Affichage soigné de l'installeur
- Task readme : README refait, code de conduite, remerciements
- Task fixe2e : Test golden-path aligné sur le step-up de R56
- Task R44 : Clôture de l'audit
- Task R56 : Step-up pour les changements d'équipe privilégiés
- Task R53 : latest.json signé, avec échéance

## Prochaine étape exacte
R60 (audit design de la landing), CI verte, tag v0.1.1

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `Cargo.toml`
- `install/install.sh`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

