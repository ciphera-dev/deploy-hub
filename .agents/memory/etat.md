# État courant — mis à jour le 2026-11-24 21:23

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R11 : Remédiation audit — domaine du panneau, DNS et bascule sûre — statut : TERMINÉ (relecture : 1 Important corrigé, relecture ciblée approuvée, tests DB panel_domain 12/12 et domains 23/23)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R11 : Remédiation audit — domaine du panneau, DNS et bascule sûre
- Task R20 : Remédiation audit — anti-rejeu des webhooks sur l'empreinte du corps
- Task R16 : Remédiation audit — secrets liés à leur ligne, contrôle d'age.key
- Task R3b : Profil AppArmor appliqué au buildkitd
- Task R7 : Remédiation audit — ré-authentification avant un second facteur
- Task fixci : CI rouge après R45 et R6
- Task R45 : Remédiation audit — un état BuildKit par projet
- Task R6 : Remédiation audit — écran Équipe

## Prochaine étape exacte
Intégrer R4 (rebase en cours) et R18 (tour 2) ; R12, R15, R46 en cours

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane/src/domains/panel.rs`
- `crates/control-plane/src/domains/acme.rs`
- `crates/control-plane/migrations/0029_domains_dns_failures.sql`
- `crates/control-plane/tests/panel_domain.rs`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

