# État courant — mis à jour le 2026-11-24 23:30

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R12 : Remédiation audit — 2FA imposée aux sessions ouvertes, sécurité réservée au propriétaire — statut : TERMINÉ (relecture : 1 Important corrigé, approuvée ; vérifiée avec R13)

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R12 : Remédiation audit — 2FA imposée aux sessions ouvertes, sécurité réservée au propriétaire
- Task R46 : Remédiation audit — utilisateur et racine en lecture seule des conteneurs
- Task R13 : Remédiation audit — limites du second facteur, de la ré-authentification et de la connexion
- Task fix-ci-zombie : CI : buildkitd zombie à la désinstallation
- Task R18 : Remédiation audit — secrets masqués dans les journaux avant toute coupe
- Task R11 : Remédiation audit — domaine du panneau, DNS et bascule sûre
- Task R20 : Remédiation audit — anti-rejeu des webhooks sur l'empreinte du corps
- Task R16 : Remédiation audit — secrets liés à leur ligne, contrôle d'age.key

## Prochaine étape exacte
Intégrer R4 ; R15 tour 3 ; R14 en cours ; lancer R17, R19, R21…

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane/src/http/settings.rs`
- `crates/control-plane/src/http/me.rs`
- `crates/control-plane/src/http/auth.rs`
- `web/src/features/settings/sections/OrgSecurity.tsx`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

