# Principles

The stance behind every EvalCore surface. Everything else in this folder is these ideas made specific.

## Instrument, not brochure

EvalCore is a measuring tool, and its surfaces should feel like one: precise, calm, legible. A visitor should feel they are looking at the product working, not at claims about it. That is why the landing page runs a real terminal, embeds a real report, and shows real output from the real binary. When choosing between explaining and demonstrating, demonstrate.

## Light first

Light is the default theme. Dark is a deliberate choice a visitor makes, not an assumption we make for them. Both themes are first-class and every change is verified in both, but design decisions start from the light rendering. The one standing exception: code blocks are always dark, in both themes (see [03-color.md](03-color.md)).

## The accent is judgment, green means pass

The brand accent (iris) marks interaction and structure: links, active states, buttons, the gate line in the mark. Green belongs exclusively to a passing verdict, red to a failing one. Neither ever decorates. If green appears anywhere on a surface, something passed. This split is the design system's most load-bearing rule; breaking it makes verdicts unreadable.

## Determinism as an aesthetic

The product's core promise is that identical inputs produce identical outputs. The design carries the same temperament: fixed-height animation containers, no layout shift, no randomness, no surprise. A page should render the same way every visit.

## Minimal but posh

Restraint over flourish, but never sloppy. Hairline borders, one accent, generous whitespace, glass reserved for window chrome. No 3D illustrations, no gradient meshes as decoration, no startup-launch clichés, no marketing fluff. The polish budget goes into typography, spacing, and drawn diagrams that explain the product.

## Developers, not marketing

Copy is written for someone who will run the tool an hour after reading. Claims are checkable, numbers are real, jargon is the product's own vocabulary (targets, scorers, cassettes, gates) and nothing else. See [08-voice-and-writing.md](08-voice-and-writing.md).

## One ecosystem

The site, the GitHub organization, the README, the banner, and every future application ship the same mark, palette, type, and voice. A person moving between surfaces should never wonder whether they are still looking at EvalCore. The sync rules live in [09-ecosystem-sync.md](09-ecosystem-sync.md).
