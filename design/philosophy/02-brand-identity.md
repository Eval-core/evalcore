# Brand identity

The mark, the lockup, and the rules for using them.

## The mark

Three score bars forming an E, measured against a gate line. The bars are the evidence (per-case scores), the line is the threshold a passing run clears, and together they read as the letter E. The whole story of the product in four shapes.

Geometry, on a 64-unit viewBox:

| Element | x | y | width | height | rx |
|---|---|---|---|---|---|
| Bar 1 | 10 | 13 | 34 | 10 | 5 |
| Bar 2 | 10 | 27 | 24 | 10 | 5 |
| Bar 3 | 10 | 41 | 30 | 10 | 5 |
| Gate line | 50 | 11 | 6 | 42 | 3 |

Color roles:

- The bars follow text color (near-black on light, near-white on dark). In inline SVG they use `currentColor`.
- The gate line carries the brand accent (iris) and nothing else. See [03-color.md](03-color.md) for values.

## Files

All shipping variants live in `design/assets/`:

| File | Use |
|---|---|
| `mark.svg` | Light backgrounds (dark bars, light-mode iris line) |
| `mark-dark.svg` | Dark backgrounds (light bars, dark-mode iris line) |
| `github-avatar.svg` / `.png` | Org avatar: the mark on a rounded tile, background `#0e0e11` |
| `mark-256.png`, `mark-dark-256.png` | Raster fallbacks at 256 px |

The site renders the mark inline (`site/src/components/Logo.astro`) so it follows the theme automatically. The favicon (`site/public/favicon.svg`) is the same geometry with a `prefers-color-scheme` bar swap.

## The lockup

Mark plus wordmark. The wordmark is the word EvalCore set in Geist at weight 600 with letter-spacing -0.02em, in heading color, with a gap of about 0.625rem between mark and word. The wordmark is type, not a drawing; never convert it to outlines or restyle it per surface.

## Usage rules

- Minimum sizes: 16 px favicon, 20 px inline, 24 px lockup. Below that, use nothing rather than a mushy mark.
- Never recolor the bars. They are text-colored or they are wrong.
- Never rotate, skew, outline, or add effects to the mark.
- The gate line is always the brand accent. It is the only accent element in the mark.
- On busy or photographic backgrounds, put the mark on its tile (the avatar variant) instead of floating it.
- Alternative concepts from the exploration rounds (`design/explorations/logo-variations.html`) are reference only; V1 is the mark.
