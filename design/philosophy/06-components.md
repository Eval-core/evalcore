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
- Secondary: raised surface, hairline border, text color. Hover strengthens border and shadow.
- Hover lifts, it does not re-tint. Changing a button's fill on hover makes it read as a different button; only depth moves, plus the primary's arrow sliding 2px.
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

All explanatory SVGs share one drawing language:

- Muted 1.5px strokes for structure; the single focal element takes the accent (fill accent-soft, stroke accent).
- Mono labels around 11px on a 320 to 480 unit viewBox; cap rendered width so text keeps one optical size across diagrams.
- Orthogonal elbow connectors, staggered so no two share a lane, detached 4px from every box.
- Verdict marks inside diagrams use the semantic colors (green check, red cross).
