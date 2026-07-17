---
title: Core concepts
description: The EvalCore mental model — targets, datasets, scorers, runs, the exit-code contract, and failures-as-data.
---

An EvalCore suite has four moving parts, declared in one `evals.yaml`: **targets**
produce outputs, **datasets** supply inputs, **scorers** judge the outputs, and a
**run** block ties it together with concurrency, budgets, and gates. Everything a
feature does starts as config surface — the YAML file is the interface.

## Targets — what's evaluated

A target is the thing under test. You select one per run (with `--target <name>`
when a suite defines several). There are four types:

| Type | What it does |
|---|---|
| `openai-compatible` | POSTs to `{url}/chat/completions` in the OpenAI wire format. Supports `model`, `system`, pass-through `params`, `api_key_env`, retries, `timeout_seconds`, and `cost` rates. |
| `http` | Calls any HTTP/JSON endpoint — typically your own deployed app's REST API — and caches it like an LLM call. `{{input}}` is substituted into the `url` and `body`; `response_path` pulls the answer out of the JSON. |
| `shell` | Runs a command with the case input piped to stdin; stdout is the output. Never cached — it runs your local code. |
| `trace` | Ingests a recorded agent trace (native or OTel/OpenInference) named by each case, instead of invoking anything. Pair with the `trajectory` scorer. |

```yaml
targets:
  openai:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
```

Secrets never live in the YAML: `api_key_env` names an environment variable,
resolved at run time.

## Datasets — the inputs

Datasets are JSONL files, one test case per line, merged in listed order. Each
case has an `id` and, depending on the target, an `input` or a `trace`:

```jsonl
{"id": "refund-1", "input": "How do I get a refund for my order?"}
```

For `trace` targets, each case names a trace file instead:

```jsonl
{"id": "refund-flow", "trace": "traces/run1.json"}
```

Results always come back in dataset order — determinism is the product.

## Scorers — how outputs are judged

Every scorer runs on every case. They form a ladder from cheap and deterministic
to fully custom:

- **Deterministic checks** — `contains` (substring, with `case_sensitive`),
  `exact` (equals `value`, or the case's `expected` field), `regex` (matches a
  pattern). No network, instant, perfectly reproducible.
- **`subprocess`** — the any-language escape hatch. Your command receives
  `{"input", "output", "expected"}` as JSON on stdin and prints
  `{"score": 0.0..=1.0, "passed"?: bool, "reason"?: string}` on stdout. Write
  scorers in Python, Node, Go — anything that reads stdin and writes stdout.
- **`judge`** — LLM-as-judge. Grades the output against a `rubric` using any
  OpenAI-compatible endpoint, with a configurable pass `threshold`. Judge calls
  go through the record/replay cache, so replayed verdicts are deterministic —
  which is what makes LLM-graded suites usable as CI gates.
- **`trajectory`** — asserts on an agent's path (tool calls, ordering, step
  budget). Requires a `trace` target.

```yaml
scorers:
  - type: contains
    value: "30 days"
  - type: subprocess
    cmd: python3 scorers/faithfulness.py
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    rubric: "Is the answer grounded in the provided context?"
    api_key_env: OPENAI_API_KEY
    threshold: 0.7
```

## Runs — concurrency, budgets, gates

The optional `run` block controls execution:

```yaml
run:
  concurrency: 4          # max in-flight cases (default 4)
  budget_usd: 5.0         # stop dispatching new cases past this spend
  gates:
    - type: pass_rate
      min: 0.95
    - type: mean_score
      scorer: judge
      min: 0.8
```

- **`concurrency`** bounds how many cases run at once (default 4).
- **`budget_usd`** stops dispatching new cases once accumulated cost reaches the
  cap (requires the target to declare `cost` rates). Skipped cases are reported
  as failures with a reason — the run completes rather than aborting.
- **`gates`** are absolute floors over the whole run — `pass_rate` (fraction of
  cases passing every scorer, in `[0,1]`) and `mean_score` (mean scorer value,
  optionally restricted to one `scorer`). They are additive to the per-case
  contract. See [Running in CI](/evalcore/guides/running-in-ci/).

## The exit-code contract

`evalcore run` exits **`0` when every case passes** and **`1` otherwise**. With
`--baseline`, the contract flips to "no regressions" (accepted failures are
tolerated). `run.gates` are additive on top: dropping below any floor also
exits `1`. Users gate CI on this exit code — nothing else.

## Failures are data

A run never panics and one bad case never aborts the suite:

- A **target error** (a 500, a timeout, a budget skip) becomes a failed case
  with a reason.
- A **scorer error** (a subprocess that crashes, an un-parseable judge verdict)
  becomes a failing score with a reason.

The run always finishes, reports every case in order, and lets the exit code
carry the verdict. That is why an eval suite is safe to run unattended in CI.
