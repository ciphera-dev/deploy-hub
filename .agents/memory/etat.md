# État courant — mis à jour le 2026-11-25 07:52

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R29 : Hôtes du panneau, X-Forwarded-For, RP ID des passkeys — statut : TERMINÉ (TERMINÉ (relecture approuvée, 2 tours))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R29 : Hôtes du panneau, X-Forwarded-For, RP ID des passkeys
- Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent
- Task fix/ci-loglines : Journaux d'agent : aucune ligne partielle, aucun morceau de secret
- Task fix/R55-ci : CI : test d'annulation Compose de R55
- Task R27 : Remédiation audit — bac à sable de railpack prepare, askpass, .env de Compose
- Task R26 : Remédiation audit — images internes sous un registre non résoluble
- Task R25 : Remédiation audit — file de tâches bornée, déploiements jamais orphelins
- Task R55 : Remédiation audit — restauration des services Compose dépendants

## Prochaine étape exacte
R4 en rebasage ; lancer R30

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `.env.example`
- `README.md`
- `crates/api-types/src/domains.rs`
- `crates/api-types/src/mfa.rs`
- `crates/api-types/src/webauthn.rs`
- `crates/control-plane/src/auth/webauthn.rs`
- `crates/control-plane/src/domain/domains/mod.rs`
- `crates/control-plane/src/http/client_ip.rs`
- `crates/control-plane/src/http/domains.rs`
- `crates/control-plane/src/http/ip_allowlist.rs`
- `crates/control-plane/src/http/mfa.rs`
- `crates/control-plane/src/http/mod.rs`
- `crates/control-plane/src/http/panel_domain.rs`
- `crates/control-plane/src/http/panel_host.rs`
- `crates/control-plane/src/http/webauthn.rs`
- `crates/control-plane/src/main.rs`
- `crates/control-plane/tests/auth_login.rs`
- `crates/control-plane/tests/common/api.rs`
- `crates/control-plane/tests/domains.rs`
- `crates/control-plane/tests/listener.rs`
- `crates/control-plane/tests/panel_domain.rs`
- `crates/control-plane/tests/settings.rs`
- `crates/control-plane/tests/webauthn.rs`
- `crates/shared/src/config.rs`
- `docs/superpowers/specs/2026-11-21-ciphera-v0.1-design.md`
- `openapi.json`
- `web/src/features/auth/CodeStep.tsx`
- `web/src/features/auth/LoginFlow.test.tsx`
- `web/src/features/auth/LoginFlow.tsx`
- `web/src/features/auth/api.ts`
- `web/src/features/profile/security/SecurityKeyCard.test.tsx`
- `web/src/features/profile/security/SecurityKeyCard.tsx`
- `web/src/features/settings/SettingsPage.test.tsx`
- `web/src/features/settings/sections/PanelDomain.tsx`
- `web/src/lib/api/schema.d.ts`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

