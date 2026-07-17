---
title: FAQ
description: The philosophy and practical questions — cached responses vs mocks, running offline, evaluating a Python app, cache misses in replay, no pricing table, no Rust required, and how EvalCore compares to promptfoo, LangSmith, and Braintrust.
---

This page answers the real objections to EvalCore's design and the practical
questions that come up first.

## Can I run it offline?

Yes — that is the point of `--cache replay`. Once a suite's responses are
recorded into `.evalcore/cache.db` and committed, every subsequent run replays
from the cassette with **no network, no API keys, and $0 spend**. A miss fails
the case rather than reaching out. This is exactly how CI is meant to run:

```sh
evalcore run evals.yaml --cache replay
```

Suites that never call the network at all — `shell` targets, `trace` targets
grading recorded traces, deterministic scorers — are offline by construction,
with or without a cassette. The only things that ever need connectivity are the
first recording (`--cache auto`/`live`) and a nightly drift check. See
[Record / replay](/evalcore/guides/record-replay/).

## How do I evaluate a Python app?

You never write Rust, and you don't import an SDK — you point EvalCore at your
app over a protocol. Two common shapes:

- **Your app exposes an HTTP endpoint.** Use the `http` target: EvalCore POSTs
  each case to your `POST /chat` (or GETs a URL), pulls the answer out with a
  JSON Pointer, and caches it like an LLM call. This works for a Flask/FastAPI
  RAG service, an agent behind a gateway, anything that speaks HTTP/JSON. See
  [Evaluating REST APIs](/evalcore/guides/evaluating-rest-apis/).

- **Your app is a local script or CLI.** Use the `shell` target: EvalCore pipes
  the case input to stdin and reads stdout as the output.

  ```yaml
  targets:
    my-python-app:
      type: shell
      cmd: "python3 app.py"
  ```

And custom scoring logic — a Python faithfulness or similarity check — is a
`subprocess` scorer: EvalCore hands it `{"input", "output", "expected"}` as JSON
on stdin and reads a score back on stdout. See
[Custom scorers](/evalcore/guides/custom-scorers/).

## What happens on a cache miss in replay mode?

The case **fails with a reason** — it is never a silent live call (that would
un-determinize a replay run). The run still completes and reports every other
case; only the missing case is red:

```
FAIL new
     target error: cache miss for case "new" in replay mode — record it first with --cache auto (or live)
```

A miss means the request you are running was never recorded — usually because
you changed an input, prompt, model, or param (which changes the cache key) but
didn't re-record. Run `--cache auto` locally with credentials to record the new
request, commit the updated cassette, and CI will replay it.

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
someone reviewed — not hidden in the binary. See
[Cost and budgets](/evalcore/guides/cost-and-budgets/).

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

## How is this different from promptfoo, LangSmith, or Braintrust?

All are good tools; they solve overlapping-but-different problems. Honestly:

- **promptfoo** is the closest open-source neighbor. It is a Node-based eval
  runner whose center of gravity is **matrix comparison** — running many
  prompts × providers × cases and diffing the results, often interactively.
  EvalCore's center of gravity is **cassette-determinism and CI gating**: record
  real responses once, replay them offline and free on every PR, and gate the
  build on a stable exit code — plus first-class **agent-trajectory** evaluation
  over OTel/OpenInference traces. If you want an interactive prompt-comparison
  grid, promptfoo is excellent; if you want a deterministic test that fails a PR
  when *your* change regresses behavior, that is what EvalCore is built for.

- **LangSmith** and **Braintrust** are hosted SaaS platforms built around a
  **data flywheel**: capturing production traffic, curating datasets, running
  evals in their UI, and tracking experiments over time. They are powerful and
  they are a service — you send data to them and manage projects there.
  EvalCore is **local-first and CI-native**: no server, no signup, results in a
  SQLite file next to your repo, and the whole config is a YAML file in the same
  PR as your code. It composes *with* those platforms rather than replacing
  them — you can gate CI with EvalCore while your team's flywheel lives in a
  SaaS.

The short version: reach for EvalCore when you want an eval to behave like a
**unit test in CI** — offline, deterministic, gated on an exit code, versioned
next to the code it checks.

## Where's the agent trajectory spec?

The canonical trajectory format and the assertion-rule semantics
(`must_call`, `must_not_call`, `max_steps`) are specified in the
[trajectory format reference](/evalcore/reference/trajectory-format/) here on
this site (mirrored from the [spec on GitHub](https://github.com/eval-core/evalcore/blob/main/docs/trajectory-spec.md)).
The [Agents and traces](/evalcore/guides/agents-and-traces/) guide walks the
whole workflow.
