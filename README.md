# Ciphera

Ciphera est un PaaS auto-hébergé et **entièrement open source**, écrit en Rust. Il vise la même place que Dokploy et Coolify, en plus léger et plus sûr :
- **Un seul binaire** pour le plan de contrôle, qui sert aussi l'interface.
- **Un agent natif** sur chaque nœud.
- **Un proxy** Pingora + ACME.
- **Des builds** BuildKit/Railpack.

Il n'existe qu'une version : celle que vous installez sur vos serveurs. Pas d'offre cloud et pas de fonctionnalités sous licence.

> **État : pré-production.** Le design (maquettes complètes du panneau) est rangé et prêt à être porté. Le code de la v0.1 est dans `crates/` et `web/` : voir « Démarrer le développement » et [CONTRIBUTING.md](CONTRIBUTING.md).

## Installation (cible)

```sh
curl -fsSL https://get.ciphera.ch/install.sh | sh
```

Le script :
1. télécharge l'archive de la release et `SHA256SUMS` ;
2. vérifie la signature minisign puis le SHA-256 ;
3. s'arrête sans rien installer si une vérification échoue ;
4. installe les binaires et les unités systemd ;
5. affiche l'URL de l'assistant d'installation avec un jeton à usage unique.

Le détail est dans [docs/adr/0003-installation-et-verification-des-releases.md](docs/adr/0003-installation-et-verification-des-releases.md).

Sortie réseau des conteneurs : les applications ne joignent ni l'hôte lui-même (sauf les ports 80 et 443 du proxy du nœud, pour un domaine servi par ce même serveur) ni les adresses de métadonnées des clouds (169.254.169.254 et le reste de 169.254.0.0/16, `fd00:ec2::254`, `fd20:ce::254`, 100.100.100.200, 168.63.129.16) ; les builds ne joignent en plus aucune plage privée. Les plages privées (10/8, 172.16/12, 192.168/16, 100.64/10, `fc00::/7`) restent permises aux applications par défaut. Pour les interdire :

```sh
curl -fsSL https://get.ciphera.ch/install.sh | sudo CIPHERA_APP_EGRESS_DENY_PRIVATE=1 sh
```

Le réglage est gardé dans `/etc/ciphera-egress/egress.env` (root, `CIPHERA_APP_EGRESS_DENY_PRIVATE=0` ou `1`) : après une modification à la main, il s'applique en 5 minutes au plus (`ciphera-egress-check.timer`) ou tout de suite par `sudo systemctl start ciphera-egress-check.service`.

### Accès au panneau par un nom DNS

L'accès direct au panneau (`https://<IP>:8443`, certificat auto-signé) répond par son adresse IP et par `localhost`. Pour le joindre sur ce port par un nom DNS, déclarez ce nom dans `/etc/ciphera/ciphera.env`, puis redémarrez le service `ciphera` :

```sh
CIPHERA_PANEL_ALIASES=admin.exemple.com,ops.exemple.com
```

Les noms reconnus sur le port 8443 sont l'hôte de `CIPHERA_PUBLIC_URL`, `CIPHERA_WEBAUTHN_RP_ID`, ces alias et le domaine du panneau confirmé. Tout autre nom reçoit `421 Misdirected Request`. Un nom internationalisé se déclare sous sa forme ASCII (punycode `xn--…`). Un nom réservé au panneau ne peut pas devenir le domaine d'un projet, et un domaine de projet ne peut pas être créé ni vérifié sous le nom par lequel on joint le panneau.

Une fois le domaine du panneau confirmé (Paramètres › Domaine du panel), l'adresse IP du serveur ne mène plus au panneau : le port 8443 est fermé, et le proxy répond `404` à une requête HTTP ou HTTPS adressée à l'IP, sans révéler le domaine. Il en va de même pour le nom éventuellement déclaré dans `CIPHERA_PROXY_PANEL_HOST`, et l'écouteur facultatif `CIPHERA_HTTP_PORT` ne redirige plus que le domaine du panneau et les noms réservés au panneau (tout autre nom et toute adresse IP reçoivent `404`). Un nom réservé qui se déduit de l'IP (un nom `sslip.io`, le nom inverse PTR du serveur, etc.) reste redirigé vers le domaine du panneau : déclarer un tel nom dans `CIPHERA_PUBLIC_URL`, `CIPHERA_WEBAUTHN_RP_ID` ou les alias est un choix de l'opérateur, qui révèle alors le domaine à qui connaît l'IP. Prenez l'habitude d'ouvrir le panneau par son domaine avant de confirmer. Si le domaine ne répond plus, `sudo ciphera admin reset-panel-domain` rouvre l'accès direct au redémarrage du service. Une connexion TLS sans nom (SNI) ne reçoit jamais le certificat du panneau.

HSTS (un an, jamais `preload`) couvre les sous-domaines du domaine du panneau quand il n'est pas à la racine de son domaine (`panel.exemple.com`). Pour un domaine racine (`exemple.com`), l'option « Inclure les sous-domaines (HSTS) » est désactivée par défaut : activée, elle impose HTTPS pendant un an à tous les sous-domaines de la zone, y compris ceux servis ailleurs.

Les passkeys sont liées à l'origine publique du panneau (domaine confirmé, sinon `CIPHERA_PUBLIC_URL`). Si ce domaine change, les passkeys enregistrées sous l'ancien ne fonctionnent plus : il faut les enregistrer à nouveau. Elles ne servent qu'à cette adresse exacte (même hôte, même port) : par l'adresse IP, par `localhost`, par un alias, par un sous-domaine ou par un autre port, le panneau ne les propose pas et indique l'adresse publique ; on s'y connecte alors par mot de passe puis code TOTP ou code de secours. Pour un autre choix, fixez `CIPHERA_WEBAUTHN_RP_ID` et `CIPHERA_WEBAUTHN_RP_ORIGIN`.

## Stack

| Couche | Choix |
|---|---|
| Interface | React 19 + Vite + TypeScript + TanStack Router/Query + Tailwind v4 + shadcn/ui. SPA statique embarquée dans le binaire via `rust-embed` |
| Plan de contrôle | Axum (REST + WebSocket), PostgreSQL 17 + SQLx, files de tâches en Postgres (`SKIP LOCKED` / apalis), sans Redis |
| Authentification | Passkeys/WebAuthn (`webauthn-rs`), TOTP, jetons API à permissions |
| Secrets | Chiffrement enveloppe avec `age`, champs en écriture seule dans l'API |
| Observabilité | OpenTelemetry + export Prometheus |
| Agent de nœud | Tokio + QUIC (`quinn`), mTLS obligatoire (mini-PKI `rcgen`), Docker via `bollard`, état local SQLite (`rusqlite`). API étroite, jamais d'exécution arbitraire |
| Builds | BuildKit (`buildkitd`) piloté par l'agent : Railpack par défaut, Dockerfile, Compose |
| Proxy / TLS | Pingora + `instant-acme` |
| CLI | Rust (`clap`), client généré depuis le même schéma OpenAPI que l'interface |

Les raisons de ces choix sont dans [docs/architecture/stack.md](docs/architecture/stack.md) et [docs/adr/](docs/adr/).

Builds par Dockerfile (ou Compose) : la directive `# syntax=` n'accepte que `docker/dockerfile` en version `1`, `latest` ou `1.4` et plus (`1.N`, `1.N.M`), sans empreinte (`@sha256:…`) ni variante `-labs`. Un autre frontend, ou une version ancienne, pourrait ignorer le cloisonnement des caches de build par projet et par environnement. Le build échoue alors avec un message clair : retirer la ligne (frontend intégré à BuildKit) ou écrire `# syntax=docker/dockerfile:1`.

Limites des builds : un seul build à la fois par nœud (les suivants attendent leur tour, ce qui est indiqué dans leur journal) ; le dépôt cloné ne dépasse pas 2 Gio et l'image construite 10 Gio ; BuildKit est limité à 4096 processus, à la moitié de la mémoire du nœud (1 Gio au moins) et à tous les CPU sauf un, et chaque projet a son propre buildkitd et son propre état (le premier build d'un projet est plus lent, cache vide ; un seul buildkitd tourne à la fois, arrêté après 10 min d'inactivité, état supprimé avec le projet ou après 30 jours) ; le ramasse-miettes d'un état garde 10 % du disque (entre 5 et 50 Gio) divisés par le nombre de projets (1 Gio au moins chacun), donc le total des états peut dépasser 10 %. Un outil de build qui dépasse son délai est arrêté avec tous ses processus.

## Arborescence

```
.
├── web/                        interface (SPA React) — composants et tokens déjà présents
│   ├── public/                 favicon
│   └── src/
│       ├── design-system/      32 composants du design system (JSX + .d.ts + doc), à porter en TSX
│       ├── styles/             index.css + tokens/ (couleurs, typo, espacements, effets)
│       └── assets/             brand/ (logos Ciphera) + images/ (connecteurs, BDD, OS, SMTP, SSH, Cloudflare)
├── prototypes/                 maquettes d'origine, lançables (référence visuelle, jamais importées)
│   ├── dashboard/              ~45 écrans + login / register / 2fa / install / 404
│   └── vendor/ds-bundle.js     bundle généré du design system (pour les maquettes uniquement)
├── docs/
│   ├── adr/                    décisions d'architecture
│   ├── architecture/stack.md   stack, arborescence cible, contrat backend
│   ├── design/screens.md       contrat de production : écran → route → endpoint
│   ├── design-system/          fondations, specimens, captures de référence
│   ├── research/analyse-paas/  analyse de Dokploy et Coolify (fonctionnalités, CVE, blueprint)
│   ├── security/               fondations et exigences de sécurité
│   └── cleanup/                trace du rangement de l'export design (file-map.json, report.md)
├── config/                     règles oxlint d'adhérence au design system
├── .agents/                    mémoire multi-agents (lire .agents/memory/etat.md en premier)
├── AGENTS.md · CLAUDE.md       consignes pour les agents
└── (à créer) crates/  install/  deploy/  scripts/  justfile  Cargo.toml
```

## Voir les maquettes

Servir la **racine du dépôt** en HTTP (pas en `file://`, car Babel et les imports l'exigent), puis ouvrir `prototypes/dashboard/index.html` :

```sh
python -m http.server 8000
# → http://localhost:8000/prototypes/dashboard/index.html
#   aussi : login.html, register.html, 2fa.html, install.html, 404.html
```

Les maquettes chargent React 18 (dev) et Babel depuis unpkg. Elles servent de référence visuelle seulement, jamais de base de production.

## Démarrer le développement

Prérequis, commandes locales (`just dev`, `just gen`, `just test`, `just lint`) et conventions : voir [CONTRIBUTING.md](CONTRIBUTING.md). Pour signaler une faille en privé : [SECURITY.md](SECURITY.md).

## Tests de bout en bout (Playwright)

- `npm --prefix web run e2e` : la SPA construite servie par `vite preview` (aucune instance requise ; `golden-path.spec.ts` est ignoré).
- Parcours golden-path (`web/e2e/golden-path.spec.ts` : setup → TOTP → GitHub App simulée → déploiement → domaine via Pebble → HTTPS → rollback) : seulement sur une machine Linux jetable avec systemd et Docker (job CI `e2e-golden-path`). `install/tests/golden-path/setup.sh` y installe Ciphera, Pebble et un GitHub simulé, puis écrit les variables `CIPHERA_E2E_*` :

  ```sh
  sudo CIPHERA_DESTRUCTIVE_HOST_TEST=1 bash install/tests/golden-path/setup.sh /tmp/golden-path
  set -a; . /tmp/golden-path/e2e.env; set +a
  npm --prefix web run e2e -- golden-path
  ```

  Sans `CIPHERA_E2E_URL`, le parcours est sauté. Le script modifie la machine (`/etc/hosts`, magasin de certificats, unités systemd).

## Licence

Ciphera est distribué sous **GNU Affero General Public License v3.0** ([LICENSE](LICENSE)). Quiconque propose une version modifiée de Ciphera comme service réseau doit en publier le code source.
