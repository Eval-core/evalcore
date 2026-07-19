# Typography

Two typefaces, one scale, no exceptions.

## Typefaces

| Role | Face | Source |
|---|---|---|
| UI, headings, body | Geist Variable | Self-hosted via `@fontsource-variable/geist` |
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

Headings render in the heading token (near-black light, near-white dark) at weight 600. The hero headline runs 650. Body stays at 400; bold within prose is 600. Mono weights: 400 default, 500 for labels, 700 for verdict emphasis.

## House rule

No em-dashes in shipped prose, anywhere: site copy, docs, README, UI strings, commit messages. Use a colon, a comma, or a new sentence. See [08-voice-and-writing.md](08-voice-and-writing.md).
