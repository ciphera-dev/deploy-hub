# Clôture de l'audit de sécurité du 2026-11-24

Vérification de couverture faite le 2026-11-26 sur `main` (73937c3), avant le tag `v0.1.0`. Chaque constat de l'audit externe a été rapproché des commits de sa tâche de remédiation (R1 à R55, plus R3b) et, le cas échéant, des correctifs de CI qui ont suivi. L'audit et le plan détaillé restent hors du dépôt. Ce document décrit seulement l'état atteint, sans détail d'exploitation.

## À traiter

Aucun. Chaque identifiant a au moins un commit, une décision documentée ou une action utilisateur.

## Bilan

- 139 identifiants vérifiés.
- 132 résolus dans le code.
- 2 résiduels acceptés (DEP-1, D-BUILD-6), décrits dans `.agents/memory/problemes.md` › « Résiduels acceptés v0.1.0 ».
- 5 en attente d'une action utilisateur (H09-1, H09-3, H11-1, H11-2, D-REPO-3). Leur code est fait quand il y en a un, mais la fermeture dépend d'un réglage GitHub ou d'une décision du propriétaire du dépôt.

Statuts :
- **Résolu** : correctif dans le code, relu, CI verte.
- **Résiduel accepté** : corrigé en partie, le reste est documenté avec sa raison.
- **Action utilisateur** : la fermeture dépend d'un réglage ou d'une décision hors du dépôt (voir la fin du document).

Commits par tâche (premiers caractères de l'empreinte sur `main`) :

| Tâche | Commits |
|---|---|
| R1 | 29ed73c, 02cc23d |
| R2 | 8912120, 409cad3 |
| R3 | a77d181, 9a7bc60 |
| R3b | fc3f797, ffb435e, 68293ee, 7a36566 |
| R4 | 7006ea0, 905e88a, f6615e4, 1fee2a2 |
| R5 | c0bc89e, f3c6219 |
| R6 | 277155a |
| R7 | bace1a6, 8812dc0 |
| R8 | 5272ae5, 68e14da |
| R9 | c868c70, 1d0f531 |
| R10 | cf7675a, b604c6b |
| R11 | 63095c0, 42e2df8, 3ff47ed |
| R12 | 8337f73, 04605bc, 3a1e327 |
| R13 | 95edd05, 1a66c94, 04216ec |
| R14 | 4a6792a, bb2711c, 1bfb397 |
| R15 | 52abc8a, a961161, 0faf876, 84da0ad |
| R16 | 14e83a7, 670ebbd |
| R17 | 2f7b135, 9f0aa8d |
| R18 | 5928f64, 17cc296, 7f03e53 ; suite CI : 8e4d30c, 1a09003, 78a7bdc, da26d50, 18b5d0d |
| R19 | 97e854c, f63551c |
| R20 | a70e000, ad07e12 |
| R21 | a62128f, b0ea91b |
| R22 | 491e2e0, 3099ee7, fbc9142 |
| R23 | 2f89501, dd774bd |
| R24 | 3b6c5a8, 7fe3317, 0ad48fb, 4625d4d |
| R25 | 37a9d03, fdc7cd1 |
| R26 | 92469cc, d700eaa, 57ab953 |
| R27 | 801a58c, a612e32, 00a6762, 5ad1510 |
| R28 | 46ebc08, ccebbcf |
| R29 | 9c9065a, 1ed4466, 9ff28f2, 6db6eb6 |
| R30 | 5f72fc0, bbafc5f, 9be7a29, c0e5194, 5529f3e, c4a0a9f |
| R31 | 81cb891, 839739b, 80243de, 7c8c987 |
| R32 | 01f7559, 6cea7de, 7047bd7 |
| R33 | f3584f0, 7dead93, 0a9f631, 67364fb, 5003e50, 8a0d546 ; suite CI : e031ed1 |
| R34 | d92a664, ca08e8d, 65a71b4, 83e945a, adafb57 ; suite CI : 396b029 |
| R35 | a65e3ea, 88f7f27, 70c2e0b |
| R36 | 973edee, fd28e59, 5eb25f7, 2492399, 58610d4 ; suite CI : 1a29b31 |
| R37 | 589c54a, dd9ff7a ; suite CI : 9d5965e |
| R38 | 9055867, 035e0f9, be0724d |
| R39 | 37a65a5, ce3ede0 |
| R40 | 0ee4359, 4c102d4 |
| R41 | c34435c, 98b1a2b |
| R42 | 63f708e, f702575 ; suite CI : e6621f8 |
| R43 | 9fd5541, 63358aa, 4055f03, 5ec2aed |
| R45 | b37298e, ffe8491 ; suite CI : a30be27, 714aa39, c6c86ef |
| R46 | 271335a, b0eea7a, e64dc61 ; suite CI : 6dc1f14 |
| R47 | 09cadda |
| R48 | 350e5f8, 53ef80e, a4de54b, bfa1910, dd93d09, 85b047e, 2a3cbe5 ; suite CI : 91ebbb1 |
| R49 | 49ad32b, 0bc5549, 5fa921f ; suite CI : 0b2c940 |
| R50 | 0e15adc, 460c2c6, ecc6beb |
| R51 | 56b091c, aaeb025, 9f97571, 08bf9cd, 2cfc7ae, 6b10fe6, 1bc382b, 95afd2e, a5bcd0f, 4d40180, 5f1eaa4 ; suite CI : a98f7ef, 04a5fe6 |
| R52 | d6661db, f132b20 |
| R53 | cc8257d, d6954a2, 12c1081, 7d2ac39 |
| R54 | 5a10089, 36f2d53 |
| R55 | a58c0f9, 994fd17 ; suite CI : cb66674 |

## Constats des sections 3 à 7 de l'audit

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| H01-1 | R7 | bace1a6, 8812dc0 | Résolu | Ajouter un second facteur redemande le mot de passe actuel |
| H01-2 | R12 | 8337f73, 04605bc, 3a1e327 | Résolu | La 2FA imposée s'applique aussi aux sessions déjà ouvertes |
| H01-3 | R13 | 95edd05, 1a66c94, 04216ec | Résolu | Les compteurs du second facteur et de la ré-authentification sont séparés par facteur, compte, session et IP |
| H01-4 | R14 | 4a6792a, bb2711c | Résolu | Chaque lien de réinitialisation garde son origine ; une demande anonyme ne retire que les liens « mot de passe oublié » |
| H01-5 | R15 | 52abc8a, a961161, 0faf876, 84da0ad | Résolu | L'inscription ouverte vérifie l'adresse avant d'activer le compte |
| H02-1 | R4 | 7006ea0, 905e88a, f6615e4, 1fee2a2 | Résolu | Les réglages SMTP sont réservés au propriétaire |
| H02-2 | R7 | bace1a6, 8812dc0 | Résolu | Le lien de réinitialisation suit les règles de rang, jamais pour soi-même |
| H02-3 | R5, R6 | c0bc89e, f3c6219, 277155a | Résolu | Gestion de l'équipe par l'API et l'écran Paramètres › Équipe |
| H02-4 | R22 (couture R1) | 491e2e0, 3099ee7, fbc9142 | Résolu | Le nettoyage d'un déploiement attend l'accusé de l'agent et rattrape les restes |
| H03-1 | R16 | 14e83a7, 670ebbd | Résolu | Tous les secrets scellés sont liés à leur ligne |
| H03-2 | R18 | 5928f64, 17cc296, 7f03e53, 18b5d0d | Résolu | Les lignes de journal sont rassemblées et masquées avant toute coupe |
| H03-3 | R17 | 2f7b135, 9f0aa8d | Résolu | L'import `.env` garde le statut secret d'une clé existante |
| H03-4 | R17 | 2f7b135, 9f0aa8d | Résolu | Le masquage s'exécute en temps linéaire, hors du verrou des journaux |
| H03-5 | R19 | 97e854c, f63551c | Résolu | Les flux de journaux ont un quota par compte |
| H03-6 | R19 | 97e854c, f63551c | Résolu | Les lignes d'un agent sont bornées en taille et en débit dès la réception |
| H04-1 | R20 | a70e000, ad07e12 | Résolu | Les push GitHub sont dédupliqués sur l'empreinte du corps |
| H04-2 | R21 | a62128f, b0ea91b | Résolu | Le dépôt est identifié par son id GitHub, sans repli anonyme |
| H04-3 | R21 | a62128f, b0ea91b | Résolu | Le jeton d'installation est restreint au seul dépôt du projet |
| H04-4 | R24 | 7fe3317 | Résolu | L'annulation est relue juste avant l'envoi du déploiement |
| H05-1 | R29 | 9c9065a, 1ed4466, 9ff28f2, 6db6eb6 | Résolu | L'accès direct au panneau n'accepte que les hôtes reconnus |
| H05-2 | R11 | 63095c0, 42e2df8, 3ff47ed | Résolu | Le domaine du panneau confirmé n'est jamais mis en erreur DNS ; revérification disponible |
| H05-3 | R11 | 63095c0, 42e2df8 | Résolu | Une panne DNS passagère n'est plus prise pour une absence d'enregistrement |
| H05-4 | R30 | 9be7a29, c4a0a9f | Résolu | `renew-certs` ne re-signe que le certificat du serveur local émis par la CA |
| H06-1 | R8 | 5272ae5, 68e14da | Résolu | L'échéance d'en-têtes du proxy couvre chaque requête d'une connexion |
| H06-2 | R31 | 839739b, 80243de | Résolu | Délais d'écriture et de corps côté client dans le proxy |
| H06-3 | R8 | 5272ae5 | Résolu | La surveillance des connexions ne confond plus deux connexions sur le même descripteur |
| H07-1 | R2, R45 | 8912120, 409cad3, b37298e, ffe8491 | Résolu | Caches de build cloisonnés par projet et environnement ; un buildkitd et un état par projet |
| H07-2 (= H08-4) | R9 | c868c70, 1d0f531 | Résolu | L'agent pose toujours lui-même les étiquettes réservées |
| H07-3 | R26 | 92469cc, d700eaa, 57ab953 | Résolu | Une image interne n'est utilisable que par l'ensemble qui l'a construite |
| H07-4 | R27 | 00a6762, 5ad1510 | Résolu | Le bac à sable de préparation n'a plus de dossier de l'hôte en écriture |
| H07-5 | R3 | a77d181, 9a7bc60 | Résolu | buildkitd a des limites de ressources ; builds en série, clone et image plafonnés |
| H07-6 | R26 | 92469cc | Résolu | Les images internes sont nommées sous un registre non résoluble (action 3 devenue sans objet) |
| H07-7 | R3, R45 | a77d181, b37298e | Résolu | Un seul build à la fois par nœud et un buildkitd par projet |
| H08-1 | R9 | c868c70 | Résolu | « Redémarrer » ne vise que les services du projet connu de l'agent |
| H08-2 | R10 | cf7675a, b604c6b | Résolu | Les services Compose ont les mêmes limites que les conteneurs d'application |
| H08-3 | R28 | 46ebc08, ccebbcf | Résolu | Sortie réseau des applications filtrée (défaut sûr appliqué, action 1 sans réponse) |
| H08-5 | R23 | 2f89501, dd774bd | Résolu | Les volumes d'un projet supprimé sont retirés |
| H08-6 | R23 | 2f89501, dd774bd | Résolu | Retrait complet d'un projet sur ses serveurs ; réseaux vides balayés |
| H09-1 | R42 | 63f708e, f702575 | Action utilisateur | Le site ne garde que les fichiers signés ; « Immutable releases » reste à activer (action 7) |
| H09-2 | R42 | 63f708e | Résolu | Le build de release n'utilise plus de cache npm partagé ; l'outil de contrôle est figé par un lockfile |
| H09-3 | R42 | 63f708e, f702575 | Action utilisateur | `Assert-GitHubReady` contrôle les réglages ; ruleset des tags, « Prevent self-review » et second relecteur restent à poser (action 7) |
| H10-1 | R33 | f3584f0 | Résolu | Le flux de journaux s'arrête après la fin de la session |
| H10-2 | R33 | 5003e50, 9547f2a, 8a0d546 | Résolu | CSP des styles par nonce, sans `'unsafe-inline'` |
| H11-1 | R43 | 63358aa | Action utilisateur | CODEOWNERS en place ; politique des sessions d'agent et équipe `maintainers` à décider (action 6) |
| H11-2 | — | — | Action utilisateur | Purge de l'ancien historique avant de rendre le dépôt public (action 8) ; résiduel jusque-là |
| C01-1 | R22 | 491e2e0, 3099ee7, fbc9142 | Résolu | « Nettoyé » seulement après l'accusé de l'agent |
| C01-2 | R25 | 37a9d03, fdc7cd1 | Résolu | Les notifications ne bloquent plus la file des déploiements |
| V04-N1 | R24, R55 | 3b6c5a8, 0ad48fb, 4625d4d, a58c0f9, 994fd17 | Résolu | Ordre des services Compose autour de la sonde et restauration des dépendances (redémarrage de l'agent en plein déploiement : signalé dans l'issue) |
| V04-N2 | R2, R8, R9 | 8912120, 5272ae5, c868c70 | Résolu | Les commentaires décrivent la garantie réelle (vérifié en R44 dans `conn_limit.rs`, `forward.rs`, `build/mod.rs`, `docker.rs`) |

## Durcissements de la section 8

### Authentification, sessions, comptes

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-AUTH-1 | R12 | 8337f73, 04605bc | Résolu | 2FA imposée et liste d'IP : modification réservée au propriétaire avec son mot de passe |
| D-AUTH-2 | R29 | 9c9065a | Résolu | `X-Forwarded-For` n'est cru que sur l'écoute locale du proxy |
| D-AUTH-3 | R29 | 9c9065a, 9ff28f2, 6db6eb6 | Résolu | RP ID des passkeys fixé sur l'origine publique ; avertissement au changement de domaine |
| D-AUTH-4 | R37 | 589c54a, dd9ff7a | Résolu | Jeton de setup valable 24 h, porté par un fragment d'URL |
| D-AUTH-5 | R13 | 95edd05 | Résolu | Le plafond journalier du TOTP n'est remis à zéro que par un changement de mot de passe |
| D-AUTH-6 | R13 | 95edd05, 1a66c94, 04216ec | Résolu | Un seul calcul Argon2 à la fois par compte, délai croissant après des échecs |
| D-AUTH-7 | R13 | 95edd05 | Résolu | La session en attente du second facteur expire après 10 min |
| D-AUTH-8 | R37 | 589c54a | Résolu | Purge des événements d'audit de connexion échouée après 90 jours |
| D-AUTH-9 | R34 | ca08e8d, 83e945a, adafb57 | Résolu | Délai de corps et délai d'inactivité HTTP/2 sur l'écoute du panneau |
| D-AUTH-10 | R15 | 52abc8a, a961161, 0faf876, 84da0ad | Résolu | Le changement d'adresse passe par un lien de confirmation et ferme les autres sessions |
| D-AUTH-11 | R37 | 589c54a | Résolu | Noms d'invitation sans caractère de contrôle ; jeton vérifié avant Argon2 |
| D-AUTH-12 | R34, R51 | ca08e8d, aaeb025, 08bf9cd, 1bc382b | Résolu | CSP complétée ; HSTS `includeSubDomains` selon le domaine enregistrable, option du propriétaire pour un apex |
| D-AUTH-13 | R34 | d92a664 | Résolu | La version de `/api/health` n'est donnée qu'au pair local ou à une session |
| D-AUTH-14 | R37 | 589c54a | Résolu | `ciphera init` change propriétaire et droits par descripteur, sans suivre de lien |
| D-AUTH-15 | R37 | 589c54a, dd9ff7a | Résolu | Les commandes root refusent un `ciphera.env` qui n'appartient pas à root |
| D-AUTH-16 | R14 | 4a6792a, 1bfb397 | Résolu | Remise à zéro du second facteur d'un membre (API et CLI) |
| D-AUTH-17 | R37 | 589c54a, dd9ff7a | Résolu | Suppression d'organisation : seul propriétaire, confirmation renforcée |

### Autorisations et données

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-AUTHZ-1 | R35 | a65e3ea, 88f7f27, 70c2e0b | Résolu | Un secret global nomme explicitement ses projets |
| D-AUTHZ-2 | R35 | a65e3ea | Résolu | La liste des variables globales est réservée aux administrateurs |
| D-AUTHZ-3 | R36 | 5eb25f7, 58610d4 | Résolu | Changer le port interne ou la sonde exige le droit d'écriture sur le projet |
| D-AUTHZ-4 | R36 | 5eb25f7 | Résolu | Les vues d'ensemble omettent les données sans droit de lecture |
| D-AUTHZ-5 | R36 | 973edee, fd28e59 | Résolu | Contraintes de schéma ajoutées ; recoupement des portées globales refusé |
| D-AUTHZ-6 | R36 | 2492399, 58610d4 | Résolu | OpenAPI complète, avec schémas de sécurité |
| D-AUTHZ-7 | R36 | 2492399 | Résolu | La matrice d'autorisation teste aussi les règles au niveau de l'objet |

### Secrets, journaux, e-mails

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-SEC-1 (= V02-N1) | R18 | 5928f64, 7f03e53 | Résolu | Nettoyage de la ligne (octets nuls, ANSI, contrôles) avant le masquage, sur tous les chemins |
| D-SEC-2 | R17 | 2f7b135, 9f0aa8d | Résolu | Formes encodées des secrets masquées ; avertissement pour les valeurs courtes |
| D-SEC-3 | R18 | 7f03e53 | Résolu | Export `logs.txt` sans séquences de contrôle |
| D-SEC-4 | R19 | 97e854c | Résolu | L'origine d'un flux de build est fixée par le plan de contrôle |
| D-SEC-5 | R4 | 905e88a, f6615e4, 1fee2a2 | Résolu | Le serveur SMTP est contrôlé à chaque connexion (adresses privées refusées) |
| D-SEC-6 | R16, R41 | 14e83a7, 670ebbd, c34435c, 98b1a2b | Résolu | Mode et propriétaire d'`age.key` vérifiés ; clé HMAC des mises à jour séparée |
| D-SEC-7 | R18 (+ R17) | 7f03e53, 2f7b135 | Résolu | Les messages d'erreur de déploiement passent par le masquage |
| D-SEC-8 | R37 | 589c54a | Résolu | Mot de passe du premier compte effacé de la mémoire après usage |

### GitHub, déploiements, tâches

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-GH-1 | R25 | 37a9d03 | Résolu | Au plus 10 déploiements en file par environnement |
| D-GH-2 | R21 | a62128f, b0ea91b | Résolu | Réponses GitHub bornées en taille |
| D-GH-3 | R25 | 37a9d03, fdc7cd1 | Résolu | Une erreur GitHub durable ou une tâche morte termine le déploiement en échec |
| D-GH-4 | R25 | 37a9d03 | Résolu | Purge des tâches terminées ; nettoyage dédupliqué |
| D-GH-5 | R5, R38 (+ R37) | f3c6219, 035e0f9, be0724d, 589c54a | Résolu | Jeton du hook par en-tête, expiration, hooks retirés avec leur créateur, confirmation renforcée |
| D-GH-6 | R20 | a70e000 | Résolu | Commit en hexadécimal normalisé à la réception |
| D-GH-7 | R21 | a62128f | Résolu | Lien vers les réglages de l'App à la déconnexion (GitHub n'offre pas d'API de suppression) |
| D-GH-8 | R25 | 37a9d03 | Résolu | Ordres envoyés à l'agent après la validation de la transaction |
| D-GH-9 | R39 | 37a65a5, ce3ede0 | Résolu | `overflow-checks` en release ; arithmétique ACME saturante |

### Domaines, PKI, lien avec les agents

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-PKI-1 | R30 | c0e5194 | Résolu | Révocation d'un agent (`ciphera admin revoke-agent`), connexion coupée en quelques secondes |
| D-PKI-2 | R47 | 09cadda | Résolu | Lien agent limité à la boucle locale ; enrôlement distant conçu pour v0.2 (ADR 0004) |
| D-PKI-3 | R30 | c0e5194 | Résolu | Validation d'adresse QUIC avant toute place de poignée de main |
| D-PKI-4 | R30, R48 | 9be7a29, c4a0a9f, 53ef80e, a4de54b, dd93d09, 2a3cbe5 | Résolu | Paire écrite atomiquement ; clé de la CA lue par un signataire root séparé |
| D-PKI-5 | R30, R49 | 5f72fc0, 49ad32b, 0bc5549, 5fa921f | Résolu | Certificats feuilles `CA:false` ; rotation de la CA |
| D-PKI-6 | R30 | bbafc5f | Résolu | L'agent vérifie le certificat renouvelé et les droits de sa clé |
| D-PKI-7 | R11 | 42e2df8 | Résolu | Bascule du domaine du panneau sûre face à l'annulation |
| D-PKI-8 | R38, R50 | 9055867, 0e15adc, 460c2c6 | Résolu | Plafonds de domaines ; domaine généré en HTTPS dès que possible (repli HTTP signalé) |

### Proxy

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-PROXY-1 | R8 | 68e14da | Résolu | Unité du proxy bornée (mémoire, tâches, ports) |
| D-PROXY-2 | R8 | 5272ae5 | Résolu | Plafonds de connexions par IP, /64 et /56 |
| D-PROXY-3 | R31 | 839739b | Résolu | En-têtes d'identité retirés (toutes variantes) |
| D-PROXY-4 | R31 | 839739b | Résolu | `Host` amont normalisé |
| D-PROXY-5 | R31 | 81cb891, 80243de | Résolu | Amonts en boucle locale refusés pour les applications |
| D-PROXY-6 | R32 | 6cea7de, 7047bd7 | Résolu | Socket de pilotage réservé à root et à l'agent, avec délai |
| D-PROXY-7 | R32 | 01f7559 | Résolu | Certificat expiré ni chargé ni servi ; compteur purgé |
| D-PROXY-8 | R51 | 56b091c, 9f97571, 6b10fe6, a5bcd0f | Résolu | Les accès par IP ne révèlent plus le nom du panneau après confirmation |

### Builds et agent

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-BUILD-1 | R27 | 801a58c | Résolu | L'askpass ne répond qu'à GitHub et à l'utilisateur attendu |
| D-BUILD-2 | R28 | 46ebc08 | Résolu | Table de sortie rechargée périodiquement, préfixes de transition et DNS restreints, unité de l'agent durcie |
| D-BUILD-3 | R3, R3b | fc3f797, ffb435e, 68293ee | Résolu | Profil AppArmor de buildkitd appliqué (AppArmor 3 et 4) |
| D-BUILD-4 | R3 | a77d181, 9a7bc60 | Résolu | Clone plafonné ; tout le groupe de processus est arrêté |
| D-BUILD-5 | R27 | a612e32 | Résolu | `docker compose config` ne lit plus le `.env` du dépôt |
| D-BUILD-6 | R46 | 271335a, b0eea7a, e64dc61 | Résiduel accepté | Utilisateur et racine en lecture seule réglables par service, badge « root » ; root reste possible quand l'image l'exige |
| D-BUILD-7 | R26 | 57ab953 | Résolu | Téléchargement d'image borné en durée et en taille ; arithmétique vérifiée |
| D-BUILD-8 | R1 | 29ed73c, 02cc23d | Résolu | Une charge tronquée est une erreur de décodage |
| D-BUILD-9 | R28 | 46ebc08 | Résolu | uid 0 refusé pour le conteneur PostgreSQL |

### Installeur, updater, CI

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-INST-1 | R40, R52 | 0ee4359, 4c102d4, d6661db, f132b20 | Résolu | Mode test absent du script publié et du binaire livré |
| D-INST-2 | R40, R41 | 0ee4359, c34435c, 98b1a2b | Résolu | Téléchargements bornés en taille et en durée |
| D-INST-3 | R53 | cc8257d, d6954a2, 12c1081, 7d2ac39 | Résolu | `latest.json` signé avec une échéance de 120 jours |
| D-INST-4 | R42 | 63f708e, f702575 | Résolu | Archives comparées avant signature ; chaîne de signature épinglée |
| D-INST-5 | R41 | c34435c, 98b1a2b | Résolu | L'updater ne lit plus `age.key` ; la sauvegarde décompressée est effacée |
| D-INST-6 | R40 | 0ee4359 | Résolu | Un compte système existant est contrôlé avant réutilisation |
| D-INST-7 | R40 | 0ee4359 | Résolu | La désinstallation sans purge garde les conteneurs d'application |
| D-INST-8 | R40 | 4c102d4 | Résolu | Garde explicite en tête des scripts de test root |

### Interface web et maquettes

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-WEB-1 | R33 | 67364fb, 8a0d546 | Résolu | Jetons retirés de l'URL après lecture |
| D-WEB-2 | R33 | 7dead93 | Résolu | « Copié » seulement après une copie réussie |
| D-WEB-3 | R33 | 0a9f631 | Résolu | Session expirée distinguée d'un mot de passe incorrect |
| D-WEB-4 | R34 | ca08e8d | Résolu | Repli SPA en GET et HEAD seulement ; types MIME complétés |
| D-WEB-5 | R43 | 9fd5541, 4055f03 | Résolu | Maquettes sans CDN non vérifié ; bundle périmé nettoyé |

### Dépôt, documentation, hygiène

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| D-REPO-1 | R43 (orchestrateur) | hors commit | Résolu | Fichiers parasites non suivis supprimés à la racine |
| D-REPO-2 | R43 | 9fd5541 | Résolu | `.superpowers/` ignoré par Git |
| D-REPO-3 | — | — | Action utilisateur | Réglages locaux des permissions Git de l'agent (action 5) |
| D-REPO-4 | R43 | 9fd5541, 4055f03 | Résolu | Masques de secrets du hook mémoire complétés |
| D-REPO-5 | R43 | 9fd5541, abb4463 | Résolu | Exemples d'installation sur le seul domaine du projet |
| D-REPO-6 | R43 | 9fd5541, b88ab8c | Résolu | Adresses d'exemple sur des noms réservés (RFC 2606) |
| D-REPO-7 | R43, R44 | 9fd5541, 4055f03 ; ce commit | Résolu | Chemins locaux, identifiants et EXIF retirés ; résiduels de `problemes.md` réécrits en termes d'état |
| D-REPO-8 | R43 | 9fd5541 | Résolu | `.env.example` réaligné ; README avec `sudo` |
| D-REPO-9 | R43 | 9fd5541 | Résolu | Valeurs de test factices identifiées pour les scanners |

## Dépendances (section 9)

| ID | Tâche | Commit(s) | Statut | État vérifié |
|---|---|---|---|---|
| DEP-1 | R39, R54 | 37a65a5, ce3ede0, 5a10089, 36f2d53 | Résiduel accepté | Aucun remplaçant direct ; avis listés et justifiés, toute nouvelle crate non maintenue fait échouer cargo-deny, chargeurs YAML de Pingora interdits |
| DEP-2 | R54 | 5a10089, 36f2d53 | Résolu | `rsa` 0.10.0 corrigée ; exemption retirée |

## Actions utilisateur (état au 2026-11-26)

| N° | Action | État |
|---|---|---|
| 4 | Dire si `ciphera[.]dev` et `ciphera[.]com` vous appartiennent | Sans réponse ; sans effet sur le code (exemples déjà remplacés par R43) |
| 5 | Restreindre `git add`, `git commit` et `git push` dans `.claude/settings.local.json` | À décider par l'utilisateur (fichier jamais modifié par un agent) |
| 6 | Politique des sessions d'agent sur les branches de tiers ; équipe `maintainers` et « Require review from Code Owners » | À faire (CODEOWNERS livré par R43) |
| 7 | « Immutable releases », ruleset `refs/tags/v*`, « Prevent self-review », second relecteur, puis `new-signing-key.ps1 -CheckOnly` | À faire avant la release ; non constaté |
| 8 | Purge de l'ancien historique (support GitHub ou nouveau dépôt), `reflog expire` et `gc` en local, `.bundle` hors des dossiers synchronisés | À faire avant de rendre le dépôt public ; H11-2 reste résiduel jusque-là |
| 9 | Tag `v0.1.0` (créé et poussé par l'orchestrateur), puis approbation des environnements `release` et `github-pages` (utilisateur) | Après CI complète et Release (test) vertes |

Facultatif après R53 : approuver chaque mois le run `refresh-latest.yml`.
