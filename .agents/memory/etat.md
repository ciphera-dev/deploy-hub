# État courant — mis à jour le 2026-11-25 10:17

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R33 : Panneau web : journaux, CSP des styles, copie, 401, jetons d'URL — statut : TERMINÉ (TERMINÉ (relecture approuvée, 1 tour))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R33 : Panneau web : journaux, CSP des styles, copie, 401, jetons d'URL
- Task R32 : Proxy : socket de pilotage, certificats expirés
- Task fix/ci-panel-witness : CI : témoin du panneau sondé avant son démarrage
- Task R30 : PKI et lien agent
- Task R31 : Proxy : délais d'écriture et de corps, en-têtes, cibles
- Task R29 : Hôtes du panneau, X-Forwarded-For, RP ID des passkeys
- Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent
- Task fix/ci-loglines : Journaux d'agent : aucune ligne partielle, aucun morceau de secret

## Prochaine étape exacte
R34 ; R48

## Action en cours au moment de l'arrêt (si applicable)
- Commande / modification : aucune
- Fichiers concernés : aucun
- Résultat connu : terminé

## Commandes pour reprendre
- Registre SDD (non versionné) : .superpowers/sdd/2026-11-21-ciphera-v0.1/progress.md ; sinon git log
- cargo test --workspace (PowerShell) ; npm --prefix web test

## Fichiers clés touchés dans cette tâche
- `crates/control-plane/src/embed.rs`
- `crates/control-plane/src/http/security_headers.rs`
- `crates/control-plane/tests/headers.rs`
- `crates/control-plane/tests/spa.rs`
- `web/e2e/csp.ts`
- `web/e2e/golden-path.spec.ts`
- `web/e2e/shell.spec.ts`
- `web/index.html`
- `web/package-lock.json`
- `web/package.json`
- `web/src/app/providers.tsx`
- `web/src/design-system/components/core/CopyButton.tsx`
- `web/src/design-system/components/data/CodeBlock.tsx`
- `web/src/features/auth/InviteAccept.test.tsx`
- `web/src/features/auth/LinkGone.tsx`
- `web/src/features/auth/ResetPassword.test.tsx`
- `web/src/features/deployments/DeploymentsTable.tsx`
- `web/src/features/logs/LogViewer.test.tsx`
- `web/src/features/logs/LogViewer.tsx`
- `web/src/features/logs/useLogStream.test.tsx`
- `web/src/features/logs/useLogStream.ts`
- `web/src/features/profile/api.test.ts`
- `web/src/features/profile/api.ts`
- `web/src/features/profile/security/SecurityKeyCard.tsx`
- `web/src/features/profile/security/api.ts`
- `web/src/features/variables/GlobalVariables.tsx`
- `web/src/features/variables/ProjectVariables.tsx`
- `web/src/lib/api/errors.ts`
- `web/src/lib/api/session.ts`
- `web/src/lib/clipboard.test.ts`
- `web/src/lib/clipboard.ts`
- `web/src/lib/csp-nonce.test.ts`
- `web/src/lib/csp-nonce.ts`
- `web/src/lib/url-token.ts`
- `web/src/lib/ws.test.ts`
- `web/src/lib/ws.ts`
- `web/src/main.tsx`
- `web/src/routeTree.gen.ts`
- `web/src/routes/_auth/invite.$token.tsx`
- `web/src/routes/_auth/invite.index.tsx`
- `web/src/routes/_auth/reset.$token.tsx`
- `web/src/routes/_auth/reset.index.tsx`
- `web/src/routes/_auth/verify-email.$token.tsx`
- `web/src/routes/_auth/verify-email.index.tsx`
- `web/src/routes/token-links.test.tsx`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

