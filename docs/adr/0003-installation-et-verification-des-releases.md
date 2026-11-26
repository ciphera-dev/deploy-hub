# ADR 0003 — Installation par curl et vérification des releases

- Statut : accepté (décision utilisateur)
- Date : 2026-11-21

## Contexte

La version auto-hébergée, la seule qui existe, doit s'installer en une commande, comme Coolify et Dokploy. `curl | sh` est un acte de confiance : un binaire altéré, un miroir compromis ou un téléchargement coupé doivent être détectés **avant** toute exécution.

## Décision

**Commande publique :**
```sh
curl -fsSL https://get.ciphera.ch/install.sh | sh
```

**Script `install/install.sh`** (POSIX sh, publié avec chaque release) :
- `set -eu`.
- Tout le code est dans une fonction `main` appelée à la **dernière ligne** : un téléchargement coupé ne s'exécute pas à moitié.
- `curl --proto '=https' --tlsv1.2` uniquement.
- Travail dans un `mktemp -d`, nettoyé par `trap`.
- `sudo` demandé explicitement.

**Étapes d'installation :**
1. Détecter l'OS et l'architecture : `x86_64-unknown-linux-musl` ou `aarch64-unknown-linux-musl`.
2. Télécharger `ciphera-<version>-<target>.tar.gz`, `SHA256SUMS` et `SHA256SUMS.minisig`.
3. Vérifier la signature **minisign** de `SHA256SUMS`, avec la clé publique **intégrée au script** et publiée dans `install/ciphera.pub`.
4. Vérifier le **SHA-256** de l'archive.
5. Si une seule vérification échoue, arrêter **sans rien installer**.
6. Installer les binaires et les unités systemd (`deploy/systemd/`), puis démarrer les services.
7. Afficher l'URL de l'assistant `routes/_setup/install.tsx`, avec un **jeton de setup à usage unique**. Le jeton est désactivé après la création du premier admin (`POST /api/setup/complete`).

**Production des releases :**
- La CI (`.github/workflows/release.yml`) produit des binaires musl statiques multi-arch, puis `SHA256SUMS`.
- Elle signe avec minisign. La clé vit dans un secret CI, dans un environnement protégé, **jamais dans le dépôt**.
- En complément : signature **cosign keyless** et attestation SLSA.
- `install.sh` est lui-même signé (`install.sh.minisig`).

**Mode prudent documenté :** télécharger le script, le lire, vérifier sa signature, puis l'exécuter.

**Tests :**
- shellcheck en CI.
- Test de bout en bout dans des conteneurs Debian, Ubuntu et Fedora (amd64/arm64). Le script doit installer quand tout est valide, et refuser d'installer avec un checksum altéré ou une signature invalide.

**Note (mises à jour, audit 2026-11-24) :** `ciphera-cli updater` applique les mêmes vérifications aux mises à jour.
- Téléchargements bornés : 512 Mio et 10 min pour une archive (release, outils tiers), 1 Mio et 2 min pour `SHA256SUMS` et sa signature. La taille est comptée par l'updater lui-même, pas seulement annoncée par le serveur : au-delà, abandon avant toute modification.
- Les demandes du panneau sont signées par HMAC avec une clé propre, `/etc/ciphera/update-hmac.key` : 32 octets aléatoires, `root:ciphera 0640`, créés par `ciphera init` ou, s'ils manquent, par l'updater. Le plan de contrôle signe avec, l'updater vérifie avec ; l'updater ne lit jamais l'identité age `age.key`.
- La sauvegarde décompressée utilisée par un retour arrière (`/var/lib/ciphera-updater/restore.sql`) est réécrite à zéro puis supprimée à la fin du retour arrière, qu'il ait réussi ou échoué ; son chemin est journalisé.

## Conséquences

- (+) Installation en une commande ; une altération est détectée avant toute exécution.
- (−) La clé minisign est à garder, à faire tourner et à révoquer si besoin (`docs/security/release-signing.md`, à rédiger).
- (−) `curl | sh` reste un acte de confiance envers le domaine `get.ciphera.ch`, d'où le mode prudent documenté.

## Alternatives écartées

- **Paquets .deb/.rpm seuls** : trop de cibles à maintenir au départ (prévus plus tard).
- **Image Docker « tout-en-un »** : contraire au modèle « binaire + agent natif ».
- **Checksum sans signature** : il ne protège pas si le serveur de téléchargement est compromis.
- **Cosign seul** : il exige l'outil cosign sur la machine cible.

## Note : durcissement du script d'installation

- Les surcharges de test (adresses de release et d'outils, clé de test) sont dans des blocs `# >>> mode-test` / `# <<< mode-test` de `install/install.sh`. `install/release-script.sh` les retire du script publié et refuse de le produire s'il y reste une mention de `CIPHERA_INSTALL_TEST` : le script signé n'a aucun mode test.
- `ciphera-cli` suit la même règle : son mode test (`CIPHERA_INSTALL_TEST`, `CIPHERA_TEST_MINISIGN_PUBKEY`, autorité de test de curl) n'est compilé qu'avec la fonctionnalité `install-test`, absente par défaut. Le binaire livré, signé et publié n'a aucun mode test : `release.yml` construit à part `ciphera-cli-test` (artefact interne, jamais signé ni publié) pour les tests de bout en bout, et refuse de signer une archive dont un binaire contient `CIPHERA_INSTALL_TEST` ou `CIPHERA_TEST_`.
- Les téléchargements ont un temps maximal (30 s pour les petits fichiers, 600 s pour les archives) et une taille maximale (1 Mio, 512 Mio).
- Un compte système déjà présent (`ciphera`, `ciphera-agent`, etc.) n'est accepté que s'il a un uid inférieur à `UID_MIN`, un shell `nologin` ou `false` et un groupe principal du même nom ; sinon l'installation s'arrête avant tout téléchargement.
- `uninstall.sh` sans `--purge` ne retire que les conteneurs d'infrastructure (PostgreSQL, buildkitd et leurs aides) ; les conteneurs d'application des utilisateurs sont conservés et listés.
