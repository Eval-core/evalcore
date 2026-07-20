# Space and layout

Rhythm comes from a fixed scale; sections are separated by whitespace, not lines.

## Spacing scale

4px base, geometric. Every margin, padding, and gap uses a step; if a value is worth using twice it is worth a token.

| Token | Value |
|---|---|
| `--sp-1` | 4px (0.25rem) |
| `--sp-2` | 8px |
| `--sp-3` | 12px |
| `--sp-4` | 16px |
| `--sp-5` | 24px |
| `--sp-6` | 32px |
| `--sp-7` | 48px |
| `--sp-8` | 64px |
| `--sp-9` | 96px |

## Measures

| Token | Value | Use |
|---|---|---|
| `--measure-prose` | 46rem | Reading column in docs |
| `--measure-splash` | 1152px | Landing sections, footer |

## Radii and borders

| Token | Value | Use |
|---|---|---|
| `--radius-sm` | 6px | Chips, inline code, small controls |
| `--radius` | 10px | Buttons, cards, code frames |
| `--radius-lg` | 16px | Glass windows, panels |

Borders are 1px hairlines from the hairline token. No 2px decorative borders, no double frames.

## Whitespace does the sectioning

Sections separate by vertical space alone. Never draw a horizontal rule under a heading: the original site put a full-width border under every h2, and combined with content margins it read as an empty broken box on every page. That defect is the cautionary tale for this rule. Semantic left rules on asides (2px) are the only sanctioned decorative border.

## Elevation

Two resting levels, tokenized; nothing else casts a shadow.

| Token | Use |
|---|---|
| `--shadow-card` | Resting cards: feature-explorer visuals, drawn-diagram boxes, CardGrid cards, small raised controls |
| `--shadow-card-hover` | The same cards on hover (interactive cards only), paired with a 2px lift |
| `--shadow-lg` | Glass windows and showpiece frames only |

Shadows are layered (a 1px contact shadow plus a soft ambient one), low-opacity, and always derived from the theme's ink color, never pure black in light mode. A card at rest is *visibly* raised; hover deepens the same shadow rather than introducing one.

## Glass

The glass treatment: translucent background, `backdrop-filter: blur(14px)`, a 1px inset top highlight, and one large soft shadow. Values live in tokens (`--glass-bg`, `--glass-border`, `--glass-highlight`, `--shadow-lg`).

Glass is reserved for window chrome: terminals, editor frames, the report embed, the CTA panel. It says "this is a window onto the product." Applying it to ordinary cards or nav dilutes that meaning; the header bar's blur is the one chrome-level exception.
