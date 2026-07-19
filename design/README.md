# EvalCore design system

This folder is the canonical source for every design decision EvalCore ships. The website, the GitHub organization, the README, the social banner, and any future application or extension all derive from what is written here. If a surface disagrees with these documents, the surface is wrong. Change the decision here first, then propagate it.

## How to use this folder

Building or restyling anything user-facing:

1. Read [philosophy/01-principles.md](philosophy/01-principles.md) for the stance.
2. Pull exact values from the topic files below. They match `site/src/styles/tokens.css`, which is the machine-readable copy of the same decisions.
3. Reuse the files in [assets/](assets/) for any logo, icon, or banner need. Do not redraw the mark.
4. Verify the result in both themes, light first.

## Contents

| File | Covers |
|---|---|
| [philosophy/01-principles.md](philosophy/01-principles.md) | The stance everything else follows |
| [philosophy/02-brand-identity.md](philosophy/02-brand-identity.md) | The mark, lockup, and usage rules |
| [philosophy/03-color.md](philosophy/03-color.md) | Brand accent, semantic verdict colors, neutrals |
| [philosophy/04-typography.md](philosophy/04-typography.md) | Typefaces, scale, weights |
| [philosophy/05-space-and-layout.md](philosophy/05-space-and-layout.md) | Spacing scale, measures, radii, glass |
| [philosophy/06-components.md](philosophy/06-components.md) | The component vocabulary |
| [philosophy/07-motion.md](philosophy/07-motion.md) | Durations, easing, reduced motion |
| [philosophy/08-voice-and-writing.md](philosophy/08-voice-and-writing.md) | How EvalCore writes |
| [philosophy/09-ecosystem-sync.md](philosophy/09-ecosystem-sync.md) | Keeping every surface in sync; decision log |

## Other folders

- `assets/` holds the shipping brand files (marks, avatar, banner). Inventory in [philosophy/09-ecosystem-sync.md](philosophy/09-ecosystem-sync.md).
- `explorations/` holds working files from design rounds (logo variation sheets, banner render templates). Reference material, not shipping assets.
