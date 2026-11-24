# Modèle de menaces — Ciphera v0.1

Établi le 2026-11-23 d'après le code de la branche `main` (plan de contrôle, agent, proxy, updater, unités systemd, `install.sh`). Chaque affirmation renvoie au fichier qui la met en œuvre ; les risques encore ouverts renvoient à `.agents/memory/problemes.md`.

## 1. Périmètre et composants

Une instance v0.1 tient sur un seul nœud Linux :

| Composant | Binaire / unité | Compte | Rôle |
|---|---|---|---|
| Plan de contrôle | `ciphera serve` (`ciphera.service`) | `ciphera` | API REST + WebSocket (Axum), SPA embarquée, files de tâches PostgreSQL, écoute QUIC des agents |
| Agent | `ciphera-agent` (`ciphera-agent.service`) | `ciphera-agent`, membre du groupe `docker` | Conteneurs (bollard), builds (BuildKit), routes et certificats via le proxy |
| Proxy | `ciphera-proxy` (`ciphera-proxy.service` + `.socket`) | `ciphera-proxy`, `CAP_NET_BIND_SERVICE` seule | Ports 80/443 publics, ACME HTTP-01, routage vers les apps et le panneau |
| Updater | `ciphera-cli updater` (`ciphera-updater.service`, `.path`, `-recover`) | root, unité `oneshot` | Applique une release signée, sauvegarde et retour arrière |
| PostgreSQL | conteneur `ciphera-postgres` lancé par l'agent | `ciphera-postgres` | Base du plan de contrôle, réseau `ciphera-internal` |
| buildkitd | conteneur rootless lancé par l'agent | `ciphera-build` (uid système dédié) | Builds des projets |

Hors périmètre v0.1 : nœuds distants enrôlés (le lien QUIC existe mais seul l'agent local est créé par `ciphera init`), SSO, jetons d'API, terminal, pare-feu, scans.

## 2. Actifs

| Actif | Où | Pourquoi il compte |
|---|---|---|
| Identité age de l'instance | `/etc/ciphera/age.key` (0600, compte `ciphera`) | Déchiffre tous les secrets en base (`secrets.rs`, `shared::secretbox`) et dérive la clé HMAC des demandes de mise à jour (`shared::update`) |
| Clé de la CA d'instance | `/etc/ciphera/ca/ca.key` (0600) | Signe le certificat du panneau, du lien QUIC et des agents (`crates/pki`) : qui la détient peut se faire passer pour un agent |
| Clés TLS | `tls/panel.key`, `tls/link.key` ; clés ACME et certificats du proxy chiffrés dans son état | Confidentialité du panneau et des apps |
| Clé privée de l'agent | `agent.key` dans l'état de l'agent (générée côté agent, la CA ne reçoit que la clé publique) | Authentifie l'agent auprès du plan de contrôle |
| Mot de passe de la base | `/etc/ciphera/db_password` (0600) et copie `postgres/db_password` | Accès complet aux données |
| Jeton de setup | `/etc/ciphera/setup_token`, à usage unique | Création du premier owner |
| Sessions | jeton aléatoire de 32 octets en cookie `__Host-ciphera_session` ; seule l'empreinte SHA-256 en base (`auth/session.rs`) | Accès au panneau |
| Secrets applicatifs | variables de projet et globales, scellées age, jamais renvoyées par l'API (`http/variables.rs`) | Secrets des clients |
| Clé privée et secret de webhook de la GitHub App | `github_app.private_key_sealed`, `webhook_secret_sealed` (age) | Accès aux dépôts et déclenchement des déploiements |
| Jetons de deploy hooks (`dh_…`) | empreinte en base, valeur montrée une seule fois | Déclenchent un déploiement sans session |
| Seconds facteurs | secrets TOTP scellés, clés publiques WebAuthn, codes de secours hachés | Résistance au vol de mot de passe |
| Clé publique minisign des releases | embarquée/configurée dans l'updater (`cli/src/updater/release.rs`) | Seule racine de confiance des mises à jour |
| Hôte lui-même | — | Le groupe `docker` et l'updater root donnent root (voir §4) |

## 3. Frontières de confiance

```
Internet ──80/443──> [proxy] ──> apps (réseaux par projet)
   │                    └──────> panneau 8443 (si domaine du panneau)
   │
Navigateur ──HTTPS 8443──> [plan de contrôle] ──QUIC mTLS──> [agent] ──docker.sock──> Docker (= root)
                                │                               └──/run/ciphera/proxy.sock──> [proxy]
                                └── update-request.json (HMAC) ──> [updater root, oneshot]
GitHub ──webhook signé / API──> [plan de contrôle]
```

### 3.1 Navigateur ↔ plan de contrôle

- **Transport** : HTTPS seulement (certificat auto-signé épinglé affiché par `ciphera init`, puis ACME pour le domaine du panneau) ; l'écouteur HTTP ne fait que rediriger (`http/redirect.rs`). En-têtes de sécurité et CSP stricte (`http/security_headers.rs`).
- **Authentification** : mot de passe (Argon2, `auth/password.rs`) puis second facteur TOTP, WebAuthn ou code de secours (`http/mfa.rs`, `http/webauthn.rs`) ; session tournée après la MFA (fixation). Limitation de débit en base (`auth/rate_limit.rs` : 5 échecs par couple IP/e-mail, 50 par IP, espaces séparés pour « mot de passe oublié » et deploy hooks ; second facteur : 5 échecs par compte et par session sur 15 min, et 20 échecs TOTP par compte sur 24 h, qui ne bloquent que le TOTP ; une réinitialisation du mot de passe les efface). Calculs Argon2 bornés à 4 simultanés pour tout le plan de contrôle (`password::offload`), 503 après 10 s d'attente, sans compter la tentative comme un échec.
- **CSRF** : jeton HMAC lié à la session, exigé sur toute écriture sous `/api/` sauf la liste fermée `csrf::EXEMPT` (`auth/csrf.rs`) ; cookie `__Host-` et `SameSite=Strict`.
- **Autorisation** : rôles fixes et matrice ressource × niveau (`authz/matrix.rs`) appliqués par l'extracteur `Require<R, L>` ; actions du propriétaire seul par `owner_only` (`http/settings.rs`). `tests/authz_matrix.rs` appelle chaque endpoint d'`openapi()` (et les routes non documentées : WebSocket des logs, téléchargement des logs, webhook GitHub) sans session et avec chaque rôle ; un endpoint ajouté sans ligne dans la matrice fait échouer le test.
- **Routes sans session, par conception** : `/api/health`, `/api/setup/*` (jeton de setup), `/api/auth/*` (connexion, MFA en attente, invitation, inscription, mot de passe oublié), `POST /v1/hooks/deploy/{hook}` (jeton `dh_` à temps constant, limité par IP), `POST /v1/webhooks/github` (HMAC-SHA256 sur le corps brut, anti-rejeu 24 h, fraîcheur signée). La liste d'IP autorisées (`http/ip_allowlist.rs`) exempte la santé, les hooks, le webhook et le défi ACME.
- **WebSocket des logs** (`http/ws_logs.rs`) : session complète, `Require<Logs, Read>`, origine identique au panneau, droits relus pendant le flux (révocation de session ou changement de rôle ferment le flux).

### 3.2 Plan de contrôle ↔ agent

- **Transport** : QUIC (`quinn`) + mTLS, ALPN `ciphera/1` (`agents/transport.rs`). L'agent n'est accepté que si son certificat chaîne vers la CA d'instance, porte le SAN `urn:ciphera:agent:<id>`, correspond à une ligne `servers` non révoquée (relue après l'inscription au registre) et annonce `PROTO_VERSION` (3) avec le même id.
- **Protocole fermé** : messages typés de `crates/proto` (`ControlToAgent`, `AgentToControl`), aucune commande arbitraire (ADR 0002). Codec postcard à longueur préfixée (`proto/src/codec.rs`) : taille vérifiée avant allocation (4 Mio au plus), octets en trop refusés, jamais de panique. Fuzzing : `crates/proto/fuzz` (cible `decode`, les trois types lus sur un lien, avec aller-retour), job CI `fuzz-proto`.
- **Agent ↔ proxy** : socket Unix `/run/ciphera/proxy.sock` créé par systemd (0660, groupe `ciphera-agent`) ; seuls `ApplyRoutes`, `IssueCertificate`, `CheckCertificate` et `RemoveCertificate` sont acceptés (`proxy/src/control.rs`), le reste reçoit `Rejected`.

### 3.3 Agent ↔ Docker

- **Le groupe `docker` vaut root** : `install.sh` ajoute `ciphera-agent` au groupe `docker`, et l'accès à `/var/run/docker.sock` permet de lancer un conteneur privilégié montant `/`. Le durcissement systemd de l'agent (`NoNewPrivileges`, `CapabilityBoundingSet=` vide, `ProtectSystem=strict`) ne change rien à ce fait : **une compromission de l'agent est une compromission de l'hôte**. Tout le reste de cette section sert à ce que le plan de contrôle (et un utilisateur du panneau) ne puisse pas obtenir cet accès par l'agent.
- **API seulement** : Docker n'est piloté que par bollard, jamais par `docker run` ni `exec` (`agent/src/docker.rs`). Les specs d'application n'ont ni port publié, ni montage, ni privilège (les champs n'existent pas) ; `no-new-privileges` partout sauf buildkitd rootless (exception documentée dans `agent/src/infra.rs`). L'agent ne touche qu'aux objets étiquetés `dev.ciphera.managed=true`.
- **Isolement** : réseau `ciphera-internal` pour l'infrastructure (PostgreSQL), un réseau par projet ; fichiers Compose passés par `compose_policy::check` avant et après normalisation (`agent/src/build/compose.rs`) ; entitlements BuildKit refusés. Voir `problemes.md` › « Un developer pouvait viser la base du plan de contrôle ou obtenir root sur le nœud via Compose » (résolu).
- **BuildKit** : rootless sous le compte système `ciphera-build`, sans `--privileged`, bac à sable des processus conservé. Voir `problemes.md` › « buildkitd tournait privilégié » et « un compte local d'uid 1000 contrôle buildkitd » (résolu en Task 24a).
- **Sortie des builds filtrée** (Task 34c-1) : les ponts `ciphera-build` (buildkitd et ses étapes `RUN`) et `ciphera-prepare` (`railpack prepare`) portent un nom d'interface fixe ; la table nftables `inet ciphera_egress`, chargée en root par `ciphera-agent.service` avant l'agent (échec fermé sans `nft`), rejette tout ce qui en sort vers l'hôte lui-même (panneau, proxy, API Docker, PostgreSQL) et vers les métadonnées des clouds (169.254.169.254, `fd00:ec2::254`…), les plages privées, CGNAT, de lien local et multicast. Seuls Internet public et le DNS (port 53, résolveurs privés des clouds) restent ouverts. Preuve en CI réelle : `install/tests/run-docker-host.sh` (job `install-docker`). Choix et limites : `decisions.md` › « Task 34c-1 » ; greffons asdf/vfox de mise exécutés dans le bac à sable : `problemes.md` › « Durcissement Task 34c-1 : risques résiduels ».

### 3.4 Proxy ↔ Internet

- Seul composant exposé sur 80/443. Compte dédié, `CAP_NET_BIND_SERVICE` seule, filtre d'appels système `@system-service` sans `@privileged`, `MemoryDenyWriteExecute`.
- Plafonds de connexions avant TLS (16 384 au total, 64 par IP ou /64, 256 par /56 IPv6), premier octet sous 10 s, en-têtes plafonnés à 32 Kio / 100 (`proxy/src/conn_limit.rs`, `forward.rs`).
- Délais (Task 34c-1) : en-têtes complets sous 20 s pour chaque requête d'une connexion persistante (attente entre deux requêtes comprise), poignée de main TLS sous 10 s, au plus 32 poignées en cours par IP (1 024 au total ; plafond plein, la plus ancienne est coupée au profit du nouveau venu). Sur 443, l'hôte de la requête doit égaler le SNI de la poignée de main, sinon 421.
- ACME HTTP-01 (`proxy/src/acme.rs`) : seul le chemin `/.well-known/acme-challenge/{token}` est servi sans authentification ; certificats et clés chiffrés au repos. Un certificat n'est installé ou rechargé que si sa clé lui correspond et qu'un SAN DNS couvre l'hôte ; le dossier d'état et ses fichiers doivent appartenir au compte du proxy, sans droit pour le groupe ni les autres. Un hôte retiré pendant une émission n'en reçoit pas le certificat tardif.

### 3.5 Plan de contrôle → updater root (oneshot)

- Le plan de contrôle (non root) dépose `update-request.json` dans `/var/lib/ciphera` ; `ciphera-updater.path` lance l'unité `oneshot` root. La demande est signée HMAC-SHA256 avec une clé dérivée de `age.key` (`shared/src/update.rs`) ; seul le propriétaire peut la créer (`POST /api/system/update`, `owner_only`).
- Le HMAC ne prouve que l'origine : un processus `ciphera` compromis détient la clé. L'updater refuse donc toute version inférieure ou égale à l'installée (sauf `--allow-downgrade` lancé par root) et n'installe qu'une release dont `SHA256SUMS` est signé minisign, archive lue entrée par entrée sans lien ni chemin sortant, outils tiers vérifiés par SHA-256 (`cli/src/updater/release.rs`).
- Sous-processus de l'updater : chemins absolus pris dans les seuls dossiers système (jamais `/usr/local`), environnement vidé (`PATH` système seul, plus `PGPASSWORD` pour `docker exec`), arguments fixes, aucun shell (`clean_command`, `cli/src/updater/host.rs`). Le mot de passe de la base est lu sans suivre de lien, sur un fichier ordinaire à un seul lien, 0600, au propriétaire attendu (`read_password` : root, le propriétaire du dossier ou, dossier à root, le compte `ciphera`). Verrou `update.lock` en 0600 (un autre compte ne peut pas le tenir), dossiers créés avec un mode explicite quel que soit le umask. Restauration de la base dans une transaction validée par le seul `COMMIT` écrit après toute la sauvegarde : un client `docker` tué ne valide jamais une restauration partielle.
- L'unité est bornée : `ProtectSystem=strict` avec une liste fermée de `ReadWritePaths`, capacités limitées aux droits de fichiers, `NoNewPrivileges`.

## 4. Sous-processus autorisés

Aucun shell construit depuis une entrée (CWE-78) dans tout le dépôt. Les seuls `Command::new` du code de production :

| Composant | Programmes | Garde-fous |
|---|---|---|
| Agent (builds) | `/usr/bin/git`, `/usr/bin/docker compose`, `/usr/local/lib/ciphera/bin/buildctl` (`AllowedBin`) | Chemin absolu fixe ; chaque argument est un `ValidatedArg` (liste blanche : chemin, nom, URL Git, branche, SHA de commit, service Compose) ; environnement vidé (`PATH` = `SAFE_PATH`, `LC_ALL=C`), entrée fermée ; le jeton Git passe par l'environnement, jamais journalisé (`agent/src/build/argv.rs`). railpack ne tourne que dans son bac à sable. |
| Updater (root) | `curl` (https seulement), `docker exec ciphera-postgres pg_dump/psql/pg_isready`, `systemctl`, `systemd-tmpfiles` | `clean_command` : chemin absolu dans `/usr/sbin`, `/usr/bin`, `/sbin`, `/bin`, environnement vidé, arguments fixes ; seules les URL (construites et vérifiées) et les chemins de travail varient. |

Le plan de contrôle et le proxy ne lancent aucun sous-processus.

## 5. Gestion des clés

| Clé | Génération | Stockage | Rotation / révocation |
|---|---|---|---|
| Identité age (X25519) | `ciphera init` (`init.rs`) | `/etc/ciphera/age.key`, 0600 | Pas de rotation en v0.1 (rechiffrement de tous les secrets à prévoir) ; sa perte rend les secrets illisibles, sa fuite les révèle tous |
| CA d'instance | `ciphera init`, validité 10 ans | `/etc/ciphera/ca/ca.key`, 0600 (lue par le plan de contrôle pour renouveler) | Certificats d'agent 1 an, serveur 2 ans, renouvelés aux deux tiers (`agents::certs`, `docs/operations/certificats.md`), reprise manuelle `ciphera admin renew-certs` ; la CA elle-même n'est pas renouvelée en v0.1 ; révocation d'un agent par sa ligne `servers` (empreinte actuelle et empreinte renouvelée en attente), effective à la connexion suivante et relue après inscription ; un serveur révoqué n'est jamais renouvelé |
| Clé d'agent | côté agent, la CA ne signe que la clé publique | état de l'agent (0700) | Révocation via `servers` ; le certificat renouvelé garde la même clé (pas de rotation de clé en v0.1) ; l'agent n'accepte qu'un certificat à son identifiant et à sa clé |
| TLS panneau / lien QUIC | `ciphera init` (auto-signé épinglé) ; ACME pour le domaine du panneau | `/etc/ciphera/tls/`, 0600 ; certificats ACME chiffrés par le proxy | Renouvellement ACME ; certificats de la CA renouvelés par le plan de contrôle aux deux tiers (nouvelle clé, rechargement à chaud) |
| minisign (releases) | hors instance (mainteneurs) | seule la clé publique est sur l'hôte | Changement de clé = nouvelle version de l'updater |
| Sessions | 32 octets `SysRng` | empreinte SHA-256 en base | 12 h (30 j avec « rester connecté ») ; tournée à la connexion et après MFA ; révocables par l'utilisateur (`/api/me/sessions`) |
| Jeton CSRF | HMAC-SHA256 dont la clé est le jeton de session | jamais stocké | Suit la session |
| Clé HMAC des demandes de mise à jour | dérivée de `age.key` (étiquette `ciphera/update-request/v1`) | jamais stockée | Suit `age.key` |
| GitHub App (clé privée, secret de webhook) | manifeste GitHub (`connectors/github/manifest.rs`) | scellés age en base | Déconnexion de l'App ; rotation côté GitHub |
| Deploy hooks `dh_…` | aléatoires, montrés une fois | empreinte en base | Suppression du hook |
| Jeton de setup | `ciphera init`, usage unique | `/etc/ciphera/setup_token` | Assistant verrouillé dès qu'un compte existe |

## 6. STRIDE par frontière

| Frontière | Menace | Parade en place | Reste |
|---|---|---|---|
| Navigateur ↔ PC | **S** vol de mot de passe | MFA, limitation de débit, verrouillage progressif du TOTP (20 échecs / 24 h), sessions hachées | Phishing en temps réel du TOTP (WebAuthn y résiste) ; qui a le mot de passe peut bloquer le TOTP du compte 24 h (codes de secours et passkeys restent utilisables) |
| | **T** CSRF, injection | Jeton CSRF lié à la session, `SameSite=Strict`, CSP stricte, requêtes SQLx paramétrées | — |
| | **R** actions non tracées | Journal d'audit sur les écritures sensibles (projets, domaines, variables, hooks, paramètres, mise à jour) | Pas de journal externe inaltérable |
| | **I** fuite de secret | Secrets jamais renvoyés, masqués dans les logs (`VariableSecrets`), jamais en `localStorage` | — |
| | **D** inondation | Limitation de débit des connexions, des routes publiques (`public_limit`) et des téléchargements `logs.txt` (4 simultanés), plafonds de corps, Argon2 à 4 calculs simultanés (`password::offload`) | Pas de limite de débit générale de l'API authentifiée ; sous une rafale de connexions, les connexions légitimes attendent ou reçoivent 503 |
| | **E** élévation de rôle | `Require<R, L>`, `owner_only`, `can_grant`, matrice testée de bout en bout (`tests/authz_matrix.rs`) | Matrice fixe en v0.1 (édition en v0.2) |
| PC ↔ agent | **S** faux agent / faux PC | mTLS CA d'instance, SAN, empreinte non révoquée, version | Fuite de `ca.key` |
| | **T** trame forgée | Codec borné, octets en trop refusés, fuzzing | — |
| | **S/T** agent qui écrit pour un autre serveur | Lignes de journal et métriques acceptées seulement pour les déploiements du serveur qui les envoie ; une métrique par serveur toutes les 10 s, 512 conteneurs au plus | Source de journal suivie avant l'attribution du déploiement : premier serveur émetteur retenu |
| | **D** trames énormes | 4 Mio max, taille vérifiée avant allocation | — |
| | **E** commande arbitraire | Protocole typé fermé, argv validé, `compose_policy` | Un owner peut déployer n'importe quelle image (attendu pour un PaaS) |
| Agent ↔ Docker | **E** conteneur qui s'évade | Pas de privilège, montage ni port publié ; `no-new-privileges` ; réseaux isolés ; BuildKit rootless | Agent compromis = root (groupe `docker`) ; noyau partagé |
| | **I/E** build qui vise l'hôte, les métadonnées du cloud ou le réseau privé (SSRF) | Table nftables `inet ciphera_egress` sur `ciphera-build` et `ciphera-prepare` (Internet public et DNS seulement) | Table perdue sur un `nft flush ruleset` jusqu'au redémarrage de l'agent ; greffons asdf/vfox de mise exécutés dans le bac à sable |
| | **I/T** build qui lit ou empoisonne le cache de build d'un autre projet ou d'un autre environnement | Caches de montage (`BUILDKIT_CACHE_MOUNT_NS`) et clé Railpack sous `<UUID projet>-<UUID environnement>`, ordre sans UUID refusé ; buildkitd n'accepte que le frontend Railpack épinglé et `docker/dockerfile` (`allowedRepositories`), l'agent refuse toute autre directive `syntax` (et `docker/dockerfile` < 1.4, `-labs` ou par empreinte) | Un buildkitd, un état et un socket par projet (un seul buildkitd en marche à la fois, arrêté après 10 min d'inactivité, état supprimé avec le projet ou après 30 jours) ; les caches restent en plus cloisonnés par clé ; tous les états appartiennent au même compte hôte `ciphera-build` (une évasion vers ce compte les atteindrait tous) ; builds en série sur le nœud (aucun `RUN` simultané, R13) |
| Proxy ↔ Internet | **D** slowloris, épuisement mémoire | Plafonds de connexions, premier octet sous 10 s, en-têtes sous 20 s, poignée TLS sous 10 s (32 en cours par IP), 431 | 1 Mio d'en-têtes retenu 20 s au plus par connexion (plafond de Pingora) |
| | **S/T** usurpation de domaine | Certificats ACME (clé et SAN vérifiés), HSTS, SNI = Host sinon 421 | Ordre des commandes de certificat (voir R6) |
| | **I** fuite inter-apps | Routage par hôte, réseaux par projet | — |
| PC → updater | **T/E** demande ou release forgée | HMAC d'origine, refus des retours en arrière, minisign, extraction sûre, `clean_command`, unité bornée | `ciphera` compromis peut déclencher une montée de version **signée** (sans danger par construction) |
| GitHub → PC | **S/T** webhook forgé ou rejoué | HMAC à temps constant avant JSON, anti-rejeu 24 h, fraîcheur signée | — |
| | **D** rafale de livraisons | Limite en mémoire par IP avant tout accès à la base (`public_limit`) | Compteurs perdus au redémarrage |

## 7. Risques résiduels

Repris de `.agents/memory/problemes.md` (source de vérité ; ne pas dupliquer leur traitement ici) :

- **R1 — Proxy : lecture des en-têtes sans délai total, jusqu'à 1 Mio en mémoire par connexion** (`problemes.md`, entrée du 2026-11-22). **Fermé (Task 34c-1)** : délai total d'en-têtes de 20 s, poignée de main TLS bornée à 10 s, 32 poignées en cours par IP. Reste le plafond de 1 Mio de Pingora, retenu 20 s au plus.
- **R2 — Webhook GitHub et routes publiques sans limitation de débit par IP** (`problemes.md`, « Webhook GitHub : risques résiduels connus »). **Fermé (Task 34c-2)** : limite en mémoire par IP avant toute lecture ou écriture en base (`http/public_limit.rs` : 120 livraisons du webhook et 30 appels de deploy hooks par minute, plus 10 déclenchements par minute et par hook valide) ; une rafale ne déchiffre plus le secret du webhook ni n'écrit dans `login_attempts`. Une table par catégorie, plus ancienne clé évincée quand elle est pleine : une rafale de sources ne fait jamais refuser une livraison signée ni un hook valide. Reste : compteurs remis à zéro au redémarrage du plan de contrôle.
- **R3 — Groupe `docker` = root** (§3.3). Inhérent à l'architecture (Docker rootful) ; atténué par un agent minimal sans entrée réseau autre que le lien mTLS et le socket du proxy.
- **R4 — Identité age unique, sans rotation** (§5) : sa fuite révèle tous les secrets et permet de signer des demandes de mise à jour (bornées par minisign et le refus des retours en arrière).
- **R5 — Noyau partagé** entre apps, BuildKit et services de l'instance : une faille noyau franchit l'isolement des conteneurs.
- **R6 — Ordre des commandes de certificat** (`problemes.md`, « Durcissement Task 34c-1 : risques résiduels ») : sans identifiant de requête dans le protocole, un `RemoveCertificate` doublé par l'émission d'un domaine rajouté peut retirer le nouveau certificat ; « Vérifier » relance l'émission. v0.2.
- **R7 — Sortie des builds** (même entrée) : filtre perdu si la table nftables est vidée jusqu'au redémarrage de l'agent ; greffons asdf/vfox de mise exécutés dans le bac à sable ; un build ne joint ni miroir privé ni l'hôte.

- **R8 — Secrets chiffrés non liés à leur ligne** (`problemes.md`, « Durcissement Task 34c-2 : risques résiduels »). **Fermé pour les variables (Task 34c-2)** : chaque valeur secrète est chiffrée avec l'étiquette de sa ligne (`SecretBox::seal_bound`, `variables::Owner`) ; une valeur copiée dans une autre ligne ne se déchiffre plus. Restent non liés : clé de la GitHub App, secret TOTP, mot de passe SMTP.

- **R9 — Revue de sécurité v0.1 (Task 34d)** (`problemes.md`, « Revue de sécurité v0.1 (Task 34d) »). **Fermé** : second facteur deviné sur la durée (plafond de 20 échecs par compte sur 24 h) et Argon2 sans plafond de concurrence (épuisement mémoire sans compte, 4 calculs simultanés désormais). Restent : blocage du TOTP d'un compte par qui détient son mot de passe (24 h au plus ; codes de secours et passkeys restent utilisables, une réinitialisation du mot de passe lève le blocage) ; connexions légitimes retardées sous rafale ; essais de mot de passe répartis sur de nombreuses IP sans plafond par compte (choix : pas de verrouillage du compte par des tiers ; 12 caractères au moins, Argon2id).
- **R10 — Variables `COMPOSE_*` du `.env` d'un dépôt** (à valider en CI Docker réelle) : `docker compose --project-directory <dépôt>` lit le `.env` du dépôt, et Compose exporte ses variables `COMPOSE_*` (`COMPOSE_FILE` et `COMPOSE_PROJECT_NAME` sont écrasés par `-f` et `-p` ; la sortie normalisée repasse par `compose_policy`). Non confirmé : aucun effet hors du dépôt démontré.

- **R11 — Relecture finale v0.1** (`problemes.md`, « Relecture finale v0.1 : risques résiduels après corrections »). **Fermés** : expiration des certificats d'agent et du lien (renouvellement automatique et `ciphera admin renew-certs`), écouteurs du panneau sans délai ni plafond (`http::listener`), journaux des conteneurs sans rotation, routes perdues au redémarrage sans plan de contrôle, unités et profil AppArmor figés dans l'updater, succès de mise à jour sans vérifier proxy, agent et base. Restent : CA non renouvelée, clé d'agent sans rotation, mineurs non corrigés (accusé du proxy avant le succès d'un déploiement, second facteur pour un hook de déploiement et la suppression de l'organisation, étiquettes `dev.ciphera.*` héritées d'une image).

- **R12 — Caches de build partagés entre projets** (audit du 2026-11-24). **Corrigé, sous réserve du job agent-docker** (à marquer « Fermé » une fois ce job vert) : caches de montage des Dockerfile et de Compose et clé Railpack sous les UUID du projet et de l'environnement (une branche de préproduction ne remplit pas les caches de la production), frontends limités par `buildkitd.toml` et par l'agent (`agent/src/build/dockerfile.rs`, `agent/src/infra.rs`) ; preuve en CI réelle : `crates/agent/tests/build_it.rs` (job `agent-docker`). Un buildkitd et un état BuildKit par projet (R45, `agent/src/buildkit.rs` ; preuve en CI réelle : `build_it.rs`, `cache_marker_invisible_even_with_same_namespace`). Reste : tous les états appartiennent au même compte hôte `ciphera-build` (un compte par projet serait nécessaire pour s'en passer).

- **R13 — Ressources, profil AppArmor et clone de buildkitd** (audit du 2026-11-24). **Corrigé, sous réserve du job agent-docker** : buildkitd limité à 4096 processus, 50 % de la RAM du nœud (1 Gio au moins) et tous les CPU sauf un ; ramasse-miettes borné (`reservedSpace` : 10 % du disque de l'état, 5 à 50 Gio) ; un seul build à la fois par nœud (aucun `RUN` concurrent) ; archive d'image (non compressée, donc la taille des couches chargées dans Docker) plafonnée à 10 Gio ; délai ou annulation d'un outil de build : groupe de processus tué (`killpg`) ; clone plafonné à 2 Gio, mesuré pendant le clone. Preuve en CI réelle : `crates/agent/tests/build_it.rs` (`builds_are_serialized`, `build_timeout_kills_children`, `clone_over_cap_fails`) et `docker_it.rs` (job `agent-docker`). **Reste** : le ramasse-miettes passe après les builds, donc le disque qu'un `RUN` remplit pendant un build n'est pas plafonné (aucun quota) ; le plafond du clone compte des octets, pas des fichiers (le délai du clone borne le reste) ; le profil AppArmor `ciphera-buildkitd` garde `flags=(unconfined)` avec la seule règle `userns` (`deploy/apparmor/ciphera-buildkitd`) ; un profil appliqué (montages de rootlesskit, `ptrace` et `signal` des étapes `RUN`, fichiers de runc imbriqué) n'a pas pu être validé contre un vrai buildkitd rootless, et un profil faux casserait tous les builds. Le filtre `--filter=blob:limit` du clone n'est pas ajouté : le plafond de taille borne déjà le clone, et un dépôt privé aurait besoin du jeton au `checkout`.

Résolus et gardés en mémoire pour ne pas régresser : réseau et Compose partagés (Tasks 19, 24, 25), buildkitd privilégié puis uid 1000 (Task 24a), lecture des en-têtes sans délai total (R1, Task 34c-1), routes publiques sans limite par IP (R2, Task 34c-2).
