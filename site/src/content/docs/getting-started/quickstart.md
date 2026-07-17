---
title: Quickstart
description: Run your first EvalCore suite in five minutes — no API keys, no network — then swap in a real LLM target.
---

This is the five-minute path, with **no API keys and no network**. The shipped
`examples/quickstart/` suite evaluates a `cat` echo target, so you can see the
whole loop — config, dataset, run, exit code — before wiring up a real model.

## The suite

An eval suite is a YAML file plus a JSONL dataset. Here they are verbatim.

`examples/quickstart/evals.yaml`:

```yaml
targets:
  echo:
    type: shell
    cmd: "cat"

datasets:
  - file: cases.jsonl

scorers:
  - type: contains
    value: "refund"
    case_sensitive: false
```

- **`targets`** — what's evaluated. `shell` runs a command with the case input
  piped to stdin; stdout is the output. `cat` echoes its input straight back.
- **`datasets`** — JSONL files of test cases, resolved relative to the config
  file.
- **`scorers`** — how each output is judged. `contains` passes when the output
  contains `value`; `case_sensitive: false` makes the match ignore case.

`examples/quickstart/cases.jsonl`:

```jsonl
{"id": "refund-1", "input": "How do I get a refund for my order?"}
{"id": "refund-2", "input": "Please process my REFUND today."}
```

Each line is one case: an `id` and an `input`. The `cat` target echoes `input`
back, and the `contains` scorer checks the echo for `refund`.

## Run it

```sh
evalcore run examples/quickstart/evals.yaml
```

Both cases echo their input, and both inputs contain "refund" (case-insensitive,
so `REFUND` matches too):

```
PASS refund-1 (0ms)
PASS refund-2 (0ms)

2 passed, 0 failed, 2 total
```

## Read the output

Each case prints `PASS <id>` or `FAIL <id>`; a failing case lists the reason
from each scorer that failed underneath it. The final line is the summary:
passed / failed / total. (A shell target does no token accounting, so there is
no cost line — that appears once a target reports usage.)

## The exit-code contract

`evalcore run` exits **`0` when every case passes** and **`1` otherwise**. That
is the entire CI contract — gate your pipeline on the exit code:

```sh
evalcore run examples/quickstart/evals.yaml && echo "all passed"
echo "exit code: $?"
```

A target that errors is not a crash: it becomes a failed case with a reason, and
the run still finishes and exits `1`. Failures are data — see
[Core concepts](/evalcore/getting-started/core-concepts/).

## The leap to a real LLM target

Swap the shell target for an `openai-compatible` one — same datasets, same
scorers. Secrets stay out of the YAML: reference an environment variable by name
with `api_key_env`, and EvalCore reads it at run time.

```yaml
targets:
  my-app:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    system: "You are a support agent. Answer in one sentence."
    params:
      temperature: 0
```

Every call to this target is recorded to a local cassette and replayed on the
next run — free, offline, deterministic. That is what makes even LLM-graded
suites usable as CI gates; see [Record / replay](/evalcore/guides/record-replay/).
