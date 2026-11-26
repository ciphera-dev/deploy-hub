# Politique de sécurité

## Signaler une faille

Signalez toute faille **en privé** : sur GitHub, onglet **Security** du dépôt, puis **Report a vulnerability** (signalement privé via GitHub Security Advisories).

N'ouvrez jamais d'issue, de pull request ni de discussion publique pour une faille : le signalement resterait lisible par tous avant qu'un correctif existe. Décrivez ce que vous avez observé, les étapes pour le reproduire, la version concernée et l'effet possible. Le français et l'anglais sont acceptés.

Il n'y a pas de programme de prime.

## Versions prises en charge

Seule la dernière version publiée (0.1.x) reçoit des correctifs. Un correctif est livré sous la forme d'une nouvelle version.

## Périmètre

Dans le périmètre :

- `crates/` (plan de contrôle, agent, proxy, CLI, mise à jour) ;
- `web/` (interface) ;
- `install/` (installeur) ;
- `.github/workflows/` (chaîne de build et de release) ;
- le site `get.ciphera.ch`.

Hors périmètre :

- `prototypes/` : export de maquettes, jamais publié ni servi ;
- les instances que vous n'administrez pas. Il est interdit de sonder, scanner ou attaquer un serveur qui ne vous appartient pas : testez sur votre propre installation.

## Délais

Ce sont des objectifs, pas des garanties :

- accusé de réception sous 7 jours ;
- point d'étape sous 30 jours.

La divulgation est coordonnée : nous publions l'avis et le correctif ensemble, et nous citons la personne qui a signalé la faille si elle le souhaite.

## Intégrité des releases

Seule la signature **minisign** protège réellement les installations. La clé publique est dans `install/ciphera.pub` et sur https://get.ciphera.ch/ciphera.pub. La procédure de vérification est dans la section « Vérifier une release » de [`docs/security/release-signing.md`](docs/security/release-signing.md), qui décrit aussi la garde, la rotation et la révocation des clés.

Un soupçon de compromission de la clé de signature se signale par le même canal privé.

## Contact général

Pour une question qui n'est pas une faille (fonctionnement, code de conduite, presse) : [info@ciphera.ch](mailto:info@ciphera.ch). N'envoyez jamais le détail d'une faille à cette adresse : utilisez le signalement privé décrit plus haut.
