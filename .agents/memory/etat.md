# État courant — mis à jour le 2026-11-26 15:02

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R49 : Rotation de la CA de l'instance — statut : TERMINÉ (TERMINÉ (relecture approuvée, 3 tours))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R49 : Rotation de la CA de l'instance
- Task R50 : Domaine généré en HTTPS
- Task R39 : Profil release et dépendances non maintenues
- Task R37 : Setup, step-up, organisation, audit
- Task R51 : Nom du panneau caché, HSTS des sous-domaines
- Task R38 : Hooks de déploiement, expiration, plafonds de domaines
- Task R4 : Réglages SMTP réservés au propriétaire
- Task R48 : Signataire de la CA séparé (root)

## Prochaine étape exacte
R41, R42 en cours ; puis R40, R52-R54, R43, R44

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane/src/agents`
- `crates/control-plane/src/cli`
- `crates/agent/src`
- `crates/pki`
- `install`
- `deploy/systemd`
- `docs/operations/certificats.md`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

