# Ecosystem sync

One brand across every surface, present and future. This file is the universal standard the others feed into.

## The rule

Every surface that says EvalCore uses the same mark, palette, type, and voice: the website, the GitHub organization and repositories, the README, the social banner, release notes, the Product Hunt launch, and any future application or extension built on this foundation. A surface that cannot follow a rule exactly (GitHub renders its own fonts, for example) follows the nearest achievable form: mark and palette always, type where possible.

## Asset inventory

| Asset | Path | Purpose |
|---|---|---|
| Favicon (SVG) | `site/public/favicon.svg` | Browser tab, theme-aware bars |
| Favicon (PNG) | `site/public/favicon.png` | Crawlers and legacy contexts |
| Touch icon | `site/public/apple-touch-icon.png` | iOS home screen |
| Social banner | `site/public/og.png` | og:image / twitter:card for evalcore.cc |
| Mark, light bg | `design/assets/mark.svg` | README, light surfaces |
| Mark, dark bg | `design/assets/mark-dark.svg` | README dark, dark surfaces |
| Mark rasters | `design/assets/mark-256.png`, `mark-dark-256.png` | Contexts that reject SVG |
| Org avatar | `design/assets/github-avatar.svg` / `.png` | GitHub organization (upload manually) |
| Banner master | `design/assets/social-preview.png` | Same file as og.png; also the GitHub repo social preview |
| Banner template | `design/explorations/social-preview.html` | Re-render source for the banner |

## Adding a new surface

1. Start from the values in `site/src/styles/tokens.css`; do not eyeball colors or spacing.
2. Reuse the mark files above. Never redraw or restyle the mark ([02-brand-identity.md](02-brand-identity.md)).
3. Verify light and dark renderings; light is the default ([03-color.md](03-color.md)).
4. Banners are 1280x640, dark background (`#0a0a0b`), dot matrix fade, the lockup, the tagline, and a mono proof strip with green ticks. Re-render from the banner template rather than composing fresh.
5. Write copy to [08-voice-and-writing.md](08-voice-and-writing.md); run shipped prose through the same rules regardless of surface.
6. Record any new decision in the log below.

## Decision log

| Date | Decision |
|---|---|
| 2026-07-19 | Bar-mark (three score bars + gate line) chosen over prompt-cursor and snapshot-diff concepts; the crossing-bar variant was tried and rejected at small sizes. |
| 2026-07-19 | Iris replaced verdict-mint as the brand accent; green demoted to pass-semantic only, red fail, amber flaky. |
| 2026-07-19 | Light-first flipped from dark-first; first visit renders light regardless of system preference. |
| 2026-07-19 | Geist Variable replaced Inter as the site face; JetBrains Mono retained for code. |
| 2026-07-19 | Code blocks pinned always-dark (#0e0e11) in both themes. |
| 2026-07-19 | Glass treatment reserved for window chrome; traffic lights drawn in CSS, never baked into recordings. |
| 2026-07-19 | Button hovers repaint nothing (no lift, no re-tint); the primary's 2px arrow slide is the only hover motion. Depth changes moved to `:active`. |
| 2026-07-19 | Feature explorer panels went two-column (copy left, visual right on a dotted stage); diagram boxes are raised cards, not bare strokes. |
