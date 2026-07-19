# Color

The palette and the rules that keep it meaningful. Machine-readable copy: `site/src/styles/tokens.css`.

## Brand accent: iris

Iris (blue-violet) is the brand. It reads as judgment, precision, and trust, which is the product's job description. It marks interaction and structure: links, buttons, active navigation, focus rings, the gate line in the mark, and the focal element of drawn diagrams.

| Role | Light | Dark |
|---|---|---|
| Accent | `#4f46e5` | `#818cf8` |
| Accent hover / high | `#3730a3` | `#c7d2fe` |
| Accent soft fill | `rgb(79 70 229 / 7%)` | `rgb(129 140 248 / 10%)` |
| Accent line | `rgb(79 70 229 / 30%)` | `rgb(129 140 248 / 35%)` |

## Semantic verdict colors

These mean something and therefore never decorate. If green appears, something passed. If red appears, something failed. No exceptions, no "green because it looks fresh."

| Verdict | Light | Dark |
|---|---|---|
| Pass | `#0b8a66` | `#2dd4a0` |
| Fail | `#d33e36` | `#f8746e` |
| Warn (flaky) | `#a16207` | `#eab308` |

Soft fills and lines derive at the same opacities as the accent derivatives.

## Secondary syntax hue

Hand-drawn code (the landing YAML, annotated snippets) uses one supporting hue so it does not render monochrome-accent: code-blue, light `#0b63b8`, dark `#8cc7ff`. Keys take the accent, string values take code-blue.

## Neutrals

| Role | Light | Dark |
|---|---|---|
| Background | `#fcfcfd` | `#0a0a0b` |
| Surface | `#f4f4f6` | `#131316` |
| Raised surface | `#ffffff` | `#18181d` |
| Body text | `#3d3d46` | `#c9c9cf` |
| Heading | `#131316` | `#f4f4f5` |
| Muted | `#6b6b76` | `#9d9da6` |
| Hairline | `#e4e4e9` | `#232329` |

## Code blocks are always dark

Code and terminal blocks render on `#0e0e11` in both themes, with fixed dark chrome (`#26262c` borders, `#9d9da6` labels). A dark panel on a light page reads as the instrument being operated, and syntax colors keep full contrast. Inline code stays theme-colored; only block surfaces are pinned dark.

## Rules

- Light is the default theme. Dark is a choice the visitor makes.
- Every text/background pair meets WCAG AA at its rendered size. Check both themes when changing any value.
- One accent per composition. A diagram or component highlights one focal element in iris; everything else is neutral.
- Never mix brand and verdict roles: a button is never green, a PASS tag is never iris.
