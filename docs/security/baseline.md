# Baseline sécurité — tri des candidats (2026-11-21)

Source : `docs/security/baseline.json` (`security_check.py scan`) et `docs/security/fondations.md`. Chaque entrée est classée **faux positif**, **maquette seulement** (à ne pas reproduire au portage) ou **à corriger**.

## Candidats du scan

| # | Candidat | Emplacement | Tri | Exigence au portage |
|---|---|---|---|---|
| 1 | Clé `service_role` Supabase côté client (CWE-798) | `prototypes/dashboard/Connectors.jsx:167` | maquette seulement | La configuration du connecteur Supabase (URL et clé `service_role` pour les sauvegardes) est saisie dans le panneau, envoyée **une fois** à l'API, chiffrée avec age et jamais renvoyée ni stockée côté navigateur. Le mock actuel l'écrit en `localStorage` : ne pas reproduire. |
| 2 | URL `http://` (CWE-319) | `prototypes/dashboard/GlobalVariables.jsx:16` | faux positif (donnée factice) | La valeur d'exemple `OTEL_EXPORTER_OTLP_ENDPOINT` pointe vers un collecteur interne. En production, recommander TLS vers le collecteur. |
| 3 | URL `http://` (CWE-319) | `prototypes/dashboard/install.html:101` | à corriger au portage | L'assistant propose un panneau en HTTP quand HTTPS est désactivé. Exigence : **le jeton de setup et le compte admin ne transitent jamais en clair**. L'assistant n'est accessible qu'en HTTPS (certificat ACME ou auto-signé épinglé affiché par install.sh), en `localhost`, ou par tunnel SSH. HSTS dès que le domaine est configuré. |
| 4 | `Math.random` pour un secret (CWE-338) | `prototypes/dashboard/ProjectLogs.jsx:22` | faux positif | Il ne sert qu'à générer des logs factices. Au portage, tout secret ou jeton passe par `crypto.getRandomValues` / `OsRng`. Même règle pour la génération de clés SSH de `ServersPage.jsx` (`Math.imul`, pseudo-aléatoire) : WebCrypto obligatoire. |
| 5 | Pas de CSP visible (CWE-1021) | — | à valider au déploiement | Le plan de contrôle sert la SPA avec une CSP stricte (`default-src 'self'`, pas d'`unsafe-inline` en production), `frame-ancestors 'none'`, `X-Content-Type-Options`, `Referrer-Policy`. |
| 6 | HTTPS / HSTS / en-têtes (CWE-693) | — | à valider au déploiement | Test `curl -I` en CI sur une instance de test. |

## ⚠️ des fondations

| ⚠️ | Tri | Conclusion |
|---|---|---|
| `prototypes/vendor/ds-bundle.js` minifié, sans licence | maquette seulement | Bundle généré par l'outil de design à partir des sources de `web/src/design-system/` (les empreintes des sources correspondent). Utilisé uniquement par les maquettes, jamais par la production. Il sera supprimé quand les maquettes seront gelées. |
| Blocs base64 dans `logo.svg`, `logo-dark.svg`, `ciphera-icon.svg`, `favicon.svg` | vérifié, sans danger | Ce sont des **manifestes C2PA** (Content Credentials) dans `<metadata>` : provenance signée par Anthropic, indiquant que le fichier a été fourni par Claude. Pas de script, pas de lien externe, pas de `foreignObject`. Les garder ou les retirer est une décision utilisateur (voir `.agents/memory/etat.md`). |
| Commandes `curl … \| sh` dans README, ADR, stack.md, maquettes, CodeBlock.prompt.md | voulu (documentation) | C'est la commande d'installation décidée (ADR 0003). Elle ne doit jamais être exécutée par un agent. La commande d'installation réelle utilise `get.ciphera.ch` (README, ADR, stack.md) ; les maquettes (`prototypes/`) gardent des domaines d'exemple `example.com`, non servis, à unifier au portage sur le même script signé. |

## Ne pas reproduire au portage (repris dans `exigences.md`)

1. **Secrets côté navigateur.** Les mocks écrivent en `localStorage` les tokens des connecteurs, les clés S3 et l'état Cloudflare (`ciphera.connectors`, `ciphera.cloudflare`, `ciphera.crowdsec`). En production, rien de tout cela ne doit être stocké côté client.
2. **Dépendances CDN à l'exécution :**
   - unpkg : React, Babel, d3, topojson, lucide-static ;
   - jsDelivr : world-atlas ;
   - flagcdn.com, Google Favicons (s2), Google Fonts.

   Tout doit être embarqué. Le binaire fonctionne hors ligne, sans fuite vers des tiers.
3. **Code d'exécution dans le navigateur.** Babel exécuté dans le navigateur et builds React de développement.
4. **Révélation des identifiants.** Mots de passe de bases de données affichables : seulement après re-auth, avec audit.
5. **Inscription publique** sur une instance auto-hébergée : désactivée par défaut, le premier admin passe par l'assistant, les autres par invitation.
6. **Assistant d'installation** : verrouillé dès qu'un admin existe.
