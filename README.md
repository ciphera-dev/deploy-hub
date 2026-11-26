<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="web/src/assets/brand/logo.svg">
  <source media="(prefers-color-scheme: light)" srcset="web/src/assets/brand/logo-dark.svg">
  <img alt="Ciphera" src="web/src/assets/brand/logo-dark.svg" width="360">
</picture>

<h3>Le PaaS auto-hébergé, léger et sûr, écrit en Rust.</h3>

<p>Déployez vos applications depuis GitHub sur vos propres serveurs, avec HTTPS automatique, sans offre cloud ni fonctionnalité sous licence.</p>

<p>
  <a href="https://github.com/ciphera-dev/deploy-hub/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/ciphera-dev/deploy-hub/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/ciphera-dev/deploy-hub/releases"><img alt="Dernière release" src="https://img.shields.io/github/v/release/ciphera-dev/deploy-hub?label=release"></a>
  <a href="LICENSE"><img alt="Licence AGPL-3.0" src="https://img.shields.io/github/license/ciphera-dev/deploy-hub"></a>
</p>

<p>
  <a href="#installation">Installation</a> ·
  <a href="#fonctionnalités">Fonctionnalités</a> ·
  <a href="#documentation">Documentation</a> ·
  <a href="CONTRIBUTING.md">Contribuer</a> ·
  <a href="SECURITY.md">Sécurité</a>
</p>

</div>

<br>

## Qu'est-ce que Ciphera ?

Ciphera est une plateforme de déploiement (PaaS) **auto-hébergée et entièrement open source**. Il occupe la même place que Dokploy ou Coolify, avec deux priorités : rester léger et être sûr par défaut.

- **Un seul binaire** pour le plan de contrôle, qui sert aussi le panneau web.
- **Un agent natif** sur le nœud, relié au plan de contrôle par QUIC et mTLS, avec une API étroite : jamais d'exécution arbitraire.
- **Un proxy** Pingora avec certificats Let's Encrypt (ACME).
- **Des builds** BuildKit : Railpack, Dockerfile ou Compose.

Il n'existe qu'une version de Ciphera : celle que vous installez sur vos serveurs.

## Fonctionnalités

| Domaine | Ce qui est disponible |
|---|---|
| **Déploiement** | Projets et services reliés à un dépôt GitHub (GitHub App créée depuis le panneau), déploiement à chaque push, déploiement manuel, deploy hooks à expiration, annulation et rollback, redémarrage et arrêt des services |
| **Builds** | Railpack sans configuration, Dockerfile ou Compose, un buildkitd isolé par projet, limites de taille, de mémoire et de durée ([détail](docs/operations/builds.md)) |
| **Domaines et HTTPS** | Domaines par projet avec vérification, certificats Let's Encrypt émis et renouvelés par le proxy, domaine principal et redirections, domaine dédié au panneau avec HSTS |
| **Variables et secrets** | Variables par projet et par environnement, import au format `.env`, variables globales, secrets chiffrés avec `age`, jamais renvoyés par l'API et masqués dans les journaux |
| **Suivi** | Vue d'ensemble, métriques des nœuds, historique des déploiements, journaux de build et d'exécution en direct (WebSocket) |
| **Équipe et accès** | Invitations, rôles (propriétaire, admin, développeur, ops, lecteur), passkeys (WebAuthn), TOTP et codes de secours, confirmation renforcée des actions sensibles, liste d'IP autorisées, sessions révocables |
| **Notifications** | E-mails par votre serveur SMTP (connexion chiffrée obligatoire), préférences par membre, avis de sécurité aux propriétaires |
| **Exploitation** | Installeur à releases signées (minisign), mise à jour vérifiée avec retour arrière, vérification de nouvelle version désactivée par défaut et sans télémétrie, export de la configuration sans secret, commandes `ciphera admin` de secours |

## Installation

Sur un serveur Linux avec systemd (Debian, Ubuntu ou Fedora ; x86_64 ou aarch64) :

```sh
curl -fsSL https://get.ciphera.ch/install.sh | sudo sh
```

L'installeur vérifie la signature minisign puis le SHA-256 de la release avant d'installer quoi que ce soit, puis affiche l'adresse de l'assistant d'installation (`https://<IP>:8443/setup#token=…`, valable 24 heures). Ouvrez-la pour créer le premier compte.

Pour aller plus loin :

- [Installation détaillée](docs/operations/installation.md) : étapes de l'installeur, sortie réseau des conteneurs, accès au panneau par un nom DNS, domaine du panneau, HSTS, passkeys, commandes d'administration ;
- [Vérifier une release](docs/security/release-signing.md) : minisign, cosign et attestation SLSA ;
- [Certificats de l'instance](docs/operations/certificats.md) : autorité locale, renouvellement, rotation.

## Architecture

| Couche | Choix |
|---|---|
| Panneau | React 19, Vite, TypeScript, TanStack Router et Query, Tailwind CSS v4, shadcn/ui ; SPA statique embarquée dans le binaire (`rust-embed`) |
| Plan de contrôle | Axum (REST et WebSocket), PostgreSQL 17 et SQLx, file de tâches dans PostgreSQL (`SKIP LOCKED`), sans Redis ; schéma OpenAPI généré (`utoipa`) |
| Authentification | Mots de passe Argon2, passkeys (`webauthn-rs`), TOTP, rôles par organisation |
| Secrets | Chiffrement avec `age`, champs en écriture seule dans l'API |
| Agent de nœud | Tokio, QUIC (`quinn`) avec mTLS obligatoire (PKI de l'instance, `rcgen`), Docker via `bollard`, état local SQLite |
| Builds | BuildKit piloté par l'agent : Railpack, Dockerfile, Compose |
| Proxy et TLS | Pingora, `instant-acme`, rustls |
| Outils serveur | `ciphera admin` (secours) et `ciphera-cli updater` (mises à jour signées) |

Les raisons de ces choix sont dans [docs/architecture/stack.md](docs/architecture/stack.md) et les [décisions d'architecture](docs/adr/).

<details>
<summary><strong>Arborescence du dépôt</strong></summary>

```
.
├── crates/
│   ├── control-plane/   plan de contrôle et API (binaire `ciphera`)
│   ├── agent/           agent de nœud (builds, déploiements, métriques)
│   ├── proxy/           proxy Pingora et ACME
│   ├── cli/             mises à jour signées (`ciphera-cli`)
│   ├── proto/           protocole agent ⇄ plan de contrôle
│   ├── pki/             autorité de certification de l'instance
│   ├── api-types/       types de l'API (OpenAPI)
│   └── shared/          code commun
├── web/                 panneau (SPA React) et tests Playwright
├── install/             installeur, scripts de release et leurs tests
├── deploy/              unités systemd, profils AppArmor, images épinglées
├── scripts/             outils de release
├── docs/                architecture, ADR, sécurité, exploitation, design
├── prototypes/          maquettes d'origine (référence visuelle uniquement)
└── openapi.json         schéma de l'API, régénéré par `just gen`
```

</details>

## Documentation

| Sujet | Document |
|---|---|
| Installation et exposition d'une instance | [docs/operations/installation.md](docs/operations/installation.md) |
| Règles et limites des builds | [docs/operations/builds.md](docs/operations/builds.md) |
| Certificats de l'instance | [docs/operations/certificats.md](docs/operations/certificats.md) |
| Publier une release | [docs/operations/release.md](docs/operations/release.md) |
| Signature et vérification des releases | [docs/security/release-signing.md](docs/security/release-signing.md) |
| Modèle de menaces | [docs/security/threat-model.md](docs/security/threat-model.md) |
| Stack et contrat backend | [docs/architecture/stack.md](docs/architecture/stack.md) |
| Décisions d'architecture | [docs/adr/](docs/adr/) |
| Écrans du panneau (écran → route → endpoint) | [docs/design/screens.md](docs/design/screens.md) |
| Design system | [docs/design-system/README.md](docs/design-system/README.md) |

## Contribuer

Les contributions sont bienvenues. Prérequis, commandes qui reproduisent la CI (`just test`, `just lint`, `just audit`, `just gen`), tests de bout en bout et conventions : voir [CONTRIBUTING.md](CONTRIBUTING.md). La publication d'une version suit [docs/operations/release.md](docs/operations/release.md).

Ce projet adopte un [code de conduite](CODE_OF_CONDUCT.md) : en participant, vous vous engagez à le respecter.

## Sécurité

Ne signalez jamais une faille par une issue publique : suivez [SECURITY.md](SECURITY.md) (signalement privé par GitHub Security Advisories).

## Remerciements

Le développement de Ciphera a été réalisé avec l'aide de deux assistants de programmation : **Kimi**, puis **Claude Code**.

<div align="center">
<table>
  <tr>
    <td align="center" width="200">
      <a href="https://www.kimi.com">
        <picture>
          <source media="(prefers-color-scheme: dark)" srcset="docs/assets/brand/kimi-dark.svg">
          <source media="(prefers-color-scheme: light)" srcset="docs/assets/brand/kimi-light.svg">
          <img alt="Kimi" src="docs/assets/brand/kimi-light.svg" height="48">
        </picture>
      </a>
      <br><br>
      <strong>Kimi</strong><br>
      <sub>Moonshot AI</sub>
    </td>
    <td align="center" width="200">
      <a href="https://claude.com/claude-code">
        <img alt="Claude Code" src="docs/assets/brand/claude.svg" height="48">
      </a>
      <br><br>
      <strong>Claude Code</strong><br>
      <sub>Anthropic</sub>
    </td>
  </tr>
</table>
</div>

<sub>Logos : <a href="https://simpleicons.org">Simple Icons</a> (CC0). Les marques citées appartiennent à leurs propriétaires ; sources et conditions dans <a href="docs/assets/brand/CREDITS.md">docs/assets/brand/CREDITS.md</a>.</sub>

## Licence

Ciphera est distribué sous la **GNU Affero General Public License v3.0** ([LICENSE](LICENSE)). Quiconque propose une version modifiée de Ciphera comme service réseau doit en publier le code source.
