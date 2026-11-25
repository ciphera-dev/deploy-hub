# État courant — mis à jour le 2026-11-25 07:42

## Objectif global
Exécuter le plan v0.1 (docs/superpowers/plans/2026-11-21-ciphera-v0.1.md) par sous-agents (subagent-driven-development), commit + push par tâche

## Tâche en cours
Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent — statut : TERMINÉ (TERMINÉ (relecture approuvée, 1 tour))

## Tâche précédente
Task 35b : Domaine get.ciphera.ch, clé de production, tri CodeQL, SECURITY.md et CONTRIBUTING.md — statut : TERMINÉ (relectures propres ; CI à confirmer sur le dernier commit)

## Fait récemment
- Task R28 : Sortie réseau des applications et des builds, durcissement de l'agent
- Task fix/ci-loglines : Journaux d'agent : aucune ligne partielle, aucun morceau de secret
- Task fix/R55-ci : CI : test d'annulation Compose de R55
- Task R27 : Remédiation audit — bac à sable de railpack prepare, askpass, .env de Compose
- Task R26 : Remédiation audit — images internes sous un registre non résoluble
- Task R25 : Remédiation audit — file de tâches bornée, déploiements jamais orphelins
- Task R55 : Remédiation audit — restauration des services Compose dépendants
- Task R24 : Remédiation audit — ordre des services Compose autour de la sonde, annulation tardive

## Prochaine étape exacte
Intégrer R29 ; R4 en rebasage ; lancer R30

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
- `crates/agent/src/deploy.rs`
- `crates/agent/src/docker.rs`
- `crates/agent/src/egress.rs`
- `crates/agent/src/infra.rs`
- `crates/agent/src/lib.rs`
- `crates/agent/src/main.rs`
- `crates/agent/tests/common/mod.rs`
- `crates/agent/tests/containers.rs`
- `crates/agent/tests/deploy.rs`
- `crates/agent/tests/docker_it.rs`
- `crates/cli/src/updater.rs`
- `crates/cli/tests/updater.rs`
- `crates/control-plane/tests/systemd_units.rs`
- `deploy/systemd/ciphera-agent.service`
- `deploy/systemd/ciphera-egress-check.service`
- `deploy/systemd/ciphera-egress-check.timer`
- `docs/security/threat-model.md`
- `docs/superpowers/specs/2026-11-21-ciphera-v0.1-design.md`
- `install/install.sh`
- `install/tests/run-docker-host.sh`
- `install/tests/run.sh`
- `install/tests/units-boot.sh`
- `install/uninstall.sh`

## Questions ouvertes pour l'utilisateur
- Qui se connecte (admin seul, admin + utilisateurs) ? MFA obligatoire ?
- Le backend sert-il plusieurs clients (web + mobile + desktop) ?

## Points de vigilance
- Base vérifiée le 2026-11-23 — 0 point(s) bloquant(s) ouvert(s) (docs/security/fondations.md)
- connexion présente : Argon2id, cookies HttpOnly, rate limit (voir docs/security/exigences.md)
- Pages2.jsx (Paramètres) contient une section Licence (clé cph_live_, mode limité 1 serveur/2 membres, vérif Ciphera Cloud, lien cloud-billing.html) : à retirer de la copie et à ne pas porter — Ciphera 100 % open source

