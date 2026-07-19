# Motion

Motion carries information or it does not ship.

## Values

| Token | Value | Use |
|---|---|---|
| `--dur-fast` | 150ms | Hovers, focus, color shifts |
| `--dur` | 250ms | Reveals, tab switches |
| `--dur-slow` | 600ms | Panel entrances |
| `--ease-out` | `cubic-bezier(0.22, 1, 0.36, 1)` | Everything |

## Rules

- Motion must explain something. The hero terminal types because that is the product running; the explorer panel fades in to mark the content change. Decoration-only animation does not ship.
- Every animation honors `prefers-reduced-motion: reduce` and degrades to a meaningful static final frame, not a blank. The hero terminal shows the completed run; spinning reels stop; cursors stop blinking.
- Animation containers have fixed dimensions so playback never shifts layout. The terminal screen is a fixed-height box for exactly this reason.
- No scroll-jacking, no parallax, no autoplay with sound, no motion loops faster than the eye wants (the slow aurora drift runs 14s).
- Recorded casts pace for comprehension: a beat of stillness at the start, real command typing speed, and enough hold on the final frame to read the verdict.
