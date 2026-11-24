# Certificats de l'instance

`ciphera init` cree une autorite de certification propre a l'instance
(`/etc/ciphera/ca/`, 10 ans). Elle signe trois sortes de certificats :

| Certificat | Fichiers | Validite | Renouvellement automatique |
|---|---|---|---|
| Lien des agents (QUIC + mTLS) | `/etc/ciphera/tls/link.{crt,key}` | 730 jours | par le plan de controle, aux deux tiers (apres ~487 jours), au demarrage puis toutes les 12 h, recharge a chaud |
| Acces direct du panneau (port 8443) | `/etc/ciphera/tls/panel.{crt,key}` | 730 jours | idem |
| Certificat client de chaque agent | `/etc/ciphera-agent/agent.crt` (cle : `agent.key`) | 365 jours | sur le lien, aux deux tiers (apres ~244 jours) |

Les certificats des domaines (Let's Encrypt) sont geres a part par le proxy
du noeud.

## Renouvellement d'un agent

1. L'agent verifie son certificat toutes les 12 h. Passe les deux tiers de
   sa validite, il se reconnecte au plan de controle.
2. A la connexion, le plan de controle emet un nouveau certificat pour la
   **meme cle** et le **meme identifiant** (la cle privee ne quitte jamais
   l'agent), note son empreinte en `servers.next_cert_fingerprint` et
   l'envoie (`RenewedCertificate`, protocole version 5).
3. L'agent verifie qu'il porte son identifiant et sa cle, remplace
   `agent.crt` (ecriture atomique) puis se reconnecte avec lui.
4. A cette connexion, l'empreinte du nouveau certificat remplace l'ancienne
   en une seule instruction : l'ancien certificat n'ouvre plus le lien.

Tant que l'etape 4 n'a pas eu lieu, l'ancien certificat reste accepte : un
agent qui n'a pas pu ecrire le nouveau fichier se reconnecte comme avant et
le renouvellement reprend 12 h plus tard. Un serveur revoque n'est jamais
renouvele.

Journal a surveiller : `journalctl -u ciphera` (`certificat renouvele
envoye`, `certificat renouvele en service`, `renouvellement ... impossible`)
et `journalctl -u ciphera-agent` (`certificat de l'agent renouvele`,
`certificat renouvele refuse`).

## Certificat deja expire : `ciphera admin renew-certs`

Une machine eteinte plus de 4 mois apres l'echeance des deux tiers (ou une
CA illisible par le service) peut laisser expirer un certificat : l'agent ne
se connecte plus (`journalctl -u ciphera-agent` : erreur TLS) ou le panneau
8443 presente un certificat expire. Sur le serveur :

```sh
sudo ciphera admin renew-certs
sudo systemctl restart ciphera ciphera-agent
```

La commande renouvelle, quelle que soit leur date :

- le certificat du lien et celui du panneau (nouvelles cles, memes noms et
  adresses) ;
- le certificat de l'agent local (`/etc/ciphera-agent/agent.crt`, meme cle ;
  `--agent-dir` pour un autre dossier), et sa copie
  `/etc/ciphera/agents/local.crt`. La nouvelle empreinte est notee en base
  avant l'ecriture du fichier ; elle remplace l'ancienne a la premiere
  connexion de l'agent.

Les fichiers gardent leur proprietaire et leurs droits. La commande lit la
configuration du service (`/etc/ciphera/ciphera.env`) pour joindre la base,
comme les autres commandes `ciphera admin`. Un serveur revoque est refuse.

La CA elle-meme (10 ans) n'est pas renouvelee par Ciphera v0.1.

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

La commande note en base l'empreinte d'un certificat neuf pour la cle de
l'agent local et l'ecrit dans `/etc/ciphera-agent/agent.crt` : l'agent se
reconnecte. Les certificats du lien et du panneau, sur disque, ne dependent
pas de la base.

## Echecs de renouvellement

Un renouvellement qui echoue n'arrete rien (l'ancien certificat reste en
service jusqu'a son expiration) mais il est journalise :

- `journalctl -u ciphera -p warning` : `renouvellement de ... impossible`
  (certificat du lien ou du panneau), `CA de l'instance illisible`
  (aucun renouvellement automatique), `certificat renouvele de ... non
  enregistre` (base) ;
- `journalctl -u ciphera-agent -p warning` : `certificat de l'agent a
  renouveler (expire dans N jour(s))`. Ce message ne revient toutes les
  12 h que si le renouvellement n'aboutit pas ; `certificat renouvele
  refuse` dit pourquoi l'agent a ecarte celui recu.

Le panneau n'affiche pas encore l'etat des certificats (v0.1) : surveiller
ces lignes, ou lancer `sudo ciphera admin renew-certs` des qu'elles
apparaissent.
