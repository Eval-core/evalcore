# Voice and writing

EvalCore writes like a careful engineer explaining a tool they built, to someone who will run it within the hour.

## The stance

- Claims are checkable. If the site says the replay costs $0 and takes 0ms, that is real recorded output, not an illustration. Every terminal frame on the site is genuine binary output; the embedded report is a real artifact from the shipped example, unedited.
- Numbers are real. No rounded-up vanity metrics, no invented benchmarks.
- The product's own vocabulary (targets, scorers, cassettes, gates, trials, baselines) is the only jargon. Define a term once where it first matters, then use it plainly.

## Sentence rules

- Short sentences. One idea each.
- No marketing adjectives (powerful, seamless, blazing, robust, cutting-edge).
- No rule-of-three padding. Two reasons are two reasons.
- No em-dashes. Use a colon, a comma, or a new sentence.
- No "isn't just X" constructions, no "X. But better." patterns.
- Second person for instructions ("commit the cassette"), plain declaratives for facts.

## Headlines

Outcome first, mechanism second. The pattern is the hero: "Know when your AI gets worse, before your users do." A headline states what the reader gets; the supporting line states how. Section headings are short noun or verb phrases, not full pitches.

## UI text

- Labels are lowercase-calm: "offline replay", "exit 0", "record / replay". Uppercase only for overlines, tracked wide.
- Buttons name the action: "Get started", "Copy", "Open full report". Never "Learn more" when a precise verb exists.
- Error and empty states name the problem and the next action, in that order ("no baseline found: record one with --save-baseline main").

## Attribution

Maintainers (Abhishek Manyam, Kuladeep Mantri) appear subtly: footer baseline, README maintainers section, structured metadata. Never in headlines or hero copy; the product is the subject.
