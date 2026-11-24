# 03 — Sécurité : failles, CVE et leçons des deux plateformes

C'est le volet le plus important pour votre projet : **presque toutes les failles des deux PaaS tombent dans 4 patterns répétitifs**. Les connaître, c'est savoir quoi ne jamais reproduire.

---

## 1. Dokploy — Chronologie des vulnérabilités 2026

| CVE | Sévérité | Type | Détail |
|---|---|---|---|
| CVE-2026-24840 | 8.0 | Identifiants codés en dur (CWE-798) | `install.sh` ligne 154 : **mot de passe PostgreSQL identique sur toutes les installations**. Corrigé en 0.26.6 |
| CVE-2026-24841 | 9.9 | Injection de commandes | WebSocket `/docker-container-terminal` : `containerId` et `activeWay` interpolés dans des commandes shell sans sanitization. Corrigé en 0.26.6 |
| CVE-2026-72740 | 9.9 | Injection (CWE-78) | `customGitUrl` passé à `ssh-keyscan` sans quoting, malgré un sanitizer qui ne quote pas sa sortie |
| CVE-2026-72733 | 9.9 | Injection | `databaseName`/`backupFile` interpolés dans les commandes de restauration (rclone, pg_restore) — exécutés **avant** validation |
| CVE-2026-72865 | 9.9 | Injection | `composePath` non validé injecté dans `docker compose -f`, `docker stack deploy`, `touch` |
| CVE-2026-72736 | 9.9 | Injection | Credentials de registry + gestion Swarm : chemin local sûr, **chemin distant (SSH) vulnérable** |
| CVE-2026-72872 | 9.9 | Injection | Sérialisation YAML Traefik interpolée dans un `echo` exécuté via SSH (fix incomplet de CVE-2026-45630) |
| CVE-2026-72875 | 8.8 | Injection | `readTraefikFile` : chemin interpolé dans `cat $(configPath)` via SSH |
| CVE-2026-72880 | 9.9 | Path traversal | Écriture de fichier hors du répertoire prévu |
| CVE-2026-72734 | — | Bypass d'autorisation | — |
| CVE-2026-86059 | — | Missing authorization (CWE-862) | Credentials Git (tokens OAuth, clés privées, secrets de webhook) **en clair** exposés aux membres d'organisation non autorisés |

**Le pattern racine (Versa Networks, oct. 2026)** : quatre des cinq RCE critiques viennent du même défaut — `execAsync`/`execAsyncRemote` exécutent des **chaînes shell construites par interpolation** de champs utilisateur. Un membre d'organisation avec une permission banale (déployer, restaurer un backup, éditer un compose) obtenait l'exécution de code sur l'hôte. Et comme le conteneur Dokploy monte `/var/run/docker.sock`, **RCE dans le conteneur = root sur l'hôte**. En multi-tenant, c'est une faille d'isolation inter-locataires complète.

## 2. Coolify — Chronologie des vulnérabilités 2025-2026

| Période | CVE | Sévérité | Type |
|---|---|---|---|
| Janv. 2025 | CVE-2025-22612 | **10.0** | Clés privées récupérables en clair → RCE sur les serveurs |
| Janv. 2025 | CVE-2025-22609 | **10.0** | Détournement de clés privées → exécution sur serveurs victimes via le Terminal |
| Janv. 2025 | CVE-2025-22611 | 9.9 | Escalade de privilèges vers le rôle owner |
| Janv. 2026 (Aikido, 7 CVE) | CVE-2025-64419 | 9.7 | Injection de commandes via le build pack Docker Compose (dépôt malveillant) |
| | CVE-2025-64420 | **10.0** | La **clé SSH privée root** lisible par tout utilisateur authentifié |
| | CVE-2025-64424 | 9.4 | Injection via les champs de configuration Git (membre bas privilège) |
| | CVE-2025-64421/22/23/25 | divers | Invitation admin par un membre, bypass de rate-limit login, liens d'invitation visibles, host header injection |
| Janv. 2026 | CVE-2025-66209 | **10.0** | Injection de commandes dans les backups de BDD |
| Printemps 2026 | CVE-2026-34034 | critique | Injection via le token Sentinel → RCE hôte |
| | CVE-2026-34038 | critique | Injection de commandes pendant le déploiement |
| | CVE-2026-34592 | haute | Bypass d'autorisation cross-team (lookup par ID sans filtre `team_id`, CWE-639) |
| Mai 2026 (SolidPoint, 4 CVE) | CVE-2026-48738/39/40 + 1 | 9.9/9.9/9.6/8.5 | **Bypass d'autorisation cross-tenant** en série (corrigés en v4.1.1) |
| Sept. 2026 | CVE-2026-84694 | 8.7 | Injection via variables d'environnement → commandes sur l'hôte |
| Sept. 2026 | CVE-2026-86117 | 9.2 | Prise de contrôle de compte via OAuth (bypass mot de passe + 2FA) |
| Oct. 2026 | "PHP2Shell" | 2 critiques | Failles PHP affectant **toutes** les versions v4 — mise à jour urgente demandée le 21 oct. |

Contexte : ~52 890 instances Coolify exposées sur Internet en janv. 2026 (Censys). Alertes émises par le Centre pour la Cybersécurité belge et l'UAE Cyber Security Council. À noter : les rapports d'Aikido et de SolidPoint soulignent que Coolify a **bien réagi** (correctifs rapides, advisories), et que plusieurs CVE étaient des PoC sans exploitation sauvage confirmée.

## 3. Les 4 patterns de faille à ne JAMAIS reproduire

### Pattern 1 — Interpolation shell (cause de ~70 % des CVE des deux projets)
Les deux plateformes construisent des commandes shell en interpolant des champs utilisateur (URL Git, chemins, noms de BDD, labels Traefik). Le shell interprète les métacaractères. **Règle d'or pour votre PaaS : jamais de chaîne de commande. Uniquement des tableaux d'arguments (`execFile`/`execve` sans shell), ou mieux : l'API Docker (dockerode/Go SDK) plutôt que le CLI.**

### Pattern 2 — Authorization manquante au niveau objet (IDOR cross-tenant)
Lookup par ID sans vérifier `team_id`/`organization_id` du demandeur. Coolify a eu toute une génération de ces failles (CWE-639/862) ; Dokploy aussi (CVE-2026-86059). **Règle : scope systématique des requêtes par tenant, tests de régression d'autorisation (accès permis / membre restreint / cross-team → 403).**

### Pattern 3 — Secrets exposés en clair
Clé SSH root lisible par un membre, credentials Git retournés par l'API, mot de passe DB codé en dur dans l'installeur. **Règle : secrets chiffrés au repos (enveloppe KMS ou âge/sops), jamais retournés par une API après écriture (write-only), jamais dans les logs (redactor obligatoire), et tout secret d'installation généré aléatoirement par machine.**

### Pattern 4 — Privilège hôte par construction
Le conteneur du PaaS monte `docker.sock` → toute RCE dans l'app devient root sur l'hôte. Les deux projets en souffrent. **Règle : séparer le plan de contrôle des exécutions privilégiées — un agent minimal sur l'hôte avec API étroite (pas de shell arbitraire), idéalement rootless Docker ou socket proxy filtrant (comme Tecnativa/docker-socket-proxy), et sandboxing des builds (sysbox/rootless/gVisor — Coolify propose déjà `InstallSysbox`).**

## 4. Leçons complémentaires

- **Mises à jour auto** : Coolify a cassé des données (écrasement de `.env`) quand l'update UI tournait en parallèle → votre PaaS doit verrouiller les upgrades (flock) et sauvegarder avant chaque mise à jour.
- **Advisories** : SolidPoint note que des advisories Coolify sont restées en brouillon 3 mois après le fix — ne pas dépendre uniquement des flux CVE pour surveiller vos dépendances.
- **Surface Internet** : les deux dashboards sont des cibles (52k+ instances exposées). Prévoyez d'office : binding réseau privé/Tailscale, IP allow-list, 2FA/passkeys obligatoires pour l'admin.
- **Threat model écrit** : la seule doc qui a vraiment aidé Coolify à trier les rapports est son threat model explicite. Écrivez le vôtre **avant** la première release publique.
