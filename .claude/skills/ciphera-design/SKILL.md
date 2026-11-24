---
name: ciphera-design
description: Use this skill to generate well-branded interfaces and assets for Ciphera, either for production or throwaway prototypes/mocks/etc. Contains essential design guidelines, colors, type, fonts, assets, and UI kit components for prototyping.
user-invocable: true
---

Read `docs/design-system/README.md` (from the repository root) first, then explore:
- `web/src/styles/` (tokens and global CSS),
- `web/src/design-system/components/` (design-system components, with `.d.ts` types and `.prompt.md` usage notes),
- `web/src/assets/brand/` (logos),
- `docs/design-system/specimens/` (specimen cards),
- `prototypes/dashboard/` (the existing Ciphera screens — reuse them, never redesign) and `docs/design/screens.md` (screen → route → endpoint contract).

If creating visual artifacts (slides, mocks, throwaway prototypes, etc), copy assets out and create static HTML files for the user to view. If working on production code, port components into `web/` (React 19 + TypeScript + Tailwind v4 tokens) following the rules in the README.
If the user invokes this skill without any other guidance, ask them what they want to build or design, ask some questions, and act as an expert designer who outputs HTML artifacts _or_ production code, depending on the need.
