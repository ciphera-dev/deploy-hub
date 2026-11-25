# Problèmes rencontrés

Format : symptôme (date), Contexte, Cause, Solution, À ne pas refaire.

## Chemins dynamiques non réécrits par apply (2026-11-21 12:51)
- Contexte : organize.py apply sur un export Claude Design
- Cause : chemins d'assets construits à l'exécution ('../../assets/db/'+x) non réécrits ; chemins entre guillemets doubles du bundle réécrits relativement au bundle (../../../../) au lieu de la page
- Solution : réécriture manuelle vers ../../web/src/assets/images/<cat>/ dans ApiPage, Connectors, Databases, ServersPage et vendor/ds-bundle.js
- À ne pas refaire : après apply, grep '../../assets/' et chemins à 4 niveaux dans les bundles
- Écrit par : claude-code

## Un developer pouvait viser la base du plan de contrôle ou obtenir root sur le nœud via Compose (2026-11-21 13:15)
- Contexte : Revue security-audit de conception (étape 8)
- Cause : Le plan d'origine mettait les apps et les conteneurs d'infra sur le même réseau Docker et ne bornait pas le contenu des fichiers Compose
- Solution : Réseau ciphera-internal + réseaux par projet ; compose_policy::check ; entitlements BuildKit refusés (Tasks 19, 24, 25)
- À ne pas refaire : Toute nouvelle façon de lancer un conteneur passe par la même politique et par un test d'isolement
- Écrit par : claude-code

## buildkitd tournait privilégié ; le relancer en rootless expose l'uid 1000 de l'hôte (2026-11-21)
- Contexte : relecture de la Task 19 (conteneurs d'infrastructure de l'agent)
- Cause : l'image buildkitd « rootful » exige `--privileged` ; l'image rootless qui la remplace tourne sous l'uid 1000 de l'hôte (fixé par l'image), parfois celui du premier utilisateur humain, et doit se passer de `no-new-privileges` (newuidmap setuid)
- Solution : image `v0.35.1-rootless` épinglée, non privilégiée, seule sur `ciphera-build`, bac à sable des processus de BuildKit conservé (`systempaths=unconfined`, jamais `--oci-worker-no-process-sandbox`) ; socket dans un dossier setgid du groupe de l'agent, buildkitd lancé avec `--group=-1` (l'ACL par défaut de la relecture 1 n'a pas tenu en CI) ; profil AppArmor limité à `userns` là où Ubuntu l'exige. Le parent 0750 ne protège PAS d'un compte local d'uid 1000 : voir l'entrée BLOQUANTE ci-dessous
- À ne pas refaire : ne jamais marquer un conteneur `privileged` ; pour tout conteneur qui exige une exception (no-new-privileges, seccomp, AppArmor), la documenter dans le code et la tester
- Écrit par : claude-code

## BLOQUANT avant la Task 24 : un compte local d'uid 1000 contrôle buildkitd (2026-11-22)
- Contexte : relecture 2 de la Task 19 (buildkitd rootless)
- Cause : l'image `moby/buildkit:rootless` fixe l'uid 1000 ; buildkitd et rootlesskit tournent sous l'uid 1000 de l'hôte dans l'espace de noms initial. Un compte local de même uid (souvent le premier utilisateur humain) atteint le socket par `/proc/<pid>/root` et peut entrer (`setns`) dans l'espace de noms de rootlesskit : lecture/modification des builds, du cache et des secrets. Le dossier parent 0750 n'y change rien.
- Solution : pour l'instant, avertissement d'`install.sh` nommant le compte (sans refus) et documentation (`infra.rs`, `ciphera.tmpfiles.conf`). **Avant que la Task 24 exécute des builds non fiables** : image dérivée avec un uid système dédié (plage de sous-uids réservée), ou refus/ajustement si le compte existe.
- **Résolu (Task 24a, 2026-11-22)** : buildkitd tourne sous le compte système `ciphera-build` (uid système propre, ni 0 ni 1000, refusé sinon par `install.sh`) avec sa propre plage de sous-uids/sous-gids, sans image dérivée : fichiers `passwd`/`group`/`subuid`/`subgid` minimaux générés par l'agent et montés en lecture seule, état dans `/var/lib/ciphera-buildkit` (0700), capacités réduites à SETUID/SETGID. Détails dans `decisions.md` › Task 24a. Seuls root et `ciphera-build` (sans shell ni mot de passe) partagent désormais l'uid de buildkitd.
- À ne pas refaire : ne pas présenter des permissions de chemin comme une frontière contre l'uid sous lequel tourne le processus lui-même
- Écrit par : claude-code

## CI réelle : tmpfiles « unsafe path transition » et busybox nslookup (2026-11-22)
- Contexte : premier run CI avec Docker réel (44960989689) de la Task 19
- Cause : (1) `systemd-tmpfiles` refuse de passer de `/run/ciphera-agent` (compte ciphera-agent) à `buildkit/` (uid 1000) → install.sh s'arrêtait ; (2) busybox `nslookup <nom>` essaie aussi les domaines de recherche du resolv.conf hôte, relayés en SERVFAIL depuis un réseau `internal` → code 1 alors que le nom se résout
- Solution : parent `/run/ciphera-agent` à `root:ciphera-agent` 0750 ; `nslookup -type=a ciphera-postgres.` (nom complet). Reproduit et vérifié localement (Debian + systemd-tmpfiles ; Docker-in-Podman ; e2e install-docker complet dans un conteneur systemd + Docker)
- À ne pas refaire : un dossier tmpfiles d'un compte non root ne contient jamais un dossier d'un autre compte ; sondes DNS toujours avec un nom complet
- Écrit par : claude-code

## CI réelle : socket de buildkitd inaccessible à l'agent malgré l'ACL par défaut (2026-11-22)
- Contexte : second run CI avec Docker réel (44964838736) de la Task 19, seul échec `(a) socket de buildkitd accessible a l'agent (ACL)`
- Cause : buildkitd fait `chmod 0660` puis `chown` du socket vers son propre uid:gid (1000:1000 sur l'hôte) ; l'accès de l'agent reposait uniquement sur l'héritage de l'ACL par défaut du dossier, qui n'a pas tenu sur l'hôte de la CI (passait en local ; le test n'affichait aucun diagnostic)
- Solution : dossier setgid `2770 1000:ciphera-agent` + `--group=-1`, sans ACL ; test e2e par vraie connexion avec diagnostics. Prouvé localement, avec un témoin sans le drapeau qui échoue
- À ne pas refaire : ne pas faire dépendre un droit d'accès des ACL par défaut quand un processus tiers chown/chmod le fichier ; tout test de droits affiche `stat`/`getfacl`/`id` en cas d'échec
- Écrit par : claude-code

## Proxy : lecture des en-têtes sans délai total, jusqu'à 1 Mio en mémoire (2026-11-22)
- Contexte : relecture de la Task 20 (slowloris sur le seul point d'entrée 80/443)
- Cause : Pingora 0.9.1 n'a pas de délai total de lecture des en-têtes (seulement 60 s entre deux lectures, fixe, `early_request_filter` n'intervient qu'une fois les en-têtes lus) et met en mémoire jusqu'à 1 Mio d'en-têtes (`MAX_HEADER_SIZE`) avant nos contrôles 431 (32 Kio, 100 en-têtes) ; la poignée de main TLS a un délai fixe de 60 s
- Solution partielle (relecture Task 20) : plafonds de connexions (16 384 au total, 256 par IP ou /64 IPv6) par `ConnectionFilter` avant TLS, compteur libéré à la fin de la connexion ; premier octet exigé sous 10 s ; `LimitNOFILE=65536`. Reste ouvert : un client qui envoie un octet toutes les 59 s garde sa connexion (dans la limite de 256 par IP) et chaque connexion peut retenir 1 Mio → **à fermer par le durcissement de la Task 34** (délai total d'en-têtes, voire lecteur d'en-têtes en amont de Pingora ou limitation de débit par IP)
- **Résolu (Task 34c-1, 2026-11-23)** : délai total d'en-têtes de 20 s par connexion (y compris l'attente entre deux requêtes d'une connexion persistante), armé par `Capped::process_new` et désarmé par `early_request_filter` ; poignée de main TLS bornée à 10 s (descripteur dupliqué fermé au-delà, rappel pré-TLS de Pingora) ; au plus 32 poignées en cours par IP (ou /64), comptées 11 s au plus. Le plafond de 1 Mio reste celui de Pingora, mais une connexion ne le retient plus que 20 s. Tests : `crates/proxy/tests/forward.rs` (Linux)
- À ne pas refaire : ne pas croire qu'un `set_read_timeout` posé dans un filtre de requête couvre la lecture des en-têtes
- Écrit par : claude-code (implémenteur Task 20, relecture 1)

## Webhook GitHub : risques résiduels connus (2026-11-22)
- Contexte : revue sécurité (secure-code-review, api-security) de la Task 23, `POST /v1/webhooks/github`
- Cause : la route est publique ; chaque requête lit `github_app` et déchiffre le secret de webhook (age) avant le contrôle HMAC, sans limitation de débit. GitHub ne signe aucun en-tête d'horodatage : une livraison signée capturée, rejouée après la purge de la table anti-rejeu (24 h), aurait remis en file le déploiement d'un **ancien** commit (retour arrière de la production sans authentification). La première version de cette note le disait à tort « idempotent »
- Solution : corps plafonné à 5 Mio, HMAC à temps constant avant tout JSON, anti-rejeu 24 h, et (relecture 1) **fraîcheur signée** : la date du push lue dans le corps signé (`repository.pushed_at`, entier ou RFC 3339, sinon `head_commit.timestamp`) doit dater de moins de 24 h (même constante que la purge, `REPLAY_WINDOW_SECS`) et d'au plus 5 min dans le futur ; sinon, ou sans date, 200 ignoré et rien en file. Une livraison ne peut donc plus être rejouée après l'oubli de son identifiant (prix : une relivraison manuelle d'un push de plus de 24 h est ignorée). Sans App connectée : 401, comme une signature invalide. Reste ouvert → **limitation de débit par IP des routes publiques, à traiter par le durcissement de la Task 34**
- À ne pas refaire : ne pas parser le JSON d'un webhook avant d'avoir vérifié la signature sur le corps brut ; ne jamais fier l'anti-rejeu à une table purgée sans borne de fraîcheur signée au moins aussi courte
- Écrit par : claude-code (implémenteur Task 23)

## CI réelle : useradd attribue des sous-ids à un compte non système (2026-11-22)
- Contexte : release (test) install-e2e, cas (e) d'`install/tests/run.sh` (ciphera-build d'uid 1000 pré-créé), run 44992141193, 3 distributions
- Cause : `useradd` (shadow ≥ 4.x, `SUB_UID_*`/`SUB_GID_*` de login.defs) attribue lui-même une plage de 65536 sous-uids/sous-gids à tout compte d'uid ≥ UID_MIN ; la fixture avait donc déjà une plage avant qu'install.sh ne tourne. install.sh refuse bien avant d'écrire (check_build_account avant ensure_subids), vérifié sur debian:12, ubuntu:24.04, fedora:45
- Solution : le test compare /etc/subuid et /etc/subgid avant et après install.sh au lieu d'exiger l'absence de ligne ciphera-build
- À ne pas refaire : ne pas affirmer « aucune plage » après un `useradd` non système ; distinguer ce que l'outil testé a écrit de ce que le système avait déjà
- Écrit par : claude-code (implémenteur Task 24a, correctif CI 1)

## CI réelle : téléchargement des outils tiers impossible dans le conteneur Fedora d'install-e2e (2026-11-22)
- **Contexte** : release (test) install-e2e, run 45003029721 (commit b44cd1a). debian et ubuntu passent ; sur fedora, `telechargement impossible : https://github.com/railwayapp/railpack/...` fait échouer tous les cas (a), et le cas (e) échoue avec un autre motif.
- **Cause** : le mode test exporte `CURL_CA_BUNDLE=<certificat auto-signé du serveur de test>`.
  - Le curl de Debian/Ubuntu garde en plus son CApath `/etc/ssl/certs` : GitHub reste vérifié.
  - Celui de Fedora ne fait confiance qu'au fichier donné, donc échec TLS (curl 60).
  - Reproduit avec podman (debian:12 : code 0 ; fedora:45 : code 60 ; fedora sans la variable : code 0). Ni le réseau ni le nom de l'archive n'étaient en cause.
  - De plus, le refus du compte ciphera-build d'uid 1000 venait après les téléchargements.
- **Solution** :
  - install.sh fait ses vérifications locales (`preflight_checks` : compte ciphera-build existant, plages) avant tout téléchargement.
  - En mode test seulement, `CIPHERA_TOOLS_BASE_URL` remplace `https://github.com` comme origine de railpack, buildctl et mise. Mêmes contrôles que `CIPHERA_RELEASE_BASE_URL` (fonction commune `check_test_url`), SHA-256 épinglés toujours vérifiés.
  - run.sh télécharge une fois ces archives sur l'hôte (versions et empreintes lues dans install.sh, vérifiées) et les sert par le serveur de test.
  - Nouveaux cas : miroir refusé sans mode test, (f) archive railpack altérée sur le miroir refusée.
- **À ne pas refaire** :
  - ne pas supposer que `CURL_CA_BUNDLE` s'ajoute aux autorités du système : selon la distribution, il les remplace ;
  - dans un test hermétique, ne laisser aucun téléchargement externe aux conteneurs.
- Écrit par : claude-code (implémenteur Task 24b, correctif CI 1)

## 2026-11-23 — IP du nœud inconnue après une installation standard (golden-path CI)
- **Symptôme** : job CI `e2e-golden-path` (run 45177092112), étape Domaines : « Vérifier le DNS » renvoie « IP publique du nœud inconnue : définissez CIPHERA_PUBLIC_IP. ».
- **Cause** : la doc de `CIPHERA_PUBLIC_IP` (crates/shared/src/config.rs) annonçait un repli sur l'IP du certificat du panneau (`ciphera init --public-ip`), jamais implémenté ; install.sh n'écrit pas non plus `CIPHERA_PUBLIC_IP`. Sur toute installation standard, la vérification DNS des domaines échouait et le domaine généré n'était jamais créé.
- **Solution** : `pki::ip_from_cert_pem` (SAN IP unique) et `tls::with_panel_ip` appliqué au démarrage du serveur (`main.rs`) : sans `CIPHERA_PUBLIC_IP` ni IP dans `CIPHERA_PUBLIC_URL`, l'IP du nœud est celle de `tls/panel.crt`. Variable explicite toujours prioritaire. Tests : `crates/pki/tests/ca.rs`, `tls::tests`.
- **À ne pas refaire** : un repli documenté dans un tableau de configuration doit avoir son test ; le parcours golden-path (instance réelle) est le seul à couvrir la chaîne install.sh → configuration effective.
- Écrit par : claude-code (implémenteur Task 34b, correctif CI 1)

## 2026-11-23 — Durcissement Task 34c-1 : risques résiduels
- **Contexte** : filtrage de la sortie des builds (table nftables `inet ciphera_egress`) et durcissement du proxy (SNI/Host, contrôles des certificats, état privé, suppression pendant une émission).
- **Ordre des commandes de certificat** : l'agent traite les commandes du proxy en tâches parallèles et le protocole n'a pas d'identifiant de requête. Le proxy abandonne un certificat dont l'hôte a été retiré pendant l'émission (ticket par hôte), et le plan de contrôle n'envoie plus `RemoveCertificate` quand le domaine a été rajouté (N5). Reste : un `RemoveCertificate` doublé par un `IssueCertificate` d'un nouvel ajout peut encore supprimer le nouveau certificat ; « Vérifier » (CheckCertificate) relance alors l'émission → **identifiant de requête et file ordonnée par hôte, v0.2**.
- **mise et ses greffons asdf/vfox** : railpack construit lui-même l'environnement de mise (sans hériter du nôtre), donc `MISE_DISABLE_BACKENDS` ne peut pas lui être passé ; un greffon asdf (shell) ou vfox (Lua) choisi par un dépôt s'exécute pendant `railpack prepare`. Confiné par le bac à sable (non root, sans capacité, racine en lecture seule, tmpfs `noexec`, sortie filtrée). Voir decisions.md.
- **Filtre de sortie vidé** : un `nft flush ruleset` (administrateur, autre outil de pare-feu) retire la table jusqu'au prochain redémarrage de `ciphera-agent` ; rien ne la surveille en continu.
- **Builds privés de réseau local** : un build ne joint ni l'hôte (ses propres sites publics compris, s'ils sont servis par ce nœud), ni un miroir de paquets ou un registre sur une plage privée ; seul le DNS (port 53) y reste ouvert, pour les résolveurs privés des clouds.
- **DNS ouvert partout** : l'exception DNS des ponts de build vaut pour le port 53 de toute adresse (l'hôte, les plages privées, 169.254.169.254:53), pas seulement pour les résolveurs configurés. Accepté : on ne joint qu'un serveur DNS. Piste : ne l'ouvrir que vers les résolveurs de `/etc/resolv.conf` ou de `/run/systemd/resolve/resolv.conf`, lus au chargement de la table.
- **Refus tardif pris pour celui du rajout** : un domaine supprimé puis rajouté pendant sa première émission. Le refus de cette première émission (`install_ticket`) revient comme une réponse `Certificate` associée au seul hôte, et peut être pris pour le résultat de l'émission du domaine rajouté. Effet : un échec affiché à tort, puis une nouvelle tentative qui réussit. Même cause que l'ordre des commandes (pas d'identifiant de requête).
- **Plafond global des poignées de main TLS (relecture, corrigé)** : 32 sources (des /64 IPv6) à 32 poignées lentes remplissaient les 1 024 places et faisaient refuser tout nouveau HTTPS. Désormais, plafond plein, la plus ancienne poignée de main surveillée est coupée au profit du nouveau venu. Il reste qu'un attaquant ouvrant plus de 1 024 poignées de main pendant celle d'un client légitime peut encore couper ce client.
- **nftables requis** : install.sh l'installe. install.sh l'installe même sans Docker, car l'unité de l'agent est toujours activée et démarre dès que Docker apparaît. L'updater refuse, avant toute modification, une release dont une unité exige un outil absent de l'hôte (`/usr/sbin/nft`), et nomme le paquet à installer. Les images de test d'install-e2e incluent `nftables`.
- Écrit par : claude-code (implémenteur Task 34c-1)

## 2026-11-23 — CI acme-pebble : `tempfile::tempdir()` n'est pas privé
- **Symptôme** : job `acme-pebble` (run 45195649876, commit 5fdc3cd), `load_or_create_key` refuse le dossier d'état du test : « /tmp/.tmpXXXX : doit etre en mode 0700 ».
- **Cause** : `tempfile::tempdir()` (3.27) crée le dossier sans mode explicite, donc 0777 moins le umask, soit 0755 sur les runners. Le contrôle du proxy (0700 exigé, comme `StateDirectoryMode=0700`) est juste. Le test `#[ignore]` (Pebble) n'avait pas été lancé localement.
- **Solution** : le test utilise un sous-dossier `state` créé par le proxy lui-même (0700). Les vraies installations passaient déjà : systemd crée `/var/lib/ciphera-proxy` en 0700 au compte ciphera-proxy, et le proxy crée `certs/` et `acme/` en 0700 et ses fichiers en 0600 (déjà le cas avant la Task 34c-1).
- **À ne pas refaire** : un test qui alimente du code vérifiant les droits doit créer ses dossiers en 0700 (`create_private_dir`, ou `tempfile::Builder::permissions`). Lancer localement les tests `#[ignore]` touchés (Pebble en conteneur : `--network container:<conteneur de build>`).
- Écrit par : claude-code (implémenteur Task 34c-1, correctif CI 1)

## 2026-11-23 — Durcissement Task 34c-2 : risques résiduels
- **Contexte** : jobs et lien agent (reprise du LISTEN, garde `attempts < max_attempts`, renvoi sur outbox pleine, `RequestState` après `Lagged`), journaux et métriques liés au serveur émetteur, limites des routes publiques, secrets des variables liés à leur ligne, AppShell, updater, matrice d'autorisation, `public_ip`.
- **Origine des journaux suivis avant l'attribution** : un abonnement à un déploiement encore en file (sans `server_id`) retient le premier serveur qui envoie une ligne pour cette cible (confiance au premier émetteur). Un agent compromis qui devance le bon peut alors injecter des lignes dans ce flux en direct (pas dans `deployment_logs` : l'enregistrement exige le `server_id` du déploiement).
- **Compteurs des routes publiques en mémoire** (`http/public_limit.rs`) : remis à zéro au redémarrage du plan de contrôle ; une table par catégorie, bornée à 10 000 clés, la plus ancienne évincée quand elle est pleine (relecture : la première version faisait partager une seule fenêtre, fermée, à toutes les nouvelles sources, ce qui laissait une rafale depuis un /48 IPv6 faire refuser les livraisons GitHub et les hooks valides). Une rafale depuis de nombreuses sources peut donc remettre à zéro le compteur d'une autre source (échec ouvert, HMAC ou secret toujours vérifiés derrière).
- **Secrets encore non liés à leur ligne** : clé privée de la GitHub App, secret du webhook, secret TOTP, mot de passe SMTP restent chiffrés age sans étiquette (seules les variables le sont). Un retour arrière de l'updater qui ne restaurerait pas la base laisserait l'ancien binaire incapable de lire les variables secrètes rechiffrées : l'updater restaure toujours la sauvegarde `pg_dump` quand la nouvelle version a tourné.
- **Garde de reprise des jobs** (`attempts < max_attempts` dans la branche « bail expiré » de `claim`) : défense en profondeur, sans test RED déterministe (le chemin normal marque déjà `failed` à la dernière tentative).
- **`RequestState` côté agent** : traité dans `crates/agent/src/main.rs` (binaire), sans test unitaire ; la demande côté plan de contrôle est testée (`lagged_agent_events_request_a_full_state`).
- **Retrait d'un membre, changement de rôle (R2 du brief)** : aucun endpoint n'existe encore (écran Équipe en maquette ; seul `POST /api/team/members/{id}/reset-link`). Tout futur endpoint qui retire un membre ou change un rôle doit appeler `state.access.notify()` (comme `me.rs`, `organization.rs`, `settings.rs`).
- **Faux positifs « secret en clair »** : valeurs factices des tests renommées (`sk_test_…`, `…-test-…`) ; les en-têtes PEM factices des tests sont construits par `concat!` pour ne pas imiter une vraie clé dans la source. Le scanner n'a pas été modifié.
- **Relecture 1** : lignes de journal d'un autre serveur reçues pendant une panne de base : retenues avec leur serveur et revérifiées au retour de la base (avant : enregistrées sans contrôle). `RequestState` : une demande au plus toutes les 10 s côté plan de contrôle, lectures d'état fusionnées côté agent.
- **Relecture 2** : l'anti-rebond de `RequestState` perdait la demande de fin d'intervalle (pertes survenues dans les 10 s jamais rattrapées avant le chien de garde) ; corrigé des deux côtés (demande différée unique, relecture unique après la lecture en cours). Reste : une ligne de journal retenue faute de base puis revérifiée est enregistrée après les lignes arrivées entre-temps (ordre d'enregistrement, pas d'horodatage, modifié).
- Écrit par : claude-code (implémenteur Task 34c-2)

## 2026-11-23 — Revue de sécurité v0.1 (Task 34d) : second facteur deviné sur la durée, Argon2 sans plafond
- **Contexte** : revue `security-audit` (guidance), `sharp-edges` et `differential-review` de 8d3ee41..HEAD (auth, sessions, autorisation, routes publiques).
- **Second facteur sans verrouillage progressif (moyen, confirmé)** : `admit_mfa` ne limitait qu'à 5 échecs par 15 min. Qui détient le mot de passe pouvait essayer 480 codes TOTP par jour (2 fenêtres acceptées sur 10^6) : environ 3 % de réussite en un mois, près de 30 % en un an, contre l'exigence « verrouillage progressif » d'`exigences.md`. **Corrigé** : 20 échecs par compte sur 24 h au plus (test `mfa_failures_are_capped_per_day`).
- **Argon2 sans plafond de concurrence (élevé, confirmé par lecture du code)** : chaque connexion sans compte lançait un Argon2 de 19 Mio dans le pool bloquant de tokio (512 fils). La limite par IP (50 échecs) laisse 50 calculs en vol par IP ou /64 : une dizaine d'IPv4, ou un seul /56 IPv6, suffit à demander ~10 Gio et à faire tuer le plan de contrôle (unité sans `MemoryMax`). Non reproduit à l'échelle (pas de charge réseau lancée). **Corrigé** : `password::offload`, 4 calculs simultanés, 503 après 10 s d'attente (tests `offload_bounds_concurrent_work`, `offload_gives_up_when_saturated`). Reste : sous une telle rafale, les connexions légitimes attendent ou reçoivent 503.
- **Relecture** : le plafond journalier ne vise plus que le TOTP (codes de secours et passkeys restent utilisables), une réinitialisation du mot de passe efface les échecs de second facteur, et un refus 503 `busy` ne compte plus comme un échec (connexion ni second facteur).
- **À ne pas refaire** : tout calcul coûteux déclenchable sans session passe par un sémaphore global, pas seulement par une limite par IP ; une limite par fenêtre courte doit être doublée d'un plafond long quand l'espace à deviner est petit (codes à 6 chiffres).
- Écrit par : claude-code (implémenteur Task 34d)

## 2026-11-23 — Relecture finale v0.1 : risques residuels apres corrections
- **Certificats** : la CA (10 ans) n'est pas renouvelee ; la cle de l'agent ne tourne pas ; `agents/local.crt` n'est pas mis a jour par le renouvellement automatique (avertissement « deja enregistre autrement » au demarrage, sans effet) ; entre les deux renommages (cle puis certificat du lien ou du panneau), un arret brutal laisse une paire incoherente : `ciphera admin renew-certs` la refait.
- **Routes gardees** : la table reprise au demarrage est la derniere recue ; un changement fait pendant l'absence du plan de controle n'y est pas.
- **Updater** : `post-install` ne fait rien en v0.1 ; une unite retiree qui echoue a l'arret n'empeche pas la mise a jour (avertissement).
- **install.sh** : une desinstallation sans purge retire `/usr/local/share/ciphera` ; une reinstallation ensuite n'est pas detectee comme relance.
- **Non corriges (mineurs de la relecture)** : B-M3 (deploiement annonce reussi avant l'accuse du proxy), re-authentification + second facteur pour la creation d'un hook de deploiement et la suppression de l'organisation (mot de passe seul aujourd'hui), B-M1 (etiquettes `dev.ciphera.*` heritees d'une image).
- **Relecture des corrections, laisses en l'etat** : M1 (connexions h2 du panneau sans delai d'inactivite dans `http/listener.rs` ; plafond de 128 connexions par IP genant derriere un NAT partage) ; M4 (l'agent arrete la reprise de `routes.json` au premier envoi refuse au lieu de continuer les essais) ; M5 (une route perimee est resservie apres un redemarrage jusqu'a la reconnexion du plan de controle).
- **Registre des unites (I1)** : une unite conservee par l'archive n'est jamais reactivee, donc une section `[Install]` modifiee par une release (autre cible `WantedBy=`) ne refait pas ses liens ; a traiter dans `post-install` de la release concernee. `install.sh` en reparation reactive toujours ses unites (`enable --now`), y compris `ciphera-updater.path` desactivee a la main.
- **install.sh (M2)** : relecture de la version et du journal sous le verrou, sans test automatise (course non reproductible dans `run.sh`).
- **Etat des certificats (M3)** : journalise seulement (`warning`/`error`), pas encore affiche dans le panneau.
- Ecrit par : claude-code (correcteur de la relecture finale v0.1)

## 2026-11-24 — Audit : ressources de buildkitd, builds concurrents, image et clone sans plafond, outils non tues (Task R3)
- Resolu (sous reserve du job agent-docker) : buildkitd sans limite de processus, memoire ni CPU ; cache sans seuil de ramasse-miettes ; deux builds simultanes (RUN concurrents) ; image chargee dans Docker et clone sans plafond ; delai d'un outil sans arret de ses descendants.
- Reste : profil AppArmor de buildkitd non applique (`flags=(unconfined)`), voir threat-model R13.

## 2026-11-24 — CI rouge après l'état BuildKit par projet : socket de test trop long
- Symptôme : job `agent-docker`, chaque build attend 120 s puis « buildkitd n'a pas créé …/buildkitd.sock ».
- Cause : chemin de socket des tests (119 octets) au-delà de la limite `sun_path` (107 + NUL) ; buildkitd créait le socket mais `connect()` échouait. Production non touchée (~79 octets).
- Solution : dossiers de test courts ; chemin > 107 octets refusé avant de lancer le conteneur ; diagnostic complet (état, dossier, journal) dans le journal de l'agent, message court dans le journal de build ; échec rapide si le conteneur s'arrête.
- À ne pas refaire : un test « le fichier existe » ne prouve pas qu'un socket Unix est joignable ; garder les chemins de socket courts.
- Écrit par : claude-code (orchestrateur)

## 2026-11-24 — CI : conteneur buildkitd « zombie » à la désinstallation, test de purge instable
- Symptôme : `install-docker` échouait sur « (b) conteneurs retires » (`docker rm -f` : « PID is zombie and can not be killed ») ; `agent-docker` échouait par moments sur `project_state_removed_on_remove_project`.
- Cause : un buildkitd encore en marche (rootlesskit en PID 1) était supprimé de force par SIGKILL ; Docker abandonnait parfois. Même défaut dans `BuildkitManager::remove_locked`, dont l'erreur n'était que journalisée.
- Correctif : arrêt propre avant suppression (agent et uninstall.sh), init Docker en PID 1 pour buildkitd, essais bornés de suppression ; uninstall.sh sort en 1 et `--purge` ne supprime rien si des conteneurs restent ; install.sh avertit si docker-init manque (les builds échoueraient).
- Reste : si le démon Docker est arrêté, uninstall.sh sort en 0 et `--purge` supprime quand même les répertoires (préexistant).

## 2026-11-25 — CI rouge (agent-docker) après R55 : test d'annulation Compose
- Symptôme : `cancel_restores_dependency_services` (deploy_it.rs, Docker réel) attendait un service exposé jamais créé.
- Cause : le test demandait 60 essais de sonde alors que l'agent en admet 30 (`Healthcheck::MAX_TRIES`) ; `Start` refusait l'ordre avant tout lancement.
- Correction : le test utilise `MAX_TRIES`, attend l'étape (Healthcheck, Started) signalée par l'agent et échoue aussitôt (étapes et journal affichés) si le déploiement se termine avant l'annulation ; `compose_spec` refuse un nombre d'essais hors bornes.
- Leçon : les fichiers de tests réservés à unix ne sont jamais compilés ni lancés sous Windows ; la CI agent-docker est leur premier passage réel.
- Statut : Résolu.

## 2026-11-25 — Journaux d'agent : ligne partielle envoyée, fin ou début de secret visible (garantie R18)
- Symptôme : CI agent-docker rouge sur `long_log_line_is_reassembled_with_the_secret_contiguous` (32768 au lieu de 36404) ; cause du test : lecture pendant que le conteneur écrivait encore (test corrigé : attente de la sortie du conteneur).
- Faille réelle trouvée par la relecture (présente depuis R18) : `LogAssembler::finish()` envoyait la ligne en cours sur toute fin de flux (erreur, délai de prepare, conteneur encore en marche) ; une lecture `tail` ou après rotation (json-file 10m×3) pouvait commencer sur une trame de suite ; sur un conteneur TTY, l'horodatage de Docker restait au milieu de la ligne. Dans ces cas, un secret à cheval sur une trame sortait en partie non masqué (`redact_line` ne masque que la valeur entière).
- Correction (fail-closed) : ligne en attente envoyée seulement sur une fin propre d'une lecture sans suivi d'un conteneur sorti (même exécution avant/après) ; sinon « [ligne incomplète ignorée] » ; toute lecture réelle (`Tail(n)` qui demande n+1, `Beginning`) remplace la première ligne de chaque flux par ce repère ; fin d'un suivi : ligne en attente toujours écartée ; conteneur TTY ou illisible : `timestamps: false` (heure de réception).
- Limites acceptées : première ligne d'une lecture perdue (repère) ; sur un TTY, une ligne qui commence par un horodatage RFC 3339 `Z` le perd (sans fuite, à corriger : sauter `strip_timestamp`/`split_log_chunk` quand les horodatages sont désactivés).
- Statut : Résolu (reste le défaut mineur ci-dessus).

## 2026-11-25 — CI rouge (install-docker) après R30 : témoin du panneau sondé trop tôt
- Symptôme : « (a) temoin (pont par defaut) : panneau de l'hote joignable » en échec deux fois sur 0f4d8a6 (connexion refusée sur <ip>:8443), le reste du test vert.
- Cause : les sondes de sortie passaient avant le premier `panel_up` du script ; le panneau était prêt à temps par hasard jusque-là. Un refus pendant le démarrage faussait aussi les cas « bloqué » (BuildKit, ciphera-prepare).
- Correction : `check "(a) panneau joignable avant les sondes de sortie" panel_up` avant les sondes (fix/ci-panel-witness).

## 2026-11-25 — CI rouge (e2e-golden-path) : polices en data: refusées par la CSP
- Symptôme : 15 violations « font-src data » relevées par l'écouteur CSP ajouté en R33 (polices woff/woff2 inlinées par Vite sous `assetsInlineLimit`) ; défaut antérieur à R33, rendu visible par lui.
- Correction (fix/csp-fonts) : les polices ne sont jamais inlinées (fichiers sous /assets/, servis par embed.rs avec cache immuable) ; `check-dist` échoue sur toute URI data: autre qu'image/* dans le CSS, le JS ou le HTML. La CSP n'est pas élargie.
