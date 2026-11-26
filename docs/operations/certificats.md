# Certificats de l'instance

`ciphera init` cree une autorite de certification propre a l'instance
(`/etc/ciphera/ca/`, 10 ans). Sa cle (`/etc/ciphera/ca/ca.key`) est a root
(0600) et masquee au plan de controle : seul le signataire root
`ciphera-certsign.service` (`ciphera admin sign-requests`) la lit. Le
dossier `ca/` (0750) et `ca.crt` (0640) sont a root, groupe `ciphera` : le
service lit le certificat mais ne peut ni remplacer ni supprimer la cle ou
le certificat. Le dossier `/etc/ciphera` lui-meme est a root, groupe
`ciphera`, 0750 : le service le traverse mais ne peut pas y renommer `ca/`.
Elle signe trois sortes de certificats :

| Certificat | Fichiers | Validite | Renouvellement automatique |
|---|---|---|---|
| Lien des agents (QUIC + mTLS) | `/etc/ciphera/tls/link.{crt,key}` | 730 jours | demande du plan de controle au signataire, aux deux tiers (apres ~487 jours), au demarrage puis toutes les 12 h ; meme cle, recharge a chaud |
| Acces direct du panneau (port 8443) | `/etc/ciphera/tls/panel.{crt,key}` | 730 jours | idem |
| Certificat client de chaque agent | `/etc/ciphera-agent/agent.crt` (cle : `agent.key`) | 365 jours | sur le lien, aux deux tiers (apres ~244 jours) |

Les certificats des domaines (Let's Encrypt) sont geres a part par le proxy
du noeud.

## Signataire root (`ciphera-certsign`)

Le plan de controle (compte `ciphera`) n'a que `ca.crt`. Pour chaque
certificat a renouveler, il ecrit une demande
`/var/lib/ciphera/cert-requests/<nom>.json` (`<nom>` : identifiant du
serveur, `link` ou `panel`) : identifiant de la demande, usage, empreinte de
la cle publique actuelle et, pour un agent, son certificat actuel.
`ciphera-certsign.path` lance alors `ciphera-certsign.service` (root,
oneshot), qui :

- ne signe un certificat d'agent que s'il est emis par la CA, porte
  l'identifiant demande, et que son empreinte est celle enregistree en base
  pour un serveur non revoque ; toujours pour la **meme cle publique** ;
- ne signe le lien ou le panneau que pour la cle du certificat en place
  (memes noms et adresses) ;
- impose la duree (365 ou 730 jours) et les usages ;
- refuse toute autre forme et tout lien symbolique, et supprime la demande
  dans tous les cas ; une entree impossible a supprimer (nom non UTF-8,
  dossier non vide nomme `*.json`) est deplacee dans
  `/var/lib/ciphera/cert-rejected/` (root 0700) et journalisee une fois,
  pour que `ciphera-certsign.path` ne relance pas le signataire sans fin ;
- ecrit la reponse `/var/lib/ciphera/cert-responses/<nom>.crt`
  (identifiant de la demande en premiere ligne, puis le certificat).

Ce que le signataire garantit face a un plan de controle compromis : il ne
signe que pour une cle publique deja certifiee par la CA, pour le meme
identifiant ou les memes noms (aucun certificat pour une cle nouvelle, ni
de CA, ni d'une autre duree). Les controles en base (serveur enregistre,
non revoque) protegent contre une erreur, pas contre un service compromis :
la base appartient au service, qui peut la modifier.

Le plan de controle n'accepte que la reponse a sa propre demande (meme
identifiant), verifiee (CA, meme cle). Sans reponse apres 10 minutes, la
demande est reecrite, puis apres un delai double a chaque fois (au plus
12 h). Journal : `journalctl -u ciphera-certsign` (`certificat ... signe`,
`demande ... refusee : ...`). Une demande refusee ou abandonnee (base
indisponible) ne met pas `ciphera-certsign.service` en echec ; seuls une
CA illisible, un dossier de la CA non protege ou un dossier de demandes
inutilisable le font.

Le signataire tourne aussi a chaque demarrage de la machine, a
l'installation et a la mise a jour qui l'apporte : il remet alors le
dossier de la CA d'une installation anterieure (compte `ciphera`) a root
(cle 0600, `ca.crt` 0640 et `ca/` 0750, groupe conserve). Il refuse de
tourner sous un autre compte que root, comme `ciphera admin renew-certs`.
A l'installation ou a la reparation, un echec du signataire n'arrete
`install.sh` que si la cle de la CA n'est toujours pas a root en 0600 ;
sinon un avertissement est affiche.

Le `.path` et le signataire surveillent `/var/lib/ciphera` : un
`CIPHERA_DATA_DIR` different dans `ciphera.env` n'est pas pris en charge
par le renouvellement (les demandes ne seraient jamais signees ; le journal
du service le signale par `pas de reponse du signataire`).

Le dossier `/etc/ciphera` est a root, groupe `ciphera`, 0750 : `ciphera
init` le laisse ainsi quand il le cree, et `install.sh`, `ciphera admin
renew-certs` et `ciphera admin rotate-ca` corrigent une installation
anterieure ou il appartenait au compte `ciphera` (qui pouvait alors renommer
`ca/` hors de la sandbox du service). Le signataire, sans droit d'ecriture
sur `/etc/ciphera` dans sa sandbox, ne fait que le signaler (`attention :
/etc/ciphera pas a root en 0750`).

## Renouvellement d'un agent

1. L'agent verifie son certificat toutes les 12 h. Passe les deux tiers de
   sa validite, il se reconnecte au plan de controle.
2. A la connexion, le plan de controle demande au signataire un nouveau
   certificat pour la **meme cle** et le **meme identifiant** (la cle
   privee ne quitte jamais l'agent) ; le signataire note son empreinte en
   `servers.next_cert_fingerprint` ; le plan de controle l'envoie
   (`RenewedCertificate`) des que la reponse arrive.
3. L'agent verifie qu'il est signe par une CA de l'instance (`ca.crt`) et
   porte son identifiant et sa cle, remplace `agent.crt` (ecriture
   atomique) puis se reconnecte avec lui.
4. A cette connexion, l'empreinte du nouveau certificat remplace l'ancienne
   en une seule instruction : l'ancien certificat n'ouvre plus le lien.

Tant que l'etape 4 n'a pas eu lieu, l'ancien certificat reste accepte : un
agent qui n'a pas pu ecrire le nouveau fichier se reconnecte comme avant et
le renouvellement reprend 12 h plus tard. Un serveur revoque n'est jamais
renouvele.

Journal a surveiller : `journalctl -u ciphera` (`renouvellement du
certificat demande au signataire`, `certificat renouvele envoye`,
`certificat renouvele en service`, `pas de reponse du signataire`)
et `journalctl -u ciphera-agent` (`certificat de l'agent renouvele`,
`certificat renouvele refuse`).

## Certificat deja expire : `ciphera admin renew-certs`

Une machine eteinte plus de 4 mois apres l'echeance des deux tiers (ou un
signataire en echec) peut laisser expirer un certificat : l'agent ne
se connecte plus (`journalctl -u ciphera-agent` : erreur TLS) ou le panneau
8443 presente un certificat expire. Sur le serveur :

```sh
sudo ciphera admin renew-certs
sudo systemctl restart ciphera ciphera-agent
```

La commande remet d'abord la cle de la CA a root (0600) si besoin, puis
renouvelle, quelle que soit leur date, avec les memes fonctions de signature
que le signataire :

- le certificat du lien et celui du panneau (memes cles, memes noms et
  adresses) ;
- le certificat de l'agent local (`/etc/ciphera-agent/agent.crt`, meme cle ;
  `--agent-dir` pour un autre dossier), et sa copie
  `/etc/ciphera/agents/local.crt`. La nouvelle empreinte est notee en base
  avant l'ecriture du fichier ; elle remplace l'ancienne a la premiere
  connexion de l'agent.

Les fichiers gardent leur proprietaire et leurs droits. La commande lit la
configuration du service (`/etc/ciphera/ciphera.env`) pour joindre la base,
comme les autres commandes `ciphera admin`. Un serveur revoque est refuse.

Le certificat de l'agent n'est re-signe que s'il est emis par la CA de
l'instance, porte l'identifiant du serveur local enregistre et non revoque,
et :

- soit son empreinte est celle enregistree en base pour le serveur local
  (actuelle, ou celle d'un renouvellement en attente) ;
- soit son empreinte est inconnue de la base (base restauree, voir plus
  bas) mais sa cle publique est celle de la copie
  `/etc/ciphera/agents/local.crt` (la cle de l'agent ne change jamais).

Sinon (autre CA, autre identifiant, autre cle, serveur revoque) la commande
s'arrete avant d'ecrire quoi que ce soit.

Le certificat du lien et du panneau est remplace par renommage, la cle ne
change pas. Un remplacement de paire certificat + cle interrompu par une
version anterieure (fichiers prepares `.link.crt.renew`, `.link.key.renew`,
manifeste `.link.crt.pair`) est termine si le manifeste existe, sinon
annule, avant toute lecture. Le service et la commande prennent un verrou
exclusif (`/etc/ciphera/tls/.pair.lock`) pour ces operations : la commande
peut etre lancee pendant que le service tourne.

La CA elle-meme (10 ans) se remplace par une rotation (voir plus bas).
Pendant une rotation, `renew-certs` accepte un certificat emis par l'une ou
l'autre CA ; le lien et le panneau restent signes par la CA courante,
l'agent local par la suivante.

## Rotation de la CA : `ciphera admin rotate-ca`

A faire :

- **apres la mise a jour vers la version qui apporte le signataire root**
  (`ciphera-certsign`) : avant elle, la cle de la CA etait lisible par le
  compte `ciphera` ; la rotation la remplace par une cle qui ne l'a jamais
  ete ;
- quand il reste moins d'un an a la CA : le proprietaire de l'instance
  recoit alors un e-mail (verification horaire, rappel chaque mois, rien
  pendant une rotation) ;
- apres tout doute sur la cle de la CA.

```sh
sudo ciphera admin rotate-ca            # 1. nouvelle CA
sudo ciphera admin rotate-ca --status   # 2. suivi
sudo ciphera admin rotate-ca --finish   # 3. ancienne CA retiree
```

1. `rotate-ca` ecrit une nouvelle CA (10 ans, nommee « Ciphera CA » suivie
   de la date et de l'heure UTC) a cote de la courante : releve des
   certificats du lien et du panneau (`ca/ca-next.own` : cle publique et
   SAN, root 0600), `/etc/ciphera/ca/ca-next.key` (root 0600) puis
   `ca-next.crt` (root, groupe `ciphera`, 0640). Le debut est journalise en
   base (`ca.rotate.start`), puis la commande redemarre le service
   `ciphera` : la nouvelle cle lui est masquee (`InaccessiblePaths` ne
   masque qu'un fichier present au demarrage). Relancee, la commande ne
   change rien. Le service :
   - accepte les certificats d'agent des deux CA sur le lien ;
   - reconnecte les agents connectes ; a chaque connexion, il leur envoie
     les deux CA (`TrustedCas`), que l'agent ecrit dans
     `/etc/ciphera-agent/ca.crt`, puis leur fait signer un nouveau
     certificat par la nouvelle CA (meme cle, meme identifiant, procedure du
     renouvellement ci-dessus, sans attendre les deux tiers) ;
   - note pour chaque serveur la CA du certificat presente
     (`servers.cert_issuer_fingerprint`).

   Le lien et le panneau restent signes par l'ancienne CA : un agent eteint
   pendant la rotation se reconnecte normalement a son retour et passe alors
   a la nouvelle CA.
2. `rotate-ca --status` affiche les deux CA et les serveurs non revoques
   dont l'agent ne presente pas encore un certificat de la nouvelle CA (y
   compris un serveur jamais reconnecte depuis la mise a jour). Un serveur
   qui ne reviendra pas se revoque : `sudo ciphera admin revoke-agent <id>`.
3. `rotate-ca --finish` est refuse tant que cette liste n'est pas vide, ou
   si la cle ou les SAN du lien ou du panneau ont change depuis le debut
   (releve `ca-next.own`) : rien n'est change. Sinon : certificats du lien
   et du panneau re-signes par la nouvelle CA (memes cles), puis `ca.crt`
   et `ca.key` remplaces ensemble par la nouvelle CA (meme mecanisme de
   paire que le lien : manifeste `ca/.ca.crt.pair`, verrou
   `ca/.pair.lock`), puis `ca-next.crt` supprime, puis `ca-next.key` et le
   releve : l'ancienne cle n'existe plus. La commande redemarre ensuite le
   service `ciphera` (sinon elle demande `systemctl restart ciphera`) : lien
   et panneau relus, agents reconnectes, qui ne gardent que la nouvelle CA
   dans `ca.crt`, et cles de la CA de nouveau masquees au service
   (`InaccessiblePaths` ne suit pas un fichier remplace). Un certificat de
   l'ancienne CA n'ouvre plus le lien.

   Une fin interrompue (coupure, erreur) se reprend en relancant
   `--finish`, quel que soit le point d'arret : sous le verrou
   `ca/.pair.lock`, le signataire et chaque commande root terminent d'abord
   un remplacement de `ca.crt` + `ca.key` interrompu, puis retirent les
   restes de la suivante (`ca-next.crt` identique a `ca.crt`, ou
   `ca-next.key` sans `ca-next.crt`). Si c'est le signataire qui a termine
   la fin avant la relance, il en laisse le constat
   (`ca/.rotation-finished`). Dans les deux cas, `--finish` repond « fin de
   rotation interrompue terminee » (code 0) et redemarre le service.

La fin est manuelle : elle remplace la cle de la CA (root) et l'operateur
choisit quand retirer l'ancienne (par exemple apres avoir revoque un
serveur perdu).

**Lien ou panneau changes pendant la rotation** (cle ou SAN, par exemple
apres un changement d'adresse IP) : `--finish` refuse de les re-signer.
Si le changement est voulu, verifier `tls/link.crt` et `tls/panel.crt`,
puis :

```sh
sudo rm /etc/ciphera/ca/ca-next.own
sudo ciphera admin rotate-ca            # refait le releve, rien d'autre
sudo ciphera admin rotate-ca --finish
```

**Une rotation commencee ne s'annule pas** (pas de `--abort`) : des que des
agents ont un certificat de la nouvelle CA, retirer `ca-next.*` a la main
les couperait (le lien n'accepterait plus que l'ancienne). On la termine :
attendre la reconnexion des serveurs, revoquer ceux qui ne reviendront
pas, puis `--finish`. Un navigateur dans lequel l'ancienne CA avait ete importee
pour l'acces direct au panneau (port 8443) doit recevoir la nouvelle
(`/etc/ciphera/ca/ca.crt`) ; l'empreinte du certificat du panneau change.

Journal : `journalctl -u ciphera` (`CA de l'instance changees (rotation)`,
`agent(s) reconnecte(s) pour les nouvelles CA`), `journalctl -u
ciphera-agent` (`CA de l'instance mises a jour`, `CA de l'instance
refusees : ...`). L'agent n'adopte des CA que si ce sont des CA racines,
si le certificat du lien de la connexion en cours est emis par l'une
d'elles (il pourra toujours se reconnecter) et si son propre certificat
l'est aussi (une fois passe a la nouvelle CA, il ne revient jamais a
l'ancienne seule). Debut et fin sont journalises en base
(`ca.rotate.start`, `ca.rotate.cli`).

## Revoquer un serveur : `ciphera admin revoke-agent`

```sh
sudo ciphera admin revoke-agent <id-du-serveur>
```

La commande pose `servers.revoked_at` (definitif, auditee
`server.revoke.cli`). Le service deconnecte l'agent en quelques secondes et
refuse ses poignees de main suivantes (`journalctl -u ciphera` : `agent ...
revoque, deconnecte`, puis `inconnu ou revoque`). Un serveur revoque n'est
plus jamais renouvele ; il n'existe pas de commande pour annuler une
revocation.

L'agent refuse aussi de demarrer si `agent.key` est lisible par le groupe ou
les autres (`ciphera init` l'ecrit en 0600).

## Base restauree depuis une sauvegarde anterieure a un renouvellement

L'empreinte du certificat de chaque agent est en base. Une base remise depuis
une sauvegarde prise **avant** un renouvellement (retour arriere
`ciphera-cli updater --allow-downgrade --restore-db <sauvegarde>`, ou
restauration manuelle) ne connait plus que l'ancienne empreinte : l'agent,
qui presente deja le nouveau certificat, est refuse (`journalctl -u ciphera` :
`certificat ... inconnu ou revoque`, cote agent : `UNAUTHORIZED`). Sur le serveur, apres la
restauration :

```sh
sudo ciphera admin renew-certs
sudo systemctl restart ciphera ciphera-agent
```

La commande reconnait le certificat presente par l'agent (emis par la CA,
identifiant du serveur local, meme cle que `/etc/ciphera/agents/local.crt`)
meme si la base ne connait pas son empreinte, note en base l'empreinte d'un
certificat neuf pour la meme cle et l'ecrit dans
`/etc/ciphera-agent/agent.crt` : l'agent se reconnecte. Le journal le
signale (`empreinte ... inconnue de la base (base restauree ?)`). Les certificats du lien et du panneau, sur disque, ne dependent
pas de la base.

## Echecs de renouvellement

Un renouvellement qui echoue n'arrete rien (l'ancien certificat reste en
service jusqu'a son expiration) mais il est journalise :

- `journalctl -u ciphera -p warning` : `renouvellement de ... non demande`
  (certificat du lien ou du panneau), `pas de reponse du signataire`,
  `certificat ... du signataire refuse`, `certificat renouvele de ... non
  enregistre` (base) ;
- `journalctl -u ciphera-certsign` : `demande ... refusee`, `CA de
  l'instance illisible`, `base indisponible`, `entree ... du dossier des
  demandes : mise de cote ...` ;
- `journalctl -u ciphera-agent -p warning` : `certificat de l'agent a
  renouveler (expire dans N jour(s))`. Ce message ne revient toutes les
  12 h que si le renouvellement n'aboutit pas ; `certificat renouvele
  refuse` dit pourquoi l'agent a ecarte celui recu.

Le panneau n'affiche pas encore l'etat des certificats (v0.1) : surveiller
ces lignes, ou lancer `sudo ciphera admin renew-certs` des qu'elles
apparaissent.
