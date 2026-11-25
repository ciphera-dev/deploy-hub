# ADR 0004 — Enrôlement des agents distants

- Statut : proposé (v0.2) ; seule la règle de refus hors boucle locale est en vigueur en v0.1
- Date : 2026-11-25

## Contexte

En v0.1, un seul serveur existe : l'agent tourne sur le même hôte que le plan de contrôle. Le lien QUIC + mTLS (ADR 0002) est donc écouté sur la boucle locale, et le certificat de l'agent local est émis à l'installation par la CA d'instance.

Ouvrir le lien à une autre adresse sans mécanisme d'enrôlement obligerait à distribuer à la main un certificat client signé par la CA, sans preuve que la clé reste sur le serveur distant. Cette porte reste donc fermée tant que l'enrôlement n'existe pas.

## Décision

### En v0.1 : refus hors boucle locale

`CIPHERA_AGENT_LINK_BIND` doit être une adresse de boucle locale (`127.0.0.0/8` ou `::1`). Toute autre valeur (`0.0.0.0`, `::`, adresse privée ou publique, IPv4 mappée en IPv6) fait échouer le chargement de la configuration, donc le démarrage, avec une erreur explicite (`ConfigError::NotLoopback`). Il n'existe aucun réglage pour contourner ce refus. La règle ne sera levée qu'avec l'implémentation décrite ci-dessous.

### En v0.2 : enrôlement par jeton de jonction

1. **Jeton de jonction.** Le propriétaire de l'organisation crée un jeton de 256 bits tirés du générateur du système, affiché une seule fois. Seul son haché est stocké. Il est à **usage unique** et valable **1 h**. La création exige une confirmation renforcée du propriétaire (step-up, R37) et est journalisée.
2. **Demande de certificat.** Sur le serveur distant, l'agent génère lui-même sa clé privée, qui ne quitte jamais la machine. Il présente le jeton et une demande de signature (CSR) en HTTPS au panneau. L'agent **épingle l'empreinte** du certificat du panneau, communiquée avec le jeton (commande de jonction) : aucune confiance dans une autorité tierce pour cette étape.
3. **Signature.** Le plan de contrôle valide le jeton (haché, non expiré, non consommé), le consomme de façon atomique, puis fait signer la CSR par le **signataire séparé** (R48) qui détient seul la clé de la CA. Le SAN `urn:ciphera:agent:<id>` est imposé par le signataire, jamais repris de la CSR. Le plan de contrôle ne détient plus la clé de la CA.
4. **Enregistrement.** Une ligne `servers` est créée pour l'agent ; il se connecte ensuite sur le lien QUIC + mTLS existant, sur une adresse choisie par l'administrateur.
5. **Révocation (R30).** Un serveur est révoqué (`servers.revoked_at`, `ciphera admin revoke-agent`) ; ses connexions sont fermées par le balayage `watch_revocations` (5 s) et son certificat n'est plus accepté. Le jeton consommé ne peut pas être rejoué.

### Dépendances

- R37 (confirmation renforcée du propriétaire) pour la création du jeton.
- R48 (signataire racine séparé) pour la signature des CSR. Livré en v0.1 (`ciphera-certsign.service`, `ciphera admin sign-requests`, `agents::signer`) pour les renouvellements : il ne signe aujourd'hui que pour une clé publique déjà enregistrée ; la signature d'une CSR d'enrôlement sera une nouvelle forme de demande, liée au jeton consommé, refusée tant qu'elle n'existe pas.
- Révocation (R30), déjà livrée.

La règle de refus hors boucle locale n'est levée qu'une fois ces éléments livrés et testés ensemble.

## Conséquences

- v0.1 : aucune exposition réseau du lien agent, quelle que soit la configuration.
- v0.2 : un jeton volé ne sert qu'une fois et pendant 1 h ; une clé d'agent n'est jamais transmise ; la CA n'est plus dans le processus exposé.
- Reste hors périmètre de cette ADR : rotation de la clé d'un agent enrôlé. La rotation de la CA (`ciphera admin rotate-ca`) est décrite dans `docs/operations/certificats.md` : l'agent reçoit les CA de confiance sur le lien (`TrustedCas`) et le lien ne passe à la nouvelle CA qu'une fois tous les agents passés à elle.
