# Components

The shared vocabulary. A guide page, the landing, and a future app should all draw from this list rather than inventing variants.

## Glass window (CastFrame)

The frame for anything that shows the product running: VHS casts, the animated hero terminal, the report embed.

- CSS traffic lights, 11px circles: `#ff5f57`, `#febc2e`, `#28c840`. Drawn by the frame, never baked into recordings (tapes render with no window bar).
- Mono title in muted at `--fs-xs`, centered or leading.
- Glass surface per [05-space-and-layout.md](05-space-and-layout.md), radius `--radius-lg`.

## Buttons

Squared (`--radius`), calm, instrument-like.

- Primary: solid accent, white text, 1px inset top highlight. Hover deepens to accent-high. Active presses in place (inset shadow), no lift.
- Secondary: raised surface, hairline border, text color. Hover strengthens border and heading color.
- Focus: visible 2px accent outline, offset 2px. Tap highlight disabled.
- Never pills, never translateY hover, never green.

## Tabs

Content tabs (installation OS picker, feature explorer) are a segmented control: hairline pill strip on surface, active segment on raised surface with a small shadow, inactive segments muted text. No underline tabs, no borders per segment.

## Chips and tags

- Informational chips (release badge, YAML segment labels): hairline pill, `--fs-xs`, muted text, optional accent dot.
- Verdict tags (PASS, flaky, FAIL): pill in the semantic color's soft fill, line border, and text. Verdict colors only ever appear here and in terminal output.

## Navigation states

- Sidebar current page: tinted accent pill (accent soft fill, accent text, weight 500). Hover on other items: faint neutral fill.
- Right TOC: entries hang off a single hairline guide; the current entry gets accent text and a 2px accent tick on the guide.
- Pagination: compact chips with direction eyebrow and page title at body size, never full-width cards.

## Copy buttons

Hidden until the block is hovered or the button focused. Ghost square (`--radius-sm`) on the block's surface. Success feedback is a small pill in the pass color with a check; no oversized "Copied!" tooltips.

## Asides

Flat surface fill, radius `--radius`, hairline border, plus a 2px semantic left rule (note, tip, caution use their Starlight semantic colors). One treatment for every callout, including "known gap" sections.

## Drawn diagrams (fx-* vocabulary)

All explanatory SVGs share one drawing language:

- Muted 1.5px strokes for structure; the single focal element takes the accent (fill accent-soft, stroke accent).
- Mono labels around 11px on a 320 to 480 unit viewBox; cap rendered width so text keeps one optical size across diagrams.
- Orthogonal elbow connectors, staggered so no two share a lane, detached 4px from every box.
- Verdict marks inside diagrams use the semantic colors (green check, red cross).
