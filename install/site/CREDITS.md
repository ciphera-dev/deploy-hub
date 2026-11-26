# Site get.ciphera.ch : sources des fichiers

Fichiers statiques de la page d'accueil et de la page 404 de `get.ciphera.ch`, publiés par `install/pages-site.sh` (dossier `assets/` copié tel quel à la racine du site ; `index.html` et `404.html` reçoivent la version, le dépôt et le domaine). Ce fichier n'est pas publié sur le site.

## Logos des technologies (`assets/icons/`)

Source : [Simple Icons](https://simpleicons.org), dossier `icons/` (branche `develop`, récupérés le 2026-11-27). Seule modification : l'attribut `fill="#8a8a8a"` (gris du design system) ajouté sur la balise `<svg>` ; les tracés sont ceux de la source, sans changement.

| Fichier | Marque | Fichier d'origine (SHA-256) |
|---|---|---|
| `debian.svg` | Debian | `8835536d63c80990f9c90c935982ba329d163e707c894278ca6bd73258d1962b` |
| `docker.svg` | Docker | `e91474e8e2ee0af54cb42104a0616625f678479a91c4a2bcbd2284352d78b57d` |
| `fedora.svg` | Fedora | `c234757373cab5ed80d7b2df8ff26ea039bcec16a9e9a47adbd29f5d0e94489a` |
| `github.svg` | GitHub | `3bf8cceead820aec50d4ee825a3fd02c5a1cd6665cc9cf4cbf3d9c8861a204bb` |
| `letsencrypt.svg` | Let's Encrypt | `1307c2a85ba8ea8fb6e738d2eef463eadfce19b1b1295084daff4663c4fdbddd` |
| `postgresql.svg` | PostgreSQL | `c8ccadbd88c9312c2ad3734c4861b31d3485ce179ee91d64793143f83cfa745c` |
| `react.svg` | React | `ab9136a1a4a8be72ac6743e1bdd1912d878bd68d57787ea7a6c6838e3b23c01c` |
| `rust.svg` | Rust | `95a291d8887610d103704adcc5c5f2cbfa4c80a96f8e72b00531d5691beb67a8` |
| `typescript.svg` | TypeScript | `7f023e8d1e767370306ec9437d2de5d5af97491d6208dd7799a3808a7c10a36a` |
| `ubuntu.svg` | Ubuntu | `b0aca674c881a07bb60b26afbcee07093ae419e29ee38e376f938817fa66ae03` |
| `webauthn.svg` | WebAuthn | `f3b35ce22669bcec7e012be47f648f5d49c94fa914902c0f5882524c802009c6` |

Les fichiers SVG de Simple Icons sont publiés sous [CC0 1.0](https://github.com/simple-icons/simple-icons/blob/develop/LICENSE.md). CC0 ne couvre pas le droit des marques : chaque marque appartient à son propriétaire. Leur présence indique seulement une technologie utilisée par Ciphera (voir la section « Architecture » du README) ou un système pris en charge par l'installeur ; elle ne signifie ni partenariat, ni parrainage, ni approbation.

## Polices (`assets/fonts/`)

Geist et Geist Mono, les polices du design system, copiées sans modification depuis les paquets npm déjà installés pour le panneau (`web/node_modules/@fontsource/geist` et `@fontsource/geist-mono`, version 5.3.0, dossier `files/`) : sous-ensemble latin seulement (suffit au français), graisses utilisées par le site seulement. Chargées par `@font-face` local (`font-display: swap`), aucune requête tierce. Licence : SIL Open Font License 1.1, copiée dans `assets/fonts/OFL.txt` (avec les deux mentions de copyright des paquets) et publiée à côté des fichiers.

| Fichier | Police | SHA-256 |
|---|---|---|
| `geist-latin-400-normal.woff2` | Geist 400 | `ead637fd0b6b887d829b3ce3f25fdc242b1de0cfb69c0d97987933675d1315ba` |
| `geist-latin-500-normal.woff2` | Geist 500 | `6145bf6706dbc1b6686f04f871489cf724951f00b752745896ccdcac07639a53` |
| `geist-mono-latin-400-normal.woff2` | Geist Mono 400 | `3f98383b122fe015a48536cd4a1cda855a201718923ffe74931a01597107b9b5` |

Après une mise à jour de `@fontsource/geist*` dans `web/`, recopier les mêmes fichiers et mettre à jour ce tableau.

## Fichiers publiés

`pages-site.sh` ne publie de `assets/` que des dossiers et fichiers ordinaires aux extensions `css`, `js`, `webp`, `png`, `svg`, `woff2` et `txt` (licence des polices), sans nom caché ni lien ; tout autre fichier arrête la construction du site.

## Icônes de l'interface (dans `index.html`)

Tracés des icônes [Lucide](https://lucide.dev) (rocket, hammer, globe, lock-keyhole, activity, users, mail, wrench, file-signature, network, key-round, shield-check, copy, check), tirés du paquet `lucide-react` 1.77.0 déjà utilisé par le panneau, insérés en SVG en ligne. Licence ISC, © Lucide Icons and Contributors.

## Capture du panneau (`assets/dashboard.webp`)

Capture du panneau réel (`web/`, page Vue d'ensemble, thème sombre) servi par `vite` en développement avec l'API simulée MSW (`VITE_MSW=1`, données factices de `web/src/mocks/fixtures/`), à 1440 × 900 px et une densité de 1,5. Les entrées de menu prévues pour des versions ultérieures (« Bientôt ») sont masquées pour la capture : l'image ne montre que des écrans livrés. Conversion en WebP par Chromium (canvas), sans métadonnées. Aucune donnée réelle. La même image sert dans le hero et dans la carte « Le panneau » de la section « Comment ça marche ».

Pour la refaire après un changement du panneau : lancer `VITE_MSW=1 npx vite` dans `web/`, ouvrir `/` à 1440 × 900 (densité 1,5, thème sombre, fuseau Europe/Zurich, horloge fixée un matin pour le message « Bonjour »), masquer `[aria-disabled="true"]`, capturer, puis convertir en WebP (qualité 0,8).

## Logo de Ciphera

`assets/brand/` n'est pas versionné ici : `pages-site.sh` y copie `web/src/assets/brand/logo.svg` et `web/public/favicon.svg`, les fichiers officiels du design system.
