# Installer et exposer une instance Ciphera

Ce document détaille l'installation résumée dans le [README](../../README.md#installation) : ce que fait l'installeur, la sortie réseau des conteneurs et l'accès au panneau (nom DNS, domaine confirmé, HSTS, passkeys). Les choix de conception sont dans [l'ADR 0003](../adr/0003-installation-et-verification-des-releases.md).

## Prérequis

- Linux avec systemd : Debian, Ubuntu ou Fedora ;
- processeur x86_64 ou aarch64 ;
- accès root (`sudo`).

## Ce que fait l'installeur

```sh
curl -fsSL https://get.ciphera.ch/install.sh | sudo sh
```

Le script :

1. télécharge l'archive de la release et `SHA256SUMS` ;
2. vérifie la signature minisign puis le SHA-256 ;
3. s'arrête sans rien installer si une vérification échoue ;
4. installe les binaires et les unités systemd ;
5. affiche l'URL de l'assistant d'installation avec un jeton à usage unique (`https://<IP>:8443/setup#token=…`), valable 24 heures. Passé ce délai, et tant qu'aucun compte n'existe, `sudo ciphera admin setup-token` en génère un nouveau et réimprime l'URL.

Mode prudent : télécharger `install.sh` et `install.sh.minisig`, vérifier la signature avec la clé publique (`install/ciphera.pub`), lire le script, puis le lancer. La vérification complète d'une release (minisign, cosign, attestation SLSA) est décrite dans [docs/security/release-signing.md](../security/release-signing.md).

`/etc/ciphera/ciphera.env` appartient à root (`root:ciphera`, mode 0640) : les commandes `sudo ciphera admin …` refusent un fichier qui n'appartient pas à root ou qui est modifiable par le groupe ou les autres (corriger avec `chown root:ciphera /etc/ciphera/ciphera.env && chmod 0640 /etc/ciphera/ciphera.env`).

Les certificats propres à l'instance (lien des agents, accès direct du panneau) sont décrits dans [certificats.md](certificats.md).

## Sortie réseau des conteneurs

Les applications ne joignent ni l'hôte lui-même (sauf les ports 80 et 443 du proxy du nœud, pour un domaine servi par ce même serveur) ni les adresses de métadonnées des clouds (169.254.169.254 et le reste de 169.254.0.0/16, `fd00:ec2::254`, `fd20:ce::254`, 100.100.100.200, 168.63.129.16) ; les builds ne joignent en plus aucune plage privée. Les plages privées (10/8, 172.16/12, 192.168/16, 100.64/10, `fc00::/7`) restent permises aux applications par défaut. Pour les interdire :

```sh
curl -fsSL https://get.ciphera.ch/install.sh | sudo CIPHERA_APP_EGRESS_DENY_PRIVATE=1 sh
```

Le réglage est gardé dans `/etc/ciphera-egress/egress.env` (root, `CIPHERA_APP_EGRESS_DENY_PRIVATE=0` ou `1`) : après une modification à la main, il s'applique en 5 minutes au plus (`ciphera-egress-check.timer`) ou tout de suite par `sudo systemctl start ciphera-egress-check.service`.

## Accès au panneau par un nom DNS

L'accès direct au panneau (`https://<IP>:8443`, certificat auto-signé) répond par son adresse IP et par `localhost`. Pour le joindre sur ce port par un nom DNS, déclarez ce nom dans `/etc/ciphera/ciphera.env`, puis redémarrez le service `ciphera` :

```sh
CIPHERA_PANEL_ALIASES=admin.exemple.com,ops.exemple.com
```

Les noms reconnus sur le port 8443 sont l'hôte de `CIPHERA_PUBLIC_URL`, `CIPHERA_WEBAUTHN_RP_ID`, ces alias et le domaine du panneau confirmé. Tout autre nom reçoit `421 Misdirected Request`. Un nom internationalisé se déclare sous sa forme ASCII (punycode `xn--…`). Un nom réservé au panneau ne peut pas devenir le domaine d'un projet, et un domaine de projet ne peut pas être créé ni vérifié sous le nom par lequel on joint le panneau.

## Domaine du panneau confirmé

Une fois le domaine du panneau confirmé (Paramètres › Domaine du panel), l'adresse IP du serveur ne mène plus au panneau : le port 8443 est fermé, et le proxy répond `404` à une requête HTTP ou HTTPS adressée à l'IP, sans révéler le domaine. Il en va de même pour le nom éventuellement déclaré dans `CIPHERA_PROXY_PANEL_HOST`, et l'écouteur facultatif `CIPHERA_HTTP_PORT` ne redirige plus que le domaine du panneau et les noms réservés au panneau (tout autre nom et toute adresse IP reçoivent `404`).

Un nom réservé qui se déduit de l'IP (un nom `sslip.io`, le nom inverse PTR du serveur, etc.) reste redirigé vers le domaine du panneau : déclarer un tel nom dans `CIPHERA_PUBLIC_URL`, `CIPHERA_WEBAUTHN_RP_ID` ou les alias est un choix de l'opérateur, qui révèle alors le domaine à qui connaît l'IP. Prenez l'habitude d'ouvrir le panneau par son domaine avant de confirmer.

Si le domaine ne répond plus, `sudo ciphera admin reset-panel-domain` rouvre l'accès direct au redémarrage du service. Une connexion TLS sans nom (SNI) ne reçoit jamais le certificat du panneau.

## HSTS

HSTS (un an, jamais `preload`) couvre les sous-domaines du domaine du panneau quand il n'est pas à la racine de son domaine (`panel.exemple.com`). Pour un domaine racine (`exemple.com`), l'option « Inclure les sous-domaines (HSTS) » est désactivée par défaut : activée, elle impose HTTPS pendant un an à tous les sous-domaines de la zone, y compris ceux servis ailleurs.

## Passkeys

Les passkeys sont liées à l'origine publique du panneau (domaine confirmé, sinon `CIPHERA_PUBLIC_URL`). Si ce domaine change, les passkeys enregistrées sous l'ancien ne fonctionnent plus : il faut les enregistrer à nouveau.

Elles ne servent qu'à cette adresse exacte (même hôte, même port) : par l'adresse IP, par `localhost`, par un alias, par un sous-domaine ou par un autre port, le panneau ne les propose pas et indique l'adresse publique ; on s'y connecte alors par mot de passe puis code TOTP ou code de secours. Pour un autre choix, fixez `CIPHERA_WEBAUTHN_RP_ID` et `CIPHERA_WEBAUTHN_RP_ORIGIN`.

## Commandes d'administration

Sur le serveur, avec la configuration du service (`sudo ciphera admin <commande>`) :

| Commande | Effet |
|---|---|
| `setup-token` | nouveau lien de l'assistant d'installation (24 heures), tant qu'aucun compte n'existe |
| `reset-password --email <e-mail>` | imprime un lien de réinitialisation du mot de passe (30 minutes, usage unique) |
| `reset-mfa <e-mail>` | supprime le second facteur d'un compte (TOTP, passkeys, codes de secours) et révoque ses sessions |
| `reset-panel-domain` | retire le domaine du panneau ; l'accès direct 8443 se rouvre au redémarrage du service |
| `clear-ip-allowlist` | vide la liste d'IP autorisées |
| `renew-certs` | renouvelle tout de suite les certificats de l'instance, même expirés |
| `revoke-agent <id>` | révoque définitivement un serveur |
| `rotate-ca [--status \| --finish]` | rotation de l'autorité de certification de l'instance |

Mises à jour : `sudo ciphera-cli updater --latest` (dernière version publiée, signature vérifiée) ou `--version vX.Y.Z` ; le retour à une version antérieure exige `--allow-downgrade --restore-db <sauvegarde.sql.zst>`.
