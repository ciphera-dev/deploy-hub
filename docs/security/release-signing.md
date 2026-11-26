# Signature des releases : garde, rotation, révocation, compromission

Ce document complète l'ADR 0003 (installation et vérification des releases). La procédure de publication est dans [`docs/operations/release.md`](../operations/release.md).

## Ce qui protège une release

| Couche | Ce qu'elle prouve | Qui la vérifie |
|---|---|---|
| **minisign** (clés de confiance, Ed25519) | `SHA256SUMS` et `install.sh` viennent du détenteur d'une clé de confiance. | `install.sh` (clés injectées dans le script publié), `ciphera-cli updater` (clés intégrées au binaire depuis `install/ciphera.pub`), le job `pages-site`. |
| **SHA-256** (`SHA256SUMS`, `install.sh.sha256`) | L'archive téléchargée est celle qui a été signée. | `install.sh`, l'updater. |
| **cosign keyless** (`*.sigstore.json`) | Le fichier a été produit par `release.yml` de `ciphera-dev/deploy-hub` sur ce tag (certificat Fulcio lié à l'identité OIDC du workflow, inscrit dans le journal public Rekor). | Vérificateurs externes seulement (manuellement). |
| **Attestation SLSA** (`actions/attest`) | La provenance du build : dépôt, commit, workflow, runner. | Vérificateurs externes seulement (`gh attestation verify`). |

**Seul minisign protège réellement les installations.** `install.sh` et l'updater ne vérifient ni cosign ni l'attestation. Un attaquant qui détient une clé minisign de confiance et contrôle l'hébergement (`get.ciphera.ch` ou les releases) peut donc livrer du code aux serveurs, même si ses fichiers ne passeraient pas la vérification cosign. cosign et l'attestation servent à l'audit : quiconque vérifie à la main voit qu'un artefact ne vient pas du workflow attendu, et l'entrée Rekor d'un artefact frauduleux, s'il en existe une, est publique.

`releases/latest.json` (dernière version) est signé minisign par la même clé (`install/sign-latest.sh`). Le commentaire de confiance, couvert par la signature globale, porte la version et une échéance de 120 jours : `ciphera-latest version=<v> issued=<ts> expires=<ts + 120 jours>`. Le plan de contrôle et `ciphera-cli updater --latest` le vérifient avec les clés intégrées à leur binaire (`install/ciphera.pub`, jamais les clés retirées) : signature invalide, commentaire mal formé ou version différente de celle du commentaire, le fichier est ignoré et l'échec journalisé. Une échéance passée ne bloque rien : le panneau avertit qu'il ne peut pas garantir que Ciphera est à jour. `refresh-latest.yml` re-signe le fichier chaque mois (approbation de `release`). La mise à jour elle-même vérifie toujours la signature de la release visée, et l'updater refuse toute version inférieure ou égale à la version installée.

## Clés

| Clé | Où | Usage |
|---|---|---|
| Production (minisign) | **Secrète** : environnement GitHub `release` (`CIPHERA_MINISIGN_SECRET_KEY`, chiffrée par `CIPHERA_MINISIGN_PASSWORD`), plus 2 copies hors ligne. **Publique** : `install/ciphera.pub` (ensemble de confiance, 1 à 3 clés) et `https://get.ciphera.ch/ciphera.pub`. | Releases `v*` uniquement. |
| Test (minisign, id `5AA05FF500FABE9`) | **Secrète** : secret de dépôt `CIPHERA_TEST_MINISIGN_KEY`. **Publique** : `install/tests/ciphera-test.pub`. | Releases de test de `main`. Jamais acceptée en production. |
| cosign | Aucune clé durable : un certificat éphémère (10 min) est délivré au job `sign-release`. | — |

**Ensemble de confiance.** `install/ciphera.pub` peut contenir plusieurs clés : des paires « untrusted comment » + clé, mises bout à bout. `install.sh` publié (variable `CIPHERA_PUBKEYS`), l'updater, `sign-release.sh` et `pages-site.sh` acceptent une signature de **n'importe laquelle** de ces clés, et refusent toute autre.

Garde-fous automatiques :

- `install/install.sh` du dépôt ne contient **aucune** clé (marqueur `@CIPHERA_RELEASE_PUBKEYS@`) et refuse de s'exécuter hors mode test. Seul le script publié porte les clés : `install/release-script.sh` les y injecte depuis `install/ciphera.pub`.
- Production = **push** d'un tag `v*`. Un lancement manuel, même sur un tag, prend le chemin de test.
- Le job `sign-release` échoue dans ces cas :
  - le commit du tag n'est pas sur `main` ;
  - `install/ciphera.pub` contient la clé de test ;
  - les secrets manquent ;
  - aucune clé de l'ensemble ne vérifie la signature produite.
- Le site n'est construit que dans le run de release approuvé, à partir des fichiers signés de ce run. Il est déployé dans l'environnement `github-pages`, qui exige un relecteur. **Publier le site vaut une signature** : `install.sh` servi porte les clés de confiance. Ce droit est donc gardé comme la clé.
- Les surcharges de test (`CIPHERA_RELEASE_BASE_URL`, `CIPHERA_TOOLS_BASE_URL`, `CIPHERA_TEST_MINISIGN_PUBKEY`) sont refusées sans `CIPHERA_INSTALL_TEST=1`, dans `install.sh` comme dans l'updater (ses unités root ne lisent aucun fichier d'environnement). Elles restent dans le script publié pour deux raisons : les retirer ferait publier un script différent de celui testé de bout en bout, et quiconque peut fixer l'environnement d'un processus root contrôle déjà la machine.

## Garde de la clé de production

- La clé est créée par `scripts/release/new-signing-key.ps1`, lancé par la personne qui garde la clé, sur son poste. Jamais par un agent ni par la CI.
- **Mot de passe** : 16 caractères minimum, conservé uniquement dans un gestionnaire de mots de passe, jamais à côté du fichier de clé.
- **Clé secrète** (fichier chiffré) : deux copies hors ligne (clé USB chiffrée, coffre). Aucune copie ne reste sur le poste après la mise en place. Jamais dans le dépôt, un ticket, une conversation ou un outil d'IA.
- **Environnements protégés** :
  - `release` : au moins deux relecteurs obligatoires, *Prevent self-review*, déploiements limités aux tags `v*`. Seul `sign-release` lit ses secrets.
  - `github-pages` : mêmes relecteurs, *Prevent self-review*, tags `v*` seulement.
  - Les tags `v*` sont réservés aux administrateurs (ruleset). Le relecteur vérifie le tag et le commit avant chaque approbation.
  - Releases immuables activées : une release publiée ne change plus.
  - `pwsh scripts/release/new-signing-key.ps1 -CheckOnly` contrôle ces réglages (lecture seule) ; la création et l'activation d'une clé les exigent.
- **Accès** : le propriétaire de l'organisation. Tout ajout d'administrateur du dépôt est traité comme un accès à la clé.
- **Contrôle annuel** :
  - relire les administrateurs, les relecteurs des deux environnements, le ruleset et les secrets, et lancer `pwsh scripts/release/new-signing-key.ps1 -CheckOnly` ;
  - vérifier que les copies hors ligne se lisent : `pwsh scripts/release/new-signing-key.ps1 -Verify -SecretKeyFile <copie>`. Cette commande fait une signature d'essai et la vérifie contre `install/ciphera.pub`. Elle n'appelle pas GitHub et n'envoie rien.

## Rotation planifiée (tous les 2 ans, ou départ d'une personne ayant eu accès)

Chaque serveur installé ne fait confiance qu'aux clés intégrées à **son** binaire. La nouvelle clé doit donc lui parvenir dans une release signée par l'ancienne.

1. **Préparer.** Lancer `pwsh scripts/release/new-signing-key.ps1 -Rotate`.
   - Une nouvelle paire est créée hors du dépôt.
   - Sa clé publique est **ajoutée** à `install/ciphera.pub`, à côté de l'ancienne.
   - Rien n'est envoyé à GitHub : la CI signe toujours avec l'ancienne clé.
   - Mettre la nouvelle clé à l'abri (deux copies hors ligne, mot de passe dans le gestionnaire).
2. **Release N (distribuer la nouvelle clé).** Commiter `install/ciphera.pub` (deux clés), puis tagger.
   - N est signée par l'**ancienne** clé : les serveurs existants l'acceptent.
   - Ses binaires et son `install.sh` font confiance aux deux clés.
   - Annoncer la nouvelle clé (identifiant, notes de release) et demander de mettre à jour chaque serveur vers N.
3. **Activer.** Après la publication de N, lancer `pwsh scripts/release/new-signing-key.ps1 -Activate -SecretKeyFile <nouvelle clé>`. Les secrets de `release` passent à la nouvelle clé.
   - Le script refuse si la dernière release publiée ne distribue pas encore cette clé (son `install.sh` ne la contient pas).
   - Entre « Activer » et N+1, `refresh-latest.yml` refuse de re-signer le `latest.json` de N : la clé active n'est plus celle qui a signé `SHA256SUMS` de N (`install/sign-latest.sh`). Le run échoue avec « publier la release suivante au lieu de re-signer ». C'est voulu : un `latest.json` de N signé par la nouvelle clé serait illisible pour les serveurs encore avant N, qui ne verraient plus que N existe. Le `latest.json` de N garde sa signature par l'ancienne clé et expire 120 jours après la release N : publier N+1 avant cette date, sinon le panneau des serveurs affiche l'avertissement d'échéance.
4. **Release N+1.** Elle est signée par la **nouvelle** clé. Seuls les serveurs déjà en N (ou au-delà) l'acceptent.
5. **Release N+2 (retirer l'ancienne clé).**
   - **Déplacer** les deux lignes de l'ancienne clé de `install/ciphera.pub` vers `install/ciphera.retired.pub` (créer le fichier s'il n'existe pas), commiter, publier N+2.
   - Après une période d'annonce, détruire les copies hors ligne de l'ancienne clé.

`install/ciphera.pub` accepte au plus 3 clés.

**Ce que deviennent les serveurs anciens**

- **Clés retirées.** `install/ciphera.retired.pub` n'est lu **que** par `pages-site.sh`. Il permet de continuer à servir, sur `get.ciphera.ch`, les releases signées par une clé retirée. Ni `install.sh` ni l'updater n'en tiennent compte : une nouvelle installation, ou une mise à jour d'un serveur à jour, n'accepte jamais une clé retirée. Un serveur ancien vérifie une release avec les clés de **son** binaire : il suffit donc que la release lui reste servie.
- **Fenêtre du site.** Le site sert la release du run et les 4 releases publiées précédentes, soit 5 versions. Une release sortie de cette fenêtre n'est plus servie, quelle que soit sa clé. GitHub Releases les garde toutes, mais l'updater ne lit que `get.ciphera.ch`.
- **Serveur resté avant N.** Il peut passer à N tant que N est dans la fenêtre : `sudo ciphera-cli updater --version vN`, ou depuis le panneau. Ensuite, il peut aller vers les versions suivantes. Il ne peut pas sauter directement à N+1 ou plus : l'updater refuse la signature et le panneau affiche l'erreur. Il voit N dans le panneau tant que le `latest.json` servi est celui de N, signé par l'ancienne clé (la re-signature mensuelle ne change jamais de clé pour une release donnée). Dès que N+1 est publiée, son `latest.json` est signé par la nouvelle clé : le panneau d'un serveur avant N affiche alors « Non vérifiée », et il faut passer à N en ligne de commande (`--version vN`).
- **Après la sortie de N de la fenêtre** (4 releases publiées après N), un serveur encore avant N ne peut plus se mettre à jour. La seule solution est de réinstaller par `install.sh`, qui garde les données. Annoncer cette échéance avec N, et ne pas publier plus de 4 releases après N tant que des serveurs connus ne sont pas en N.

## Révocation et compromission

Une clé **compromise** ne peut pas être retirée des serveurs par l'updater. Une release qui retire l'ancienne clé devrait être signée par une clé que ces serveurs connaissent, c'est-à-dire la clé compromise elle-même, et l'attaquant peut en faire autant. Il n'existe pas de liste de révocation minisign.

**La compromission impose donc une action manuelle sur chaque serveur** : réinstaller avec un `install.sh` vérifié à la main, ou mettre à jour les binaires après avoir vérifié la release corrective par un autre canal (cosign, attestation, empreintes publiées dans l'avis).

Une clé compromise ne va **jamais** dans `install/ciphera.retired.pub` : les releases qu'elle a signées ne doivent plus être servies.

Cas particulier : si la clé compromise n'était encore que la clé « suivante » (avant l'étape 3 de la rotation), il suffit de la retirer de `install/ciphera.pub` et de publier une release signée par la clé active.

Signes d'alerte :
- une release ou un tag inattendu ;
- un run `sign-release` ou `pages-deploy` que vous n'avez pas approuvé ;
- une signature valide sur un fichier inconnu ;
- une entrée Rekor au nom du workflow sans run correspondant ;
- une alerte de connexion GitHub.

Procédure :

1. **Contenir.**
   - Supprimer les secrets de `release` : `gh secret delete CIPHERA_MINISIGN_SECRET_KEY --env release`, puis la même commande pour le mot de passe.
   - Désactiver le workflow Release (Actions → Release → *Disable workflow*).
   - Passer la release suspecte en brouillon ou la supprimer, et supprimer le tag suspect.
   - Désactiver Pages si le `install.sh` servi est en cause.
2. **Comptes.**
   - Changer le mot de passe GitHub ; révoquer les jetons personnels, les sessions et les clés SSH inconnues ; vérifier la 2FA.
   - Relire le journal d'audit de l'organisation : environnements, secrets, rulesets, protections modifiés.
3. **Évaluer.** Lister les artefacts signés pendant la fenêtre (Releases, entrées Rekor de l'identité `release.yml`, attestations) et les comparer aux runs légitimes.
4. **Nouvelle clé.**
   - Remettre `install/ciphera.pub` à la seule nouvelle clé : `pwsh scripts/release/new-signing-key.ps1 -Rotate`, puis supprimer les lignes de la clé compromise.
   - Publier depuis un commit relu une release corrective signée par la nouvelle clé : `-Activate`, puis le tag.
   - Les serveurs existants **ne l'accepteront pas** par l'updater : c'est voulu.
5. **Avertir.** Publier un avis de sécurité GitHub (Security → Advisories). Il doit donner :
   - l'identifiant de la clé révoquée, et la date à partir de laquelle ses signatures ne valent plus ;
   - la nouvelle clé publique ;
   - les versions concernées et les empreintes SHA-256 des fichiers frauduleux connus ;
   - la procédure manuelle : télécharger `install.sh` et `install.sh.minisig`, vérifier avec la nouvelle clé publiée, vérifier aussi `cosign verify-blob` et `gh attestation verify` (section suivante), puis relancer `install.sh`. Les données sont conservées.
   Noter aussi l'incident dans `.agents/memory/problemes.md`.
6. **Après coup.** Relire `release.yml`, les actions épinglées, les règles des environnements et le ruleset avant de réactiver le workflow.

## Vérifier une release

Remplacer `v0.1.0` par la version voulue ; les fichiers sont téléchargés depuis la GitHub Release.

```sh
# 1. minisign : une des clés de ciphera.pub (publiée dans le dépôt et sur le site)
for k in $(grep -v '^untrusted comment:' ciphera.pub); do
  minisign -Vm SHA256SUMS -P "$k" && minisign -Vm install.sh -P "$k" && break
done
sha256sum -c --ignore-missing SHA256SUMS

# 2. cosign keyless (cosign >= 3.1.3, GHSA-fx35-mq7g-6g98) : identité du workflow sur ce tag
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.sigstore.json \
  --certificate-identity "https://github.com/ciphera-dev/deploy-hub/.github/workflows/release.yml@refs/tags/v0.1.0" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com

# 3. Provenance SLSA (attestation GitHub) : workflow de release, sur ce tag
gh attestation verify ciphera-v0.1.0-x86_64-unknown-linux-musl.tar.gz \
  --repo ciphera-dev/deploy-hub \
  --signer-workflow ciphera-dev/deploy-hub/.github/workflows/release.yml \
  --source-ref refs/tags/v0.1.0
```

Le script servi par le site doit être identique à l'asset de la release : `curl -fsSL https://get.ciphera.ch/install.sh | sha256sum` doit donner la valeur de `install.sh.sha256`.
