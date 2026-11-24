# Ciphera Design System

Ciphera is a self-hosted, fully open-source PaaS (Rust) — node agent, Pingora proxy + ACME, BuildKit, Railpack — positioned against Dokploy and Coolify. The UI is a dark, monochrome control panel (overview, projets, monitoring, serveurs, connecteurs, paramètres) in French.

## Sources
- `references/reference-dashboard.jpg` — a single 1200×1020 screenshot of the Efferd dashboard ("Good morning"). No codebase, Figma, font files, or logo files were provided; all values are measured/sampled from this image (scaled ×1.28 to a 1440px canvas).

## Index
> Chemins relatifs à la racine du dépôt Ciphera.

- `web/src/styles/index.css` — point d'entrée CSS (imports seulement) → `tokens/fonts.css, colors.css, typography.css, spacing.css, effects.css, base.css`
- `docs/design-system/specimens/guidelines/` — cartes specimen des fondations (couleurs, typo, espacements, marque)
- `docs/design-system/specimens/components/` — cartes specimen des composants
- `web/src/design-system/components/core/` — Icon, Button, IconButton, Select, ButtonGroup, GroupButton, Badge, Delta, LegendItem
- `web/src/design-system/components/navigation/` — Sidebar, BrandMark, WorkspaceSwitcher, NavSection, NavItem, UserMenu, Tabs
- `web/src/design-system/components/data/` — Stat, StatGroup, AreaChart, RadialGauge, TickBar, SegmentedBar, KeyValueList, DataTable, StatusDot, CodeBlock
- `web/src/design-system/components/cards/` — Panel, UpdateCard, InsightText
- `web/src/design-system/components/forms/` — TextField, RadioCard, Switch
- `web/src/design-system/components/overlay/` — Modal
- `prototypes/dashboard/` — maquettes de l'app Ciphera construites sur ce système (voir `docs/design/screens.md`)
- `web/src/assets/brand/` — logo.svg, logo-dark.svg, ciphera-icon.svg
- `.agents/skills/ciphera-design/SKILL.md` — skill agent du design system
- `docs/design-system/references/` — capture de référence et captures d'écrans représentatives

## Components
Icon, Button, IconButton, Select, ButtonGroup, GroupButton, Badge, Delta, LegendItem, Sidebar, BrandMark, WorkspaceSwitcher, NavSection, NavItem, UserMenu, Stat, StatGroup, AreaChart, RadialGauge, TickBar, SegmentedBar, KeyValueList, DataTable, StatusDot, CodeBlock, Tabs, TextField, RadioCard, Modal, Panel, UpdateCard, InsightText.

### Intentional additions
- **Icon** — wrapper rendering Lucide SVGs as currentColor masks (no icon files were provided).
- **GroupButton** — the borderless date-range segment inside ButtonGroup, split out so ButtonGroup stays generic.
- **DataTable, StatusDot** — needed for the list pages (Projets, Serveurs…) requested after the reference; styled from the dashboard's hairline/grey vocabulary.
- **Switch** — on/off settings rows (auto-deploy, PR previews) for the Projets flow.
- **Tabs, TextField, RadioCard, Modal, CodeBlock** — needed for Ciphera detail views and modals (not in the reference image).
- Select's open menu and Button hover states are not visible in the screenshot; they follow the tooltip styling.

> **Note (2026-11-21)** : l'interface est en français. Les exemples ci-dessous (« Good morning », montants en $) viennent de la capture de référence : en production, formater avec `Intl` en fr-FR (espace insécable, virgule décimale, €).

## CONTENT FUNDAMENTALS
- **Tone:** calm, factual, operator-focused. Short noun labels; numbers carry the message. A single friendly touch: the time-of-day greeting ("Good morning").
- **Casing:** Title Case for nav and buttons ("Content Calendar", "View Detail", "Manage Budget", "Ask AI"); sentence case for metric labels ("Repeat purchase rate", "Monthly recurring revenue", "Active customers"). Eyebrows are ALL CAPS ("UPDATE").
- **Person:** no "I/we/you" in UI chrome. Copy is impersonal and declarative.
- **Comparisons:** always "<delta> vs prior 30 days" / "<delta> over last 30 days".
- **Insights:** one sentence, grey, with the key quantity in white: "Unused budget runway improved by **3.5% this month** vs. trailing burn."
- **Detail lists:** label ends in a colon ("Date:", "Payment method:"); masked card numbers as "**** 4432".
- **Numbers:** $ prefix, comma thousands, two decimals for money amounts ($284,920.00), abbreviated K for headline values ($92K), one decimal for percentages (38.4%). Dates as "Apr 16 - May 15, 2026", "Sun, May 10", "March 24, 2025".
- **Emoji:** never.

## VISUAL FOUNDATIONS
- **Palette:** strictly monochrome near-black. Page #0b0b0b, sidebar #101010, controls #141414, selected/filled #272727. Text runs through five greys (#ededed → #555). Colour appears only for deltas (green #22c08a up, red #ef4444 down) and the workspace avatar's green→blue gradient. Charts are grey-on-grey (#8a8a8a / #5a5a5a / #3a3a3a / #2a2a2a), never hued.
- **Type:** one grotesk sans (Geist, substitute — see caveats). Weights 400/500 only. Metrics 22–24px/500, greeting 20px/500, nav 14px/500, body 13px, labels 12px grey, captions 11px, eyebrow 10px caps. Tabular figures on.
- **Layout:** fixed 250px sidebar; main surface is a rounded-left (20px) panel inset against the sidebar colour with a 1px #1f1f1f border. Header spans full width; content is a fluid left column + 372px right rail. Sections are separated by 1px #161616 hairlines — no boxed cards in the main content. Generous padding (28–46px).
- **Backgrounds:** flat dark fills. The only gradient is the chart area fill (white 10% → 0%). No imagery, textures, or patterns.
- **Motif:** repetition of thin ticks — radial gauge bars, barcode tick bar, dotted chart grid, legend underlines (dashed).
- **Corners:** controls 8px, sidebar cards / workspace switcher 10px, main panel 20px, badges 6px, date pill and segment bars fully round.
- **Borders:** 1px; #2a2a2a on controls, #1f1f1f on cards, #161616 for dividers.
- **Shadows:** almost none. Controls have a 1px inner top highlight; the tooltip has a soft black drop + 1px ring.
- **Transparency/blur:** none observed.
- **Hover:** background steps up one grey (#141414 → #1c1c1c, #272727 → #2f2f2f); ghost buttons brighten text. **Press:** no shrink; same as hover. **Active nav:** #272727 fill, white text.
- **Animation:** not observable; use quick 120–180ms ease-out colour transitions only. No bounces.
- **Charts:** line #7a7a7a 1.5px, cursor line grey, point disc grey with black ring, tooltip dark card, active x-label becomes a white pill with black text.

## ICONOGRAPHY
- Thin-stroke, rounded line icons at 14–16px in muted grey (white when active). They match **Lucide** closely (layout-grid, calendar, megaphone, chart-column, users, plug, circle-help, book-open, chevrons-up-down, chevron-down, sliders-horizontal, ellipsis, wallet, arrow-right, message-circle, layers, chart-pie). **Substitution:** loaded from CDN `lucide-static@0.460.0` via the Icon component; original icon files not provided.
- Arrows in CTAs ("View Detail →") use the arrow-right icon.
- Delta indicators are small filled discs with an arrow (drawn inline in Delta).
- No emoji, no icon font, no unicode icon glyphs.
- **Logo:** official Ciphera wordmark — `../../web/src/assets/brand/logo.svg` (light, #ededed) and `../../web/src/assets/brand/logo-dark.svg` (#0b0b0b). Use via `BrandMark` / `CipheraLogo` components. Never redraw or recolour beyond the two greys.


### Third-party marks
- `../../web/src/assets/images/cloudflare/cloudflare-mark.png` (nuage seul), `../../web/src/assets/images/cloudflare/cloudflare-logo-light.png` (wordmark recoloré pour fond sombre), `../../web/src/assets/images/cloudflare/cloudflare-logo.webp` (original fourni). Orange Cloudflare `#f38020` réservé aux surfaces du connecteur Cloudflare.
- `assets/os/` — logos des systèmes (ubuntu, debian, fedora, linux) fournis par l'utilisateur ; `assets/db/` — logos des moteurs de bases de données.
