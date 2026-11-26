# Publier une release de Ciphera

Toute la chaîne est dans `.github/workflows/release.yml` : build, tests de bout en bout, signature, GitHub Release, puis site `get.ciphera.ch`. Pour la garde de la clé, la rotation et les incidents, voir [`docs/security/release-signing.md`](../security/release-signing.md).

| Déclencheur | Ce qui se passe |
|---|---|
| push sur `main` (install/, deploy/, crates/…) ou lancement manuel, **même sur un tag** | Release de **test** : clé de test (`CIPHERA_TEST_MINISIGN_KEY`), artefacts de workflow seulement. Rien n'est publié. |
| push d'un tag `v*` | Release de **production**, dans cet ordre : tests de bout en bout et exécution de l'archive aarch64 ; vérification que le commit du tag est sur `main` ; approbation de l'environnement `release` ; archives comparées (SHA-256) à celles du job `build` ; signature minisign (clé de production, binaire minisign épinglé), cosign keyless et attestation SLSA ; GitHub Release ; site construit depuis les fichiers signés **de ce run** ; approbation de l'environnement `github-pages` ; déploiement. |

Il n'existe aucun autre moyen de publier le site. Si le déploiement échoue, utiliser *Re-run failed jobs* sur le même run.

## Mise en place (une seule fois, par la personne qui garde la clé)

Dans cet ordre :

1. **DNS.** Chez le fournisseur du domaine `ciphera.ch`, ajouter l'enregistrement
   `get  CNAME  ciphera-dev.github.io.`
   Si le DNS est chez Cloudflare, l'enregistrement doit être **DNS only** (nuage gris) : sinon GitHub ne peut pas émettre le certificat et *Enforce HTTPS* reste grisé.
   Ne créer aucun enregistrement A/AAAA pour `get`.
2. **Vérifier le domaine**, pour empêcher qu'un tiers reprenne le sous-domaine. Dans GitHub, organisation `ciphera-dev` → Settings → Pages → *Add a domain* → `get.ciphera.ch`. Ajouter l'enregistrement TXT demandé, puis cliquer *Verify*.
3. **Activer Pages.** Dépôt `ciphera-dev/deploy-hub` → Settings → Pages → *Build and deployment* → Source : **GitHub Actions**.
   - *Custom domain* : saisir `get.ciphera.ch` → *Save*.
   - Attendre la vérification DNS, puis cocher **Enforce HTTPS**.
   - Ce réglage est obligatoire : avec un déploiement par Actions, le fichier `CNAME` du site ne suffit pas.
4. **Protéger l'environnement `github-pages`.** Il est créé à l'étape 3. Le déploiement du site vaut une signature, puisque `install.sh` porte les clés de confiance. Aller dans Settings → Environments → `github-pages` :
   - *Required reviewers* : cocher, ajouter les deux relecteurs de l'étape 6, cocher *Prevent self-review*, puis *Save protection rules*.
   - *Deployment branches and tags* → *Selected branches and tags*. Supprimer la règle `main`, puis *Add deployment branch or tag rule* → type **Tag**, motif `v*`.
5. **Réserver les tags `v*` aux administrateurs.** Settings → Rules → Rulesets → *New ruleset* → *New tag ruleset* :
   - Nom : `tags de release`. *Enforcement status* : **Active**.
   - *Bypass list* : *Add bypass* → **Repository admin** (rôle), et rien d'autre : aucun utilisateur, équipe, application ni rôle *Write* ou *Maintain*.
   - *Target tags* : *Add target* → *Include by pattern* → `v*`.
   - Règles à cocher : **Restrict creations**, **Restrict updates**, **Restrict deletions**, **Block force pushes**.
   - Cliquer *Create*.
6. **Environnement `release` et releases immuables.** L'environnement `release` existe déjà : il a été créé à la publication du dépôt. Aller dans Settings → Environments → `release` :
   - *Required reviewers* : coché, avec **au moins deux relecteurs** (deux comptes distincts, dont aucun n'est celui qui pousse les tags).
     Le contrôle compte les entrées de la liste : une équipe compte pour **un seul** relecteur, quel que soit son nombre de membres. Ajouter deux comptes (ou une équipe et un compte), pas une seule équipe.
   - **Prevent self-review** : coché. Celui qui a poussé le tag ne peut pas approuver seul la signature : l'autre relecteur doit le faire.
   - *Save protection rules*.
   - *Deployment branches and tags* : *Selected branches and tags*, avec une seule règle, de type **Tag**, motif `v*`. Aucune branche.
   - *Environment secrets* : vide avant l'étape 8.

   S'il manque, le créer à l'identique : *New environment* → `release`, avec les mêmes réglages.

   Puis activer les **releases immuables** : Settings → General → section *Releases* → cocher **Enable release immutability**. Une release publiée ne peut alors plus recevoir de fichier, perdre un fichier ou changer de tag.

   **Contrôler le tout** (lecture seule, aucune clé, rien n'est envoyé) :

   ```powershell
   pwsh scripts/release/new-signing-key.ps1 -CheckOnly
   ```

   Le script lit par `gh api` l'environnement `release` (au moins 2 relecteurs, une équipe comptant pour un, *Prevent self-review*, tags `v*` seulement), le ruleset des tags (étape 5 : actif, motif `v*`, les quatre restrictions, contournement par *Repository admin* seulement) et les releases immuables. Il liste tout ce qui manque et échoue tant qu'un réglage n'est pas en place ; l'étape 7 fait le même contrôle avant tout envoi. gh doit être connecté avec un compte administrateur du dépôt : sinon les rulesets, leur liste de contournement et le réglage des releases immuables ne sont pas lisibles (HTTP 403), et le script le dit. Un ruleset d'organisation dont la liste de contournement n'est pas lisible est accepté avec un avertissement ; celle-ci se vérifie alors dans les réglages de l'organisation.
7. **Équipe des relecteurs du code (CODEOWNERS).** `.github/CODEOWNERS` désigne l'équipe `ciphera-dev/maintainers` ; GitHub n'accepte ni une organisation seule ni un compte inexistant. Dans l'organisation `ciphera-dev` → Teams → *New team* → nom `maintainers`, avec le propriétaire comme membre. Donner à cette équipe l'accès **Write** au dépôt `deploy-hub` (Settings → Collaborators and teams). Facultatif : cocher *Require review from Code Owners* dans le ruleset de la branche `main`. Avec un seul mainteneur, cette option bloque les PR fusionnées par leur auteur : c'est le choix du propriétaire.
8. **Créer la clé de production.** Dans votre propre terminal PowerShell 7, à la racine du dépôt. Il faut minisign (`winget install jedisct1.minisign`) et gh connecté (`gh auth login`).

   ```powershell
   pwsh scripts/release/new-signing-key.ps1
   ```

   Ce que fait le script :
   - vérifie les réglages des étapes 5 et 6 (comme `-CheckOnly`) ;
   - demande deux fois le mot de passe (16 caractères minimum, jamais affiché) ;
   - génère la paire hors du dépôt (`~/ciphera-release-key-AAAAMMJJHHMMSS`) et la vérifie ;
   - écrit **seulement** la clé publique dans `install/ciphera.pub` ;
   - envoie `CIPHERA_MINISIGN_SECRET_KEY` puis `CIPHERA_MINISIGN_PASSWORD` dans l'environnement `release`.

   **Si le script s'arrête en cours de route**, il affiche ce qui a déjà été fait :
   - *Échec avant l'écriture de `install/ciphera.pub`* : la paire est effacée, rien n'a été envoyé. Relancer la même commande.
   - *Échec pendant l'envoi* : la clé publique est écrite, la clé secrète reste dans le dossier affiché, et le message liste les secrets déjà envoyés. Reprendre avec `pwsh scripts/release/new-signing-key.ps1 -Activate -SecretKeyFile '<dossier>/ciphera-release.key'`. Cette commande vérifie la clé contre `install/ciphera.pub` puis renvoie les deux secrets.
   - Tant que la clé et le mot de passe envoyés ne vont pas ensemble, `sign-release` échoue et rien n'est publié.
9. **Mettre la clé à l'abri.** Faire deux copies hors ligne de la clé secrète et ranger le mot de passe dans un gestionnaire. Supprimer ensuite le dossier local.
10. **Commiter et pousser la clé publique.** `git add install/ciphera.pub`, commit, push sur `main`. Attendre que CI et Release (test) soient verts.

## Publier une version

1. Vérifier que `version` de `Cargo.toml` et `DEFAULT_VERSION` de `install/install.sh` valent la version voulue. Le job `build` refuse tout écart avec le tag.
2. Créer et pousser le tag sur un commit de `main` à jour. Le job `sign-release` refuse un commit qui n'est pas sur `main`.
   ```sh
   git tag -s v0.1.0 -m "Ciphera v0.1.0"   # -a si aucune clé de signature git
   git push origin v0.1.0
   ```
3. Aller dans Actions → Release → le run du tag. Après `install-e2e` et `smoke-aarch64` (`ciphera --version` de l'archive aarch64 sous qemu), le job `sign-release` attend une **approbation**. Vérifier que le run vient bien du tag attendu et du bon commit, puis *Review deployments* → *Approve*.
4. `publish` crée la GitHub Release, puis `pages-site` construit le site. Le job `pages-deploy` attend une **seconde approbation** (`github-pages`) : vérifier que c'est le même run, puis *Approve*.
   - Les déploiements passent un par un.
   - Un run dont la release n'est plus la dernière publiée refuse de construire ou de déployer le site. Un run plus ancien, approuvé plus tard, n'écrase donc pas le site.
5. Vérifier depuis une machine vierge (VM Debian, Ubuntu ou Fedora) :
   ```sh
   curl -fsSL https://get.ciphera.ch/install.sh | sudo sh
   ```
   L'installation doit afficher « signature minisign verifiee », « SHA-256 verifie », puis le rapport de `ciphera init` avec l'empreinte du certificat du panneau. Pour vérifier les signatures à la main, voir `docs/security/release-signing.md`, section « Vérifier une release ».

## Ce que sert `get.ciphera.ch`

| Chemin | Contenu |
|---|---|
| `/install.sh`, `/install.sh.minisig`, `/install.sh.sha256` | Copies exactes des fichiers signés par le run de la release. |
| `/ciphera.pub` | Clés publiques minisign de confiance (une seule, ou deux pendant une rotation). |
| `/releases/latest.json` | Dernière version, utilisée pour la vérification des mises à jour du panneau. Ce fichier n'est pas signé. |
| `/releases/<version>/…` | Assets de la release, plus ceux des 4 releases précédentes qui se vérifient avec une clé de confiance ou une clé retirée (`install/ciphera.retired.pub`). Seule la liste signée est servie : archives nommées dans `SHA256SUMS`, `SHA256SUMS`, `install.sh`, leurs `.minisig` et `install.sh.sha256`. Tout autre fichier d'une release (ajouté après coup, bundle cosign, `latest.json`) est retiré, avec un avertissement dans le job `pages-site`. Lus par `install.sh` et `ciphera-cli updater`. |
| Autres chemins | Page 404 avec un lien vers GitHub Releases. |

GitHub Pages ne fait pas de redirection HTTP, donc les assets sont recopiés sur le site : environ 45 Mo par release, pour une limite de 1 Go. La bande passante de Pages est limitée à environ 100 Go/mois ; au-delà, prévoir un CDN devant le même contenu.
