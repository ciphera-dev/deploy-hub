# État courant — mis à jour le 2026-11-25 14:01

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R51 : Nom du panneau caché, HSTS des sous-domaines — statut : TERMINÉ (TERMINÉ (relecture approuvée, 3 tours))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R51 : Nom du panneau caché, HSTS des sous-domaines
- Task R38 : Hooks de déploiement, expiration, plafonds de domaines
- Task R4 : Réglages SMTP réservés au propriétaire
- Task R48 : Signataire de la CA séparé (root)
- Task R35 : Variables globales : portée explicite des secrets
- Task fix/ci-team-hooks : CI : empreintes de hooks distinctes dans le test d'équipe
- Task R34 : Écoute HTTP du panneau : SPA, en-têtes, santé, délais
- Task R36 : Autorisations par objet, contraintes de schéma, OpenAPI

## Prochaine étape exacte
R37, R49 en cours ; correctif CSP form-action ; puis R50, R39

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane/src/http/redirect.rs`
- `crates/proxy/src`
- `crates/proto/src`
- `web/src`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

