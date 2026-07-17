---
title: Quickstart
description: Build and run your first EvalCore suite in five minutes — no API keys, no network — read every line of output, make a case fail on purpose, then swap in a real LLM target.
---

Five minutes, **no API keys and no network**. You will create a suite from
scratch, run it, read every line of the output, deliberately break a case to
see the exit-code contract fire, and then learn where to go next. Everything
here runs against a local `cat` echo target, so nothing leaves your machine.

If you would rather run the shipped example than type it out:

```sh
evalcore run examples/quickstart/evals.yaml
```

## 1. Create the suite

Make a directory and drop two files in it — the config and the dataset.

```sh
mkdir evalcore-quickstart && cd evalcore-quickstart
```

`evals.yaml` — the suite:

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
  piped to stdin; whatever it writes to stdout is the output. `cat` echoes its
  input straight back, which makes the loop observable with zero dependencies.
- **`datasets`** — JSONL files of test cases, resolved relative to the config
  file (not your shell's working directory).
- **`scorers`** — how each output is judged. `contains` passes when the output
  contains `value`; `case_sensitive: false` makes the match ignore case.

`cases.jsonl` — the dataset, one JSON object per line:

```jsonl
{"id": "refund-1", "input": "How do I get a refund for my order?"}
{"id": "refund-2", "input": "Please process my REFUND today."}
```

Each line is one case: an `id` (used in reports and baselines) and an `input`.
The `cat` target echoes `input` back, and the `contains` scorer checks that
echo for `refund`.

## 2. Validate before running

```sh
evalcore validate evals.yaml
```

`validate` parses and checks the config without executing anything:

```
OK: 1 target(s), 1 dataset(s), 1 scorer(s)
```

## 3. Run it

```sh
evalcore run evals.yaml
```

Both cases echo their input, and both inputs contain "refund" (case-insensitive,
so `REFUND` matches too):

```
PASS refund-1 (9ms)
PASS refund-2 (7ms)

2 passed, 0 failed, 2 total
```

## 4. Read every line

- **`PASS refund-1 (9ms)`** — the case id, then the target's latency in
  milliseconds. Every passing case is one line.
- **`2 passed, 0 failed, 2 total`** — the summary line, always last.
- There is **no cost line** here. A `shell` target does no token accounting, so
  tokens and `$` only appear once a target reports usage (an LLM or `trace`
  target with `cost:` rates).

## 5. Make a case fail on purpose

Change the dataset so one input no longer contains "refund". Replace
`cases.jsonl` with:

```jsonl
{"id": "refund-1", "input": "How do I get a refund for my order?"}
{"id": "greeting", "input": "Hello there, nice weather today."}
```

Run again:

```sh
evalcore run evals.yaml
echo "exit code: $?"
```

```
PASS refund-1 (58ms)
FAIL greeting
     contains: expected output to contain "refund", got: "Hello there, nice weather today."

1 passed, 1 failed, 2 total
exit code: 1
```

A **failing case** prints `FAIL <id>` followed by an indented reason from every
scorer that failed — and the reason names both what was expected and what was
seen, so you never have to re-run to find out why. The process **exits `1`**.

## 6. The exit-code contract

`evalcore run` exits **`0` when every case passes** and **`1` otherwise**. That
is the entire CI contract — gate your pipeline directly on the exit code:

```sh
evalcore run evals.yaml && echo "all green"
```

A target that *errors* (a 500, a timeout, an over-budget skip) is not a crash:
it becomes a failed case with a reason, the run still finishes and reports every
other case, and it still exits `1`. Failures are data — see
[Core concepts](/evalcore/getting-started/core-concepts/).

## 7. The leap to a real LLM target

Swap the shell target for an `openai-compatible` one — the datasets and scorers
stay exactly the same. Secrets never live in the YAML: reference an environment
variable by name with `api_key_env`, and EvalCore reads it at run time.

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
suites usable as CI gates.

## Where next

- [Core concepts](/evalcore/getting-started/core-concepts/) — the run pipeline,
  the scorer ladder, and failures-as-data.
- [Record / replay](/evalcore/guides/record-replay/) — the cassette lifecycle
  and the four cache modes.
- [Running in CI](/evalcore/guides/running-in-ci/) — commit cassettes, replay
  at `$0`, and gate on regressions.
- [Evaluating REST APIs](/evalcore/guides/evaluating-rest-apis/) — point
  EvalCore at your own deployed app.
- [LLM-as-judge](/evalcore/guides/llm-as-judge/) — grade open-ended answers
  against a rubric.
- [Configuration reference](/evalcore/reference/configuration/) — every field
  of `evals.yaml`.
