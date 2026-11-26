# Exigences de sécurité

Intentions : dashboard, backend. Source : `references/stacks.md` + écrans du design.

## dashboard

- [ ] Mots de passe hachés en Argon2id (jamais MD5/SHA) — ASVS V2.4.1
- [ ] Session en cookie `HttpOnly; Secure; SameSite=Strict`, rotation de l'identifiant au login — ASVS V3.2.1, V3.4.1–3
- [ ] MFA TOTP (obligatoire pour les admins) — ASVS V2.8
- [ ] Rôles admin/utilisateur vérifiés côté serveur sur chaque route et chaque requête (contrôle d'accès objet par objet) — ASVS V4.1.1, V4.2.1
- [ ] Rate limiting (tower-governor) + verrouillage progressif du compte — ASVS V2.2.1
- [ ] Limites du second facteur, de la ré-authentification et de la connexion — ASVS V2.2.1, V2.8 :
  - TOTP : 5 échecs par compte en 15 min et 20 en 24 h, comptés pour les seules tentatives TOTP ; le plafond de 24 h n'est remis à zéro que par une réinitialisation (ou un changement) du mot de passe, jamais par une connexion réussie ;
  - passkey et code de secours : 5 échecs par session et 20 par IP en 15 min, sans lien avec les échecs TOTP du compte ;
  - ré-authentification d'une session complète : 5 échecs par session en 15 min, sans lien avec les autres sessions ;
  - connexion : un seul calcul Argon2 en vol par compte (les autres reçoivent 429 `busy`), et au-delà de 10 échecs de mot de passe en 15 min sur le compte, un délai de 1 à 30 s par tentative, sans blocage (le titulaire garde la passkey) ;
  - session en attente du second facteur : expire après 10 min.
- [ ] Jeton CSRF sur toute requête d'écriture — ASVS V4.2.2
- [ ] En-têtes de sécurité (CSP, HSTS, X-Content-Type-Options, Referrer-Policy) — ASVS V14.4
- [ ] Journal d'audit : connexions, échecs, actions admin (sans mot de passe ni donnée personnelle) — ASVS V7.1.1, V7.2.1
- [ ] Supabase si choisi : RLS activé sur toutes les tables exposées, clé service_role jamais côté client — ASVS V4.1.1

## backend

- [ ] Requêtes SQLx paramétrées uniquement (`query!` / `bind`) — ASVS V5.3.4
- [ ] Validation de chaque entrée (crate `validator`), taille max des corps de requête — ASVS V5.1.1
- [ ] JWT ≤ 15 min + refresh token en cookie HttpOnly avec rotation, ou sessions serveur — ASVS V3.5
- [ ] Rate limiting par IP et par compte — ASVS V2.2.1
- [ ] CORS : liste explicite d'origines — ASVS V14.5.3
- [ ] Secrets en variables d'environnement, `.env.example` sans valeurs — ASVS V2.10.4
- [ ] Erreurs génériques côté client, détails seulement dans les logs serveur — ASVS V7.4.1
- [ ] `cargo audit` + `npm audit` + Dependabot en CI — ASVS V14.2.1

## Exigences issues du design

- [ ] Écran de connexion détecté → anti brute-force, message d'erreur identique pour email inconnu / mauvais mot de passe
- [ ] Écran MFA détecté → codes TOTP à usage unique, codes de récupération hachés
- [ ] Upload détecté → type MIME vérifié côté serveur, taille max, stockage hors webroot, nom de fichier régénéré
- [ ] Paiement détecté → jamais de numéro de carte sur nos serveurs (Stripe Elements / Checkout), webhooks signés
- [ ] Rôles admin, user → matrice d'autorisations par endpoint, testée

## Endpoints à protéger

| Endpoint | Auth | Rôle | Validation | Rate limit |
|---|---|---|---|---|
| `GET /api/me` | oui | à définir | oui | standard |
| `POST /api/auth/login` | non | public | oui | strict |
| `POST /api/auth/logout` | oui | à définir | oui | standard |
| `POST /api/auth/mfa/setup` | oui | à définir | oui | standard |
| `POST /api/auth/mfa/verify` | oui | à définir | oui | standard |
| `POST /api/auth/refresh` | oui | à définir | oui | standard |
| `POST /api/auth/register` | non | public | oui | strict |

## Contrôle

- [ ] `python $SKILL/scripts/security_check.py scan <projet>` sans candidat ouvert
- [ ] Audit `security-audit` (Cloudflare) en mode guidance avant la mise en production
- [ ] En-têtes vérifiés sur l'environnement déployé

## Exigences propres à Ciphera (ajoutées le 2026-11-21)

### Agent de nœud et plan de contrôle
- [ ] L'agent accepte seulement les messages du schéma versionné `crates/proto`. Toute exécution arbitraire est refusée (ASVS V5, CWE-78). Le pare-feu et le scan malware passent par des actions typées, avec validation des ports, CIDR et chemins.
- [ ] mTLS obligatoire plan de contrôle ↔ agent :
  - certificats émis par la mini-PKI (`crates/pki`), rotation automatique ;
  - révocation à la suppression d'un serveur ;
  - le jeton d'enrôlement est à usage unique, expire en 15 min et n'est stocké que sous forme de hash.
  - État v0.1 : un seul agent, local ; le lien n'écoute qu'en boucle locale (refus au démarrage sinon), révocation par `ciphera admin revoke-agent`, rotation de la CA par `ciphera admin rotate-ca`. L'enrôlement par jeton est décrit pour v0.2 dans `docs/adr/0004-enrolement-des-agents.md`.
- [ ] Terminal (ADR 0002) :
  - désactivé par défaut ;
  - réservé aux rôles owner/admin, avec re-auth + MFA ;
  - session enregistrée dans l'audit ;
  - expiration après inactivité ;
  - refusable par l'agent (`terminal = false`).
- [ ] Docker via bollard uniquement, aucune commande shell construite à partir d'entrées utilisateur. Les builds BuildKit tournent avec des secrets de build non persistés dans les couches.

### Secrets et données
- [ ] Variables secrètes, tokens de connecteurs, clés S3/Supabase, clé Tailscale : chiffrement enveloppe age côté serveur, **écriture seule** (l'API ne renvoie jamais la valeur), export `.env` sans les secrets.
- [ ] Jetons API `cph_*` stockés en hash (argon2/sha-256 + sel), affichés une seule fois. Préfixe et 4 derniers caractères seulement en liste, avec permissions, expiration et liste d'IP autorisées.
- [ ] Secrets de webhook (`whsec_*`) et URL de deploy hooks : rotation possible, comparaison à temps constant, signature HMAC SHA-256 vérifiée sur les webhooks Git entrants. État v0.1 : jeton de deploy hook lu dans l'en-tête `X-Ciphera-Hook-Token` (`?token=` déprécié), expiration de 90 jours par défaut (365 au plus).
- [ ] Données personnelles (profil, sessions avec IP et géolocalisation, audit) : minimisation, export et suppression (RGPD), rétention configurable des journaux d'accès et des IP d'attaquants.
- [ ] Le contenu des tickets envoyé à un LLM tiers passe par le serveur (clés jamais côté client), avec filtrage des secrets et consentement explicite.

### Authentification
- [ ] Le premier admin est créé par l'assistant d'installation, avec un jeton de setup à usage unique, en HTTPS / localhost / tunnel seulement. L'assistant est verrouillé ensuite.
- [ ] Inscription publique désactivée par défaut ; ajout des membres par invitation.
- [ ] WebAuthn (passkeys) + TOTP (seed chiffré) + codes de secours (hashés). 2FA obligatoire activable pour l'organisation, SSO SAML/OIDC sans licence.
- [ ] Re-auth + MFA pour : reset du mot de passe root, révélation des identifiants de base de données, création de jeton, restauration, suppression d'organisation, ouverture du terminal.
  - État v0.1 : step-up mot de passe + second facteur pour la suppression de l'organisation, la création d'un deploy hook et la prolongation de son expiration, la modification des réglages SMTP et les changements d'équipe privilégiés (invitation, changement de rôle, retrait d'un membre, lien de réinitialisation du mot de passe et remise à zéro du second facteur d'un membre, R56) ; chaque changement d'équipe est notifié aux autres propriétaires quand SMTP est configuré. Les autres actions de la liste n'existent pas encore en v0.1.

### Livraison
- [ ] `install.sh` (ADR 0003) :
  - minisign obligatoire, puis SHA-256 ;
  - fonction `main` appelée à la dernière ligne, `--proto '=https' --tlsv1.2`, `mktemp` + `trap` ;
  - arrêt sans installer si une vérification échoue.
- [ ] Clé de signature des releases dans un environnement CI protégé, jamais dans le dépôt. Procédure de rotation et de révocation documentée (`docs/security/release-signing.md`).
- [ ] `latest.json` signé par la clé de release, avec une échéance (120 jours) ; vérifié par le plan de contrôle et l'updater ; re-signé chaque mois. Aucun mode test dans le script ni dans le binaire publiés.
- [ ] SPA servie avec CSP stricte, HSTS, `frame-ancestors 'none'` et sans aucune ressource tierce (CDN, polices, favicons).
- [ ] CI : `cargo audit`, `cargo deny`, `npm audit` / `osv-scanner`, `shellcheck`, `actionlint` et `npx design-to-dev ai-traces scan . --strict` avant chaque merge.

## Revue security-audit (guidance, étape 8, 2026-11-21)

Relecture de conception (pas encore de code), centrée sur les frontières de confiance. Chaque manque confirmé est reporté dans le plan `docs/superpowers/plans/2026-11-21-ciphera-v0.1.md`, avec son test.

- [ ] **Isolement réseau des apps** (Task 19, 25) : `ciphera-postgres` et `buildkitd` sur `ciphera-internal`, un réseau par projet, buildkitd en socket Unix seulement. *Risque évité : une app déployée lit la base du plan de contrôle ou pilote buildkitd.*
- [ ] **Politique Compose et BuildKit** (Task 24) : `privileged`, réseau/PID/IPC `host`, `cap_add`, `devices`, bind mounts, socket Docker et `ports` refusés ; entitlements BuildKit `security.insecure` et `network.host` refusés. *Risque évité : un rôle developer devient root sur le nœud, puis lit `age.key` et prend le contrôle de l'instance.*
- [ ] **Clone git** (Task 24) : `https://github.com/…` uniquement, `GIT_ALLOW_PROTOCOL=https`, `--no-recurse-submodules`. *Risque évité : `ext::`, `file://` et injection d'option.*
- [ ] **Unicité des hôtes** (Task 30) : un hôte appartient à un seul environnement de l'instance, et le domaine du panneau est réservé. *Risque évité : un projet capte le trafic d'un autre projet ou du panneau.*
- [ ] **Cookie `__Host-` et rotation de session** (Task 12). *Risque évité : une app sur un sous-domaine voisin pose ou écrase le cookie de session (cookie tossing), ou fixe la session.*
- [ ] **`X-Forwarded-For` de confiance** (Task 12, 31, 32) : lu seulement depuis le proxy local, et remplacé par le proxy. *Risque évité : contournement de la limitation de débit et de la liste d'IP autorisées.*
- [ ] **Anti-retour de version** (Task 33) : l'updater refuse une version inférieure ou égale à celle installée, sauf commande root explicite. *Risque évité : un processus `ciphera` compromis réinstalle une release signée mais vulnérable.*

**Lignes génériques sans objet pour Ciphera :**
- JWT + refresh : remplacés par des sessions serveur, donc pas de `/api/auth/refresh`.
- Supabase RLS : Supabase n'est pas la base de Ciphera.
- Paiement / Stripe : il n'y a aucune facturation.

La colonne « Rôle » du tableau des endpoints est remplacée par la matrice `authz::allowed` (Task 15), testée de façon exhaustive (Task 34).
