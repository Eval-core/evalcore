# Typography

Two typefaces, one scale, no exceptions.

## Typefaces

| Role | Face | Source |
|---|---|---|
| UI, headings, body | Manrope Variable | Self-hosted via `@fontsource-variable/manrope` |
| Code, data, terminal | JetBrains Mono | Self-hosted via `@fontsource/jetbrains-mono` (400/500/700) |

Both are self-hosted; the site makes no external font requests. Fallback stacks: `system-ui, -apple-system, BlinkMacSystemFont, sans-serif` and `ui-monospace, SFMono-Regular, Menlo, monospace`.

Surfaces we do not control typographically (GitHub README, crates.io) use their native system faces; the mark and palette carry the brand there.

## Scale

Minor third (1.200) off 1rem:

| Token | Size | Typical use |
|---|---|---|
| `--fs-xs` | 0.8125rem | Overlines, chips, captions |
| `--fs-sm` | 0.875rem | UI labels, secondary text, nav |
| `--fs-base` | 1rem | Body |
| `--fs-lg` | 1.125rem | Section intros |
| `--fs-xl` | 1.375rem | h2 / h3 |
| `--fs-2xl` | 1.75rem | Section headings |
| `--fs-3xl` | `clamp(2.5rem, 5.5vw, 3.6rem)` | Hero headline |

## Line height and tracking

| Token | Value | Use |
|---|---|---|
| `--lh-tight` | 1.15 | Display and headings |
| `--lh-snug` | 1.45 | UI text, captions |
| `--lh-normal` | 1.7 | Body prose |
| `--tracking-tight` | -0.03em | Display sizes |
| `--tracking-wide` | +0.08em | Uppercase overlines |

## Weight and color

Weights are tuned to Manrope, which runs softer and wider than a grotesque at equal weight: headings render in the heading token (near-black light, near-white dark) at weight 700 with -0.02em tracking (600 reads as body-bold, not a heading). The hero headline runs 800 at `--tracking-tight`. Body stays at 400. UI labels and buttons sit at 500-600; card and diagram titles at 600 (label sizes do not need the display bump). Stat figures use 700 with tabular numerals. Mono weights: 400 default, 500 for labels, 700 for verdict emphasis.

## Where mono is allowed

JetBrains Mono marks *literals*: code, config keys, flags, filenames, case ids, terminal output, and tabular data cells. Everything structural — box titles in diagrams, table headers, tags, notes, annotations, stat figures — is set in Manrope. Setting descriptive labels in mono made the drawn visuals read as debug output rather than designed UI; the split is now semantic, not aesthetic.

## House rule

No em-dashes in shipped prose, anywhere: site copy, docs, README, UI strings, commit messages. Use a colon, a comma, or a new sentence. See [08-voice-and-writing.md](08-voice-and-writing.md).
