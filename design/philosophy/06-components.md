# Components

The shared vocabulary. A guide page, the landing, and a future app should all draw from this list rather than inventing variants.

## Glass window (CastFrame)

The frame for anything that shows the product running: VHS casts, the animated hero terminal, the report embed.

- CSS traffic lights, 11px circles: `#ff5f57`, `#febc2e`, `#28c840`. Drawn by the frame, never baked into recordings (tapes render with no window bar).
- Mono title in muted at `--fs-xs`, centered or leading.
- Glass surface per [05-space-and-layout.md](05-space-and-layout.md), radius `--radius-lg`.

## Buttons

Squared (`--radius`), calm, instrument-like.

- Primary, light theme: accent gradient (a white-tinted top edge falling to the flat accent), white text, 1px inset top highlight.
- Primary, dark theme: the relationship inverts. The dark accent is a light indigo, so a white-tinted gradient under white text washes out. Fill stays bright and the ink goes near-black (`#14141b`).
- Secondary: raised surface, hairline border, text color.
- Hover repaints nothing: no fill change, no border change, no shadow growth. The lift-on-hover pass read as the button flashing a lighter surface. The only hover signal is the primary's arrow sliding 2px; depth changes are reserved for `:active`.
- Focus: visible 2px accent outline, offset 2px. Tap highlight disabled.
- Never pills, never translateY hover, never green.

## Tabs

Content tabs (installation OS picker) are a segmented control: hairline pill strip on surface, active segment on raised surface with a small shadow, inactive segments muted text. No underline tabs, no borders per segment.

The feature explorer uses a **vertical rail** instead. Horizontal pills gave the selected tab the only filled surface, and because the labels differ in length ("Record / replay" against "Cost"), the active tab read as a physically bigger button. A rail makes every tab the same width by construction: selection changes color and adds a 2px accent edge marker, never geometry. Below 60rem the rail lies down into a horizontal scroller and the marker moves to the bottom edge.

**Rule for any tab set: selection may change color, fill, and weight, but never metrics.** If the selected state changes size, the control is wrong.

## Chips and tags

- Informational chips (release badge, YAML segment labels): hairline pill, `--fs-xs`, muted text, optional accent dot.
- Verdict tags (PASS, flaky, FAIL): pill in the semantic color's soft fill, line border, and text. Verdict colors only ever appear here and in terminal output.

## Navigation states

- Sidebar current page: tinted accent pill (accent soft fill, accent text, weight 500). Hover on other items: faint neutral fill.
- Right TOC: entries hang off a single hairline guide; the current entry gets accent text and a 2px accent tick on the guide.
- Pagination: compact chips with direction eyebrow and page title at body size, never full-width cards.

## Copy buttons

One control, one shape, everywhere: a 1.75rem bordered square at `--radius-sm`, pinned to the top-right of whatever it copies. Docs code blocks and landing snippets use the same chip, so a reader learns it once. Inside a rounded pill (the install command) the chip goes round to match the field.

It is **always present**, at reduced opacity when idle and full opacity when the block is hovered or the button focused. A control that is invisible until hover is one users assume is missing.

Success feedback stays neutral: a dark chip reading "Copied", with only the check glyph in the pass color. The button itself never turns green. Green is a run verdict, not an interaction state.

Placement is pinned explicitly for every frame type (bare, titled, terminal), because expressive-code's own default drifts between them and produces a copy button in a different corner on different pages.

## Asides

Flat surface fill, radius `--radius`, hairline border, plus a 2px semantic left rule (note, tip, caution use their Starlight semantic colors). One treatment for every callout, including "known gap" sections.

## Drawn diagrams (fx-* vocabulary)

All explanatory visuals — SVG or DOM — share one drawing language:

- Boxes are raised cards (raised surface, control border, resting `--shadow-card` — as a drop-shadow filter in SVG), not bare strokes, so drawings carry the same weight as real UI. The single focal element takes the accent (fill accent-soft, stroke accent).
- Connectors are muted 1.5px strokes with solid arrowheads; **orthogonal only** (vertical/horizontal runs and elbows, never diagonals), staggered so no two share a lane, detached 4px from every box. Fan-outs route through a horizontal bus line, not spokes.
- Boxes sit on a column grid: every box center aligns with a column or the diagram's axis, and boxes in the same row share one size. If a box cannot land on the grid, the layout is wrong, not the grid.
- Labels follow the typography rule ([04-typography.md](04-typography.md)): box titles in Manrope at 600 around 12.5px optical size, muted 11px sublabels; mono only for literals (filenames, flags, config keys). Cap rendered width (34rem) so text keeps one optical size across diagrams.
- Verdict marks use the semantic colors (green check, red cross); the accent never marks a verdict, and green never fills a meter that isn't a pass measure — spend meters fill with the accent.
- In the feature explorer each visual sits on a **stage**: a hairline plate carrying the same fading dot matrix as the hero, stretched to the panel's height. Copy takes the narrow left column, the visual the wide right one; the panel keeps a fixed floor so tab switches never resize it.
