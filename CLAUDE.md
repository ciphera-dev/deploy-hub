<!-- design-to-dev:memoire:start -->
@AGENTS.md

## Claude Code
- Les hooks de `.claude/settings.json` injectent le résumé de reprise au démarrage et, avant chaque modification, les fichiers qui dépendent de celui qu'on touche.
- La mémoire automatique de Claude Code reste personnelle : ce qui doit servir à l'équipe ou à un autre agent va dans `.agents/memory/`.

## Organisation de `.claude/`
- `skills/` : skills du projet ; celles du kit de sécurité sont épinglées (`security-kit.lock.json`, licences dans `skills/THIRD_PARTY_NOTICES.md`) et ne se modifient pas à la main.
- `agents/` : sous-agents du projet ; `commands/` : commandes `/…` du projet.
- `settings.json` : hooks et permissions ; ne jamais y désactiver un hook ni élargir une permission sans l'accord de l'utilisateur.
- Vérifier le tout : `npx design-to-dev security-kit verify .` (empreintes, consignes cachées, hooks, permissions).
<!-- design-to-dev:memoire:end -->
