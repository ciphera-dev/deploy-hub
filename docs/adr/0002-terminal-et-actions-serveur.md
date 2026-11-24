# ADR 0002 — Terminal serveur et actions de sécurité via l'agent

- Statut : accepté (décision utilisateur)
- Date : 2026-11-21

## Contexte

L'agent de nœud suit une règle stricte : son API est étroite, définie par schéma, et il n'exécute **jamais** de commande arbitraire.

Les maquettes proposent pourtant :
- un **terminal** interactif sur serveur, service ou conteneur (`prototypes/dashboard/Terminal.jsx`) ;
- un **pare-feu** (nftables) et un **scanner de malwares** (`ServerSecurity.jsx`).

Ce sont des fonctions attendues d'un PaaS, mais un terminal est par nature de l'exécution arbitraire.

## Décision

1. **Pare-feu et scan malware** : ce sont des **actions typées** de `crates/proto`, par exemple `firewall.apply(rules)`, `firewall.list()`, `malware.scan(kind)`, `malware.quarantine(id)`. L'agent valide les entrées (ports, CIDR, chemins) et ne construit aucune commande shell. Il passe par l'API nftables et par un scanner intégré ou appelé avec des arguments fixes.
2. **Terminal** : c'est une **capacité séparée et restreinte** :
   - **Session et transport :**
     - une session PTY ouverte par l'agent, transportée en WebSocket (navigateur ↔ plan de contrôle) puis en QUIC (plan de contrôle ↔ agent) ;
     - pas de SSH direct depuis le navigateur.
   - **Activation :**
     - désactivée par défaut au niveau de l'instance, activable par un owner ;
     - désactivable aussi par serveur.
   - **Qui peut l'ouvrir :**
     - réservée aux rôles owner et admin ;
     - **re-auth + MFA** à chaque ouverture.
   - **Traçabilité :** chaque session est enregistrée (horodatage, utilisateur, cible, flux d'entrée/sortie) dans le journal d'audit, avec une rétention configurable.
   - **Limites de durée :** la session expire après inactivité et a une durée maximale.
   - **Conteneurs :** `exec` passe par l'API Docker (bollard), avec un shell explicite et sans interpolation.
3. Le message `TerminalOpen` du protocole agent est **distinct** des actions de déploiement. Un agent peut refuser toute session terminal si sa configuration locale l'interdit (`terminal = false`).

## Conséquences

- (+) Les fonctionnalités de la maquette sont conservées, et la surface d'exécution arbitraire est isolée, opt-in et auditée.
- (+) Le pare-feu et le scan ne passent jamais par un shell.
- (−) Le terminal reste une capacité à haut risque : il fera l'objet d'une revue de sécurité dédiée (étape 8, puis avant la v1).
- (−) Il faut stocker les enregistrements de sessions et gérer leur rétention.

## Alternatives écartées

- **Pas de terminal du tout** : plus sûr, mais en retrait face à Coolify et Dokploy, que les utilisateurs comparent sur ce point.
- **SSH direct depuis le navigateur** : il faudrait exposer SSH et gérer des clés côté client.
- **Exec arbitraire dans l'API agent** : contraire au principe de base de l'agent.
