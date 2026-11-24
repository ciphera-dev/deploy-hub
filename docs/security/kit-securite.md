# Kit de sécurité et configuration des agents — 2026-11-24

21 ✅ · 0 ⚠️ · 1 ❌ — skills du kit : 9

Contrôles : empreintes des skills du kit, consignes cachées et caractères invisibles dans `.claude/`, `.agents/skills/`, `.codex/` et les fichiers d'instructions, commandes exécutées au chargement d'une skill, hooks et permissions des réglages.

| | Contrôle | Détail |
|---|---|---|
| ❌ | hooks | `.codex/hooks.json` : la skill déclare des hooks (exécutés sans demander) |
| ✅ | kit | `api-security` : intacte (UnitOneAI/SecuritySkills@70bc259bb01a) |
| ✅ | kit | `dependency-scanning` : intacte (UnitOneAI/SecuritySkills@70bc259bb01a) |
| ✅ | kit | `differential-review` : intacte (trailofbits/skills@196a9073ab10) |
| ✅ | kit | `owasp-top-10-web` : intacte (UnitOneAI/SecuritySkills@70bc259bb01a) |
| ✅ | kit | `secrets-management` : intacte (UnitOneAI/SecuritySkills@70bc259bb01a) |
| ✅ | kit | `secure-code-review` : intacte (UnitOneAI/SecuritySkills@70bc259bb01a) |
| ✅ | kit | `security-audit` : intacte (cloudflare/security-audit-skill@c1c8a8c14710) |
| ✅ | kit | `sharp-edges` : intacte (trailofbits/skills@196a9073ab10) |
| ✅ | kit | `threat-modeling` : intacte (UnitOneAI/SecuritySkills@70bc259bb01a) |
| ✅ | hooks | `.claude/settings.json` : hook SessionStart « python "$CLAUDE_PROJECT_DIR/.agents/hooks/memory_hook.py" --agent claude » |
| ✅ | hooks | `.claude/settings.json` : hook PreToolUse « python "$CLAUDE_PROJECT_DIR/.agents/hooks/memory_hook.py" --agent claude » |
| ✅ | hooks | `.claude/settings.json` : hook PostToolUse « python "$CLAUDE_PROJECT_DIR/.agents/hooks/memory_hook.py" --agent claude » |
| ✅ | hooks | `.claude/settings.json` : hook Stop « python "$CLAUDE_PROJECT_DIR/.agents/hooks/memory_hook.py" --agent claude » |
| ✅ | hooks | `.claude/settings.json` : hook SubagentStop « python "$CLAUDE_PROJECT_DIR/.agents/hooks/memory_hook.py" --agent claude » |
| ✅ | hooks | `.claude/settings.json` : hook PreCompact « python "$CLAUDE_PROJECT_DIR/.agents/hooks/memory_hook.py" --agent claude » |
| ✅ | hooks | `.claude/settings.json` : hook SessionEnd « python "$CLAUDE_PROJECT_DIR/.agents/hooks/memory_hook.py" --agent claude » |
| ✅ | hooks | `.codex/hooks.json` : hook SessionStart « python3 "$(git rev-parse --show-toplevel)/.agents/hooks/memory_hook.py" --agent codex » |
| ✅ | hooks | `.codex/hooks.json` : hook PostToolUse « python3 "$(git rev-parse --show-toplevel)/.agents/hooks/memory_hook.py" --agent codex » |
| ✅ | hooks | `.codex/hooks.json` : hook Stop « python3 "$(git rev-parse --show-toplevel)/.agents/hooks/memory_hook.py" --agent codex » |
| ✅ | hooks | `.codex/hooks.json` : hook PreCompact « python3 "$(git rev-parse --show-toplevel)/.agents/hooks/memory_hook.py" --agent codex » |
| ✅ | hooks | `.codex/hooks.json` : hook SessionEnd « python3 "$(git rev-parse --show-toplevel)/.agents/hooks/memory_hook.py" --agent codex » |
