# Contribuer à Ciphera

Merci de votre intérêt. Ce document résume ce qu'il faut savoir avant d'ouvrir une pull request.

En participant, vous vous engagez à respecter le [code de conduite](CODE_OF_CONDUCT.md).

## Avant de commencer

Lisez dans cet ordre :

1. [`README.md`](README.md) : le projet, la stack, l'arborescence ;
2. [`AGENTS.md`](AGENTS.md) : les règles permanentes du projet ;
3. [`docs/adr/`](docs/adr/) : les décisions d'architecture. Un changement structurel s'accompagne d'un nouvel ADR.

Les dossiers `.agents/` et `AGENTS.md` servent de mémoire de travail aux agents automatisés ; une contribution externe n'a pas à les modifier.

## Prérequis

- Rust (version épinglée dans `rust-toolchain.toml`) ;
- Node 22 et npm ;
- Docker (tests d'intégration du plan de contrôle) ;
- [`just`](https://github.com/casey/just) (facultatif, il enveloppe les commandes ci-dessous) ;
- `cargo-audit` et `cargo-deny` ;
- `shellcheck` et `actionlint` pour les scripts et les workflows.

## Vérifier en local comme la CI

Les commandes correspondent aux jobs de `.github/workflows/ci.yml` :

| Job | Commande |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | D'abord l'interface, que le plan de contrôle embarque (`web/dist` doit exister) : `npm --prefix web ci` puis `npm --prefix web run build`. Ensuite `cargo test --workspace` (Docker requis : les tests d'intégration démarrent PostgreSQL) |
| `cargo-audit` | `cargo audit` |
| `cargo-deny` | `cargo deny check` |
| `web` | `npm --prefix web ci`, puis `npm --prefix web test`, `npm --prefix web run lint`, `npx --prefix web playwright install --with-deps chromium` (une fois), `npm --prefix web run e2e` et `npm --prefix web audit --audit-level=high` |
| `gen-is-fresh` | `npm --prefix web ci`, puis `just gen`, puis `git diff --exit-code openapi.json web/src/lib/api/schema.d.ts` : `openapi.json` et les types de l'API web régénérés doivent être commités |
| `shellcheck` | `find install -type f -name '*.sh' -print0 \| xargs -0 -r shellcheck` |
| `actionlint` | `actionlint` |
| `release-scripts` | `bash install/tests/release-scripts.sh` |
| `ai-traces` | `npx design-to-dev ai-traces scan . --strict` |

`just test`, `just lint` et `just audit` regroupent une grande partie de ces commandes.

Les tests qui exigent Linux, systemd ou Docker sont marqués `#[ignore]` et tournent en CI (jobs `agent-docker`, `acme-pebble`, `install-docker`, `e2e-golden-path`, `units-boot`, `fuzz-proto`). Les rejouer en local demande une machine Linux jetable avec Docker et systemd.

## Tests de bout en bout (Playwright)

- `npm --prefix web run e2e` : la SPA construite servie par `vite preview` (aucune instance requise ; `golden-path.spec.ts` est ignoré).
- Parcours golden-path (`web/e2e/golden-path.spec.ts` : setup → TOTP → GitHub App simulée → déploiement → domaine via Pebble → HTTPS → rollback) : seulement sur une machine Linux jetable avec systemd et Docker (job CI `e2e-golden-path`). `install/tests/golden-path/setup.sh` y installe Ciphera, Pebble et un GitHub simulé, puis écrit les variables `CIPHERA_E2E_*` :

  ```sh
  sudo CIPHERA_DESTRUCTIVE_HOST_TEST=1 bash install/tests/golden-path/setup.sh /tmp/golden-path
  set -a; . /tmp/golden-path/e2e.env; set +a
  npm --prefix web run e2e -- golden-path
  ```

  Sans `CIPHERA_E2E_URL`, le parcours est sauté. Le script modifie la machine (`/etc/hosts`, magasin de certificats, unités systemd).

## Maquettes de référence

`prototypes/` contient les maquettes d'origine du panneau. Elles servent de référence visuelle seulement, jamais de base de production, et décrivent aussi des écrans qui n'existent pas encore dans le code ; ce qui est réellement porté est listé dans [`docs/design/screens.md`](docs/design/screens.md).

Pour les voir, servir la **racine du dépôt** en HTTP (pas en `file://`, car Babel et les imports l'exigent), puis ouvrir `prototypes/dashboard/index.html` :

```sh
python -m http.server 8000
# → http://localhost:8000/prototypes/dashboard/index.html
#   aussi : login.html, register.html, 2fa.html, install.html, 404.html
```

Les maquettes chargent React 18 (dev) et Babel depuis unpkg.

## Publier une release

La publication d'une version (tag `v*`, signatures, approbations, site d'installation) suit [`docs/operations/release.md`](docs/operations/release.md). Elle est réservée aux mainteneurs.

## Conventions

- Documentation, commentaires et messages de commit sont en **français**. Les issues et les pull requests peuvent être en français ou en anglais.
- Commits au format **Conventional Commits**, sujet en français, par exemple :
  - `fix(api): limite de taille du corps sur le domaine du panneau`
  - `feat(notifications): e-mails de déploiement et de certificats`
  - `chore(release): clé publique minisign de production`
  - `docs(spec): sources Git hors GitHub en v0.2`
- Une pull request traite un seul sujet.
- Tout changement de comportement vient avec des tests.
- Les écrans de l'interface sont portés depuis `docs/design/screens.md`, pas redessinés.

## Dépendances

Avant d'ajouter une dépendance, vérifiez sa version courante et ses CVE (`cargo audit`, `npm audit`) et justifiez le choix dans la pull request. Ne copiez jamais une bibliothèque depuis un CDN : installez-la par le gestionnaire de paquets.

## Secrets et traces d'outils

- Aucun secret dans le code, les tests ni la documentation : mettez seulement des noms de variables dans `.env.example`.
- Aucune mention d'outil de génération dans les fichiers (commentaire « Generated by », balise `generator`, badge, caractère invisible). Seule exception : la section « Remerciements » du README et de ce document. Lancez `npx design-to-dev ai-traces scan . --strict` avant chaque commit.

## Failles de sécurité

Ne les signalez jamais par une issue publique. Suivez [`SECURITY.md`](SECURITY.md).

## Remerciements

Le développement de Ciphera a été réalisé avec **Kimi** (Moonshot AI), puis **Claude Code** (Anthropic), comme l'indique aussi la section [Remerciements du README](README.md#remerciements).

<p>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/brand/kimi-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/brand/kimi-light.svg">
    <img alt="Kimi" src="docs/assets/brand/kimi-light.svg" height="32">
  </picture>
  &nbsp;&nbsp;&nbsp;
  <img alt="Claude Code" src="docs/assets/brand/claude.svg" height="32">
</p>

Sources des logos et conditions d'usage des marques : [`docs/assets/brand/CREDITS.md`](docs/assets/brand/CREDITS.md).

## Licence

Le projet est publié sous la licence GNU Affero General Public License v3.0 (fichier [`LICENSE`](LICENSE)). Votre contribution est publiée sous cette même licence.
