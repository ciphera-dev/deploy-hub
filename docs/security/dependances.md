# Dépendances, runtimes et API — 2026-11-24

Contrôlé par design-to-dev v2.7.0 — **hors ligne** : failles connues et versions stables non vérifiées. Relancer avec `--online` (accord de l'utilisateur ; seuls des noms et versions de paquets publics sont envoyés).

**Bilan : 1074 paquet(s) dont 128 direct(s), ? faille(s) connue(s), 1 runtime(s), 1 API externe(s)**

## Failles connues

- non vérifié (hors ligne)

## Runtimes et frameworks

| Composant | Version | Fichier | Fin de support | Conseil |
|---|---|---|---|---|
| Node.js | 22 | `crates/agent/tests/fixtures/node-app/package.json` | support minimal 22, base hors ligne du 2026-10-25 | maintenu |

## Dépendances directes

| Paquet | Écosystème | Utilisée | Dernière stable | Historique des failles | Fichier |
|---|---|---|---|---|---|
| age | crates.io | 0.11.5 | ? | ? | `Cargo.lock` |
| arc-swap | crates.io | 1.9.5 | ? | ? | `Cargo.lock` |
| argon2 | crates.io | 0.5.3 | ? | ? | `Cargo.lock` |
| argon2 | crates.io | 0.6.0 | ? | ? | `Cargo.lock` |
| axum | crates.io | 0.8.9 | ? | ? | `Cargo.lock` |
| axum-server | crates.io | 0.9.2 | ? | ? | `Cargo.lock` |
| base64 | crates.io | 0.21.7 | ? | ? | `Cargo.lock` |
| base64 | crates.io | 0.22.1 | ? | ? | `Cargo.lock` |
| base64 | crates.io | 0.23.1 | ? | ? | `Cargo.lock` |
| bollard | crates.io | 0.20.2 | ? | ? | `Cargo.lock` |
| bytes | crates.io | 1.12.2 | ? | ? | `Cargo.lock` |
| clap | crates.io | 4.6.9 | ? | ? | `Cargo.lock` |
| flate2 | crates.io | 1.1.11 | ? | ? | `Cargo.lock` |
| futures-util | crates.io | 0.3.34 | ? | ? | `Cargo.lock` |
| hmac | crates.io | 0.12.1 | ? | ? | `Cargo.lock` |
| hmac | crates.io | 0.13.0 | ? | ? | `Cargo.lock` |
| image | crates.io | 0.25.10 | ? | ? | `Cargo.lock` |
| instant-acme | crates.io | 0.8.5 | ? | ? | `Cargo.lock` |
| jiff | crates.io | 0.2.46 | ? | ? | `Cargo.lock` |
| jiff | crates.io | 1.0.2 | ? | ? | `Cargo.lock` |
| lettre | crates.io | 0.11.24 | ? | ? | `Cargo.lock` |
| minisign | crates.io | 0.10.0 | ? | ? | `Cargo.lock` |
| minisign-verify | crates.io | 0.3.0 | ? | ? | `Cargo.lock` |
| postcard | crates.io | 1.2.0 | ? | ? | `Cargo.lock` |
| qrcodegen | crates.io | 1.8.0 | ? | ? | `Cargo.lock` |
| quinn | crates.io | 0.11.12 | ? | ? | `Cargo.lock` |
| rand | crates.io | 0.8.8 | ? | ? | `Cargo.lock` |
| rand | crates.io | 0.9.5 | ? | ? | `Cargo.lock` |
| rand | crates.io | 0.10.4 | ? | ? | `Cargo.lock` |
| rcgen | crates.io | 0.14.10 | ? | ? | `Cargo.lock` |
| reqwest | crates.io | 0.13.5 | ? | ? | `Cargo.lock` |
| ring | crates.io | 0.17.14 | ? | ? | `Cargo.lock` |
| rusqlite | crates.io | 0.43.0 | ? | ? | `Cargo.lock` |
| rust-embed | crates.io | 8.14.0 | ? | ? | `Cargo.lock` |
| rustls | crates.io | 0.23.46 | ? | ? | `Cargo.lock` |
| serde | crates.io | 1.0.229 | ? | ? | `Cargo.lock` |
| serde_json | crates.io | 1.0.154 | ? | ? | `Cargo.lock` |
| serde_norway | crates.io | 0.9.42 | ? | ? | `Cargo.lock` |
| sha2 | crates.io | 0.10.9 | ? | ? | `Cargo.lock` |
| sha2 | crates.io | 0.11.1 | ? | ? | `Cargo.lock` |
| sqlx | crates.io | 0.9.1 | ? | ? | `Cargo.lock` |
| subtle | crates.io | 2.6.1 | ? | ? | `Cargo.lock` |
| sysinfo | crates.io | 0.40.4 | ? | ? | `Cargo.lock` |
| tar | crates.io | 0.4.46 | ? | ? | `Cargo.lock` |
| time | crates.io | 0.3.55 | ? | ? | `Cargo.lock` |
| tokio | crates.io | 1.54.1 | ? | ? | `Cargo.lock` |
| totp-rs | crates.io | 6.0.0 | ? | ? | `Cargo.lock` |
| tower-http | crates.io | 0.6.11 | ? | ? | `Cargo.lock` |
| tower-http | crates.io | 0.7.1 | ? | ? | `Cargo.lock` |
| tracing | crates.io | 0.1.44 | ? | ? | `Cargo.lock` |
| tracing-subscriber | crates.io | 0.3.23 | ? | ? | `Cargo.lock` |
| utoipa | crates.io | 6.0.0 | ? | ? | `Cargo.lock` |
| uuid | crates.io | 1.29.3 | ? | ? | `Cargo.lock` |
| webauthn-rs | crates.io | 0.6.2 | ? | ? | `Cargo.lock` |
| webpki-roots | crates.io | 1.0.12 | ? | ? | `Cargo.lock` |
| x509-parser | crates.io | 0.16.0 | ? | ? | `Cargo.lock` |
| x509-parser | crates.io | 0.18.1 | ? | ? | `Cargo.lock` |
| zeroize | crates.io | 1.9.2 | ? | ? | `Cargo.lock` |
| zstd | crates.io | 0.14.4 | ? | ? | `Cargo.lock` |
| libfuzzer-sys | crates.io | 0.4.13 | ? | ? | `crates/proto/fuzz/Cargo.lock` |
| @fontsource/geist | npm | 5.3.0 | ? | ? | `web/package-lock.json` |
| @fontsource/geist-mono | npm | 5.3.0 | ? | ? | `web/package-lock.json` |
| @playwright/test | npm | 1.65.1 | ? | ? | `web/package-lock.json` |
| @tailwindcss/vite | npm | 4.3.5 | ? | ? | `web/package-lock.json` |
| @tanstack/react-query | npm | 5.108.2 | ? | ? | `web/package-lock.json` |
| @tanstack/react-router | npm | 1.170.46 | ? | ? | `web/package-lock.json` |
| @tanstack/router-plugin | npm | 1.169.0 | ? | ? | `web/package-lock.json` |
| @testing-library/dom | npm | 10.4.2 | ? | ? | `web/package-lock.json` |
| @testing-library/jest-dom | npm | 8.0.0 | ? | ? | `web/package-lock.json` |
| @testing-library/react | npm | 16.3.3 | ? | ? | `web/package-lock.json` |
| @types/node | npm | 26.6.6 | ? | ? | `web/package-lock.json` |
| @types/react | npm | 19.3.0 | ? | ? | `web/package-lock.json` |
| @types/react-dom | npm | 19.3.0 | ? | ? | `web/package-lock.json` |
| @vitejs/plugin-react | npm | 6.3.0 | ? | ? | `web/package-lock.json` |
| class-variance-authority | npm | 0.7.1 | ? | ? | `web/package-lock.json` |
| clsx | npm | 2.1.1 | ? | ? | `web/package-lock.json` |
| jsdom | npm | 31.0.1 | ? | ? | `web/package-lock.json` |
| lucide-react | npm | 1.77.0 | ? | ? | `web/package-lock.json` |
| msw | npm | 3.0.7 | ? | ? | `web/package-lock.json` |
| openapi-fetch | npm | 0.17.0 | ? | ? | `web/package-lock.json` |
| openapi-typescript | npm | 7.13.0 | ? | ? | `web/package-lock.json` |
| oxlint | npm | 1.94.0 | ? | ? | `web/package-lock.json` |
| radix-ui | npm | 1.8.1 | ? | ? | `web/package-lock.json` |
| react | npm | 19.3.1 | ? | ? | `web/package-lock.json` |
| react-dom | npm | 19.3.1 | ? | ? | `web/package-lock.json` |
| tailwind-merge | npm | 3.7.1 | ? | ? | `web/package-lock.json` |
| tailwindcss | npm | 4.3.5 | ? | ? | `web/package-lock.json` |
| typescript | npm | 5.9.3 | ? | ? | `web/package-lock.json` |
| vite | npm | 8.3.8 | ? | ? | `web/package-lock.json` |
| vitest | npm | 5.1.1 | ? | ? | `web/package-lock.json` |
| react | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/cards.card.html` |
| react-dom | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/cards.card.html` |
| standalone | npm | 7.29.0 | ? | ? | `docs/design-system/specimens/components/cards.card.html` |
| react | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/core.card.html` |
| react-dom | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/core.card.html` |
| standalone | npm | 7.29.0 | ? | ? | `docs/design-system/specimens/components/core.card.html` |
| react | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/data.card.html` |
| react-dom | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/data.card.html` |
| standalone | npm | 7.29.0 | ? | ? | `docs/design-system/specimens/components/data.card.html` |
| react | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/forms.card.html` |
| react-dom | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/forms.card.html` |
| standalone | npm | 7.29.0 | ? | ? | `docs/design-system/specimens/components/forms.card.html` |
| react | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/navigation.card.html` |
| react-dom | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/navigation.card.html` |
| standalone | npm | 7.29.0 | ? | ? | `docs/design-system/specimens/components/navigation.card.html` |
| react | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/overlay.card.html` |
| react-dom | npm | 18.3.1 | ? | ? | `docs/design-system/specimens/components/overlay.card.html` |
| standalone | npm | 7.29.0 | ? | ? | `docs/design-system/specimens/components/overlay.card.html` |
| react | npm | 18.3.1 | ? | ? | `prototypes/dashboard/2fa.html` |
| react-dom | npm | 18.3.1 | ? | ? | `prototypes/dashboard/2fa.html` |
| standalone | npm | 7.29.0 | ? | ? | `prototypes/dashboard/2fa.html` |
| react | npm | 18.3.1 | ? | ? | `prototypes/dashboard/404.html` |
| react-dom | npm | 18.3.1 | ? | ? | `prototypes/dashboard/404.html` |
| standalone | npm | 7.29.0 | ? | ? | `prototypes/dashboard/404.html` |
| react | npm | 18.3.1 | ? | ? | `prototypes/dashboard/index.html` |
| react-dom | npm | 18.3.1 | ? | ? | `prototypes/dashboard/index.html` |
| standalone | npm | 7.29.0 | ? | ? | `prototypes/dashboard/index.html` |
| d3 | npm | 7.9.0 | ? | ? | `prototypes/dashboard/index.html` |
| topojson-client | npm | 3.1.0 | ? | ? | `prototypes/dashboard/index.html` |
| react | npm | 18.3.1 | ? | ? | `prototypes/dashboard/install.html` |
| react-dom | npm | 18.3.1 | ? | ? | `prototypes/dashboard/install.html` |
| standalone | npm | 7.29.0 | ? | ? | `prototypes/dashboard/install.html` |
| react | npm | 18.3.1 | ? | ? | `prototypes/dashboard/login.html` |
| react-dom | npm | 18.3.1 | ? | ? | `prototypes/dashboard/login.html` |
| standalone | npm | 7.29.0 | ? | ? | `prototypes/dashboard/login.html` |
| react | npm | 18.3.1 | ? | ? | `prototypes/dashboard/register.html` |
| react-dom | npm | 18.3.1 | ? | ? | `prototypes/dashboard/register.html` |
| standalone | npm | 7.29.0 | ? | ? | `prototypes/dashboard/register.html` |

## À remplacer (déprécié, abandonné, compromis)

- rien

## À vérifier sur le web (agent)

OSV.dev ne couvre pas tout : pour chaque ligne, chercher les avis de sécurité, le changelog et l'historique des failles, puis noter la conclusion dans `.agents/memory/decisions.md`.

- Node.js 22 (`crates/agent/tests/fixtures/node-app/package.json`) : avis de sécurité de la branche 22, failles marquantes passées (ex. CVE-2024-27980 (injection de commande via child_process sous Windows))
- API `cdn.jsdelivr.net` (appelée dans `prototypes/dashboard/Security.jsx`) : incidents de sécurité, version d'API dépréciée, page de statut

## Crates non maintenues acceptées (réexamen du 2026-11-25)

`cargo audit` signale 7 avis « unmaintained » (7 crates, 7 identifiants distincts, sortie « 7 allowed warnings found ») ; `cargo deny check` reste vert (`unmaintained = "workspace"` : seules les crates directes comptent, aucune n'est concernée). Vérification sur crates.io : `pingora-core` 0.9.1 est la dernière version, `age` 0.12.1 et `testcontainers` 0.28.0 gardent les mêmes dépendances signalées ; aucune montée n'apporte donc de gain. Pingora reste en 0.9.x (le proxy s'appuie sur ses internes).

| Crate | Avis | Tirée par | Nature | Décision |
|---|---|---|---|---|
| chrono 0.4.45 | RUSTSEC-2026-0543 | pingora-core 0.9.1 | exécution (proxy) | acceptée, pas de remplaçant dans Pingora |
| unsafe-libyaml 0.2.11 | RUSTSEC-2024-0449 | serde_yaml 0.9.34+deprecated, tirée par pingora-core 0.9.1 (serde_yaml n'a pas d'avis propre) | exécution (proxy, configuration YAML de Pingora, jamais alimentée par un tiers) | acceptée |
| derivative 2.2.0 | RUSTSEC-2024-0388 | pingora-core 0.9.1 | macro de compilation | acceptée |
| rustls-pemfile 2.2.0 | RUSTSEC-2025-0134 | pingora-rustls 0.9.1 | exécution (proxy) | acceptée |
| basic-toml 0.1.10 | RUSTSEC-2026-0374 | age 0.11.5 (i18n-config) | macro de compilation | acceptée |
| proc-macro-error2 2.0.1 | RUSTSEC-2026-0173 | age 0.11.5 (i18n-embed-fl) | macro de compilation | acceptée |
| astral-tokio-tar 0.6.4 | RUSTSEC-2026-0440 | testcontainers 0.27.3 | dev-dependency (tests) | acceptée |

À réexaminer à chaque montée de `pingora`, `age` ou `testcontainers`. Le choix est consigné dans `.agents/memory/decisions.md` et `deny.toml`.
