---
title: FAQ
description: The philosophy behind EvalCore — why cached responses, why cassettes aren't mocks, why no pricing table, and whether you need to write Rust.
---

This page answers the real objections to EvalCore's design.

## What's the point of testing against cached responses?

A CI verdict is a function of two things: the **model's behavior** and your
**eval machinery** — the prompts, the target config, the datasets, the scorers,
the thresholds. On a pull request, the model isn't what changed; *you* are. The
PR-path suite exists to protect against your changes: a prompt edit that breaks
grounding, a scorer threshold that's now too loose, a dataset case you dropped.
Replaying recorded model responses holds the model constant so the test isolates
*your* diff — and does it offline, for free, deterministically.

Model **drift** — the provider quietly changing the model behind the same name —
is a real concern, but a **separate, scheduled** one. You catch it with a nightly
`--cache live` job that re-records against the live provider and surfaces the
diff, not by letting nondeterministic network calls flake every PR. See
[Record / replay](/evalcore/guides/record-replay/).

## Aren't cassettes just mocks?

No. A mock is a response *you made up* — it asserts against your assumptions
about what the model does. A cassette is a **real recorded sample of actual
model behavior**, captured the first time the request ran and then replayed
byte-for-byte. You never hand-write the expected output; you record what the
real endpoint actually returned. And the recording is honest: change the model,
URL, prompt, params, or input and the cache key changes, so a stale recording
can never masquerade as a fresh one.

## Why no shipped pricing table?

Because a bundled price list goes stale silently and produces confidently-wrong
dollar figures. Prices change, and they differ per provider, per deployment, and
per tier. EvalCore ships **no pricing table**: you declare your rates in the
target's `cost:` block, where they live in config and code review can see them.

```yaml
targets:
  openai:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    cost:
      input_per_1m: 0.40     # your provider's price per 1M tokens
      output_per_1m: 1.60
```

Cost is then `(input_tokens × input_per_1m + output_tokens × output_per_1m) / 1M`,
using the usage the provider reported. If a rate is wrong, it's wrong in a file
someone reviewed — not hidden in the binary.

## Why Rust? Do I need to write Rust?

**No — you never write Rust.** EvalCore's extension points are language-agnostic
protocols, not a Rust SDK:

- **Targets** speak HTTP or shell.
- **Custom scorers** speak JSON over stdin/stdout — write them in Python, Node,
  Go, anything.
- **Judges** are any OpenAI-compatible endpoint.
- **Agent traces** arrive as OTel/OpenInference JSON your framework already
  emits.

Rust is the *engine* — chosen for a single fast, dependency-free binary and for
determinism — but it is never the *interface*. Your entire suite is a YAML file
plus a JSONL dataset.

## Where's the agent trajectory spec?

The canonical trajectory format and the assertion-rule semantics
(`must_call`, `must_not_call`, `max_steps`) are specified in the
[trajectory spec on GitHub](https://github.com/eval-core/evalcore/blob/main/docs/trajectory-spec.md).
