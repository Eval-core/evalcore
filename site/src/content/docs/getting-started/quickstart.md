---
title: Quickstart
description: "Grade a real support bot against its policy context in five minutes, with no API keys and no network. Run the shipped suite, read every line, add an HTML report, then swap in your own app."
---

Five minutes, **no API keys and no network**. You will run a real eval suite (a
mini bank support bot graded against the policy each answer must cite), read
every line of the output, turn it into a shareable HTML report, and then swap in
your own app. Everything here runs against a local shell target, so nothing
leaves your machine.

## 1. Install

One binary, nothing to run as a service:

```sh
cargo install evalcore
```

No Rust toolchain? Grab a prebuilt binary from
[GitHub Releases](https://github.com/eval-core/evalcore/releases) instead. See
[Installation](/evalcore/getting-started/installation/) for every path.

## 2. Run the shipped suite

The repo ships a ready-to-run example at `examples/quickstart/`. Run it:

```sh
evalcore run examples/quickstart/evals.yaml
```

```
PASS late-refund (12ms)
PASS fee-dispute (11ms)
PASS card-lost (10ms)
PASS wire-eta (10ms)

4 passed, 0 failed, 4 total
GATE PASS pass_rate >= 0.95 (actual 1.00)
```

That is a whole eval suite passing. A support bot answered four customer
questions, each answer was graded against the policy it had to cite, and a
suite-level compliance gate held, all offline and in a few milliseconds.

## 3. What just happened

The suite is one YAML file plus a JSONL dataset. Here is the config
(`examples/quickstart/evals.yaml`, abridged):

```yaml
targets:
  support-bot:
    type: shell
    cmd: "sh examples/quickstart/bot.sh"

datasets:
  - file: cases.jsonl

scorers:
  - type: contains          # grounding: the answer must cite a policy
    value: "policy"
    case_sensitive: false
  - type: regex             # specificity: it must cite a *numbered* policy
    pattern: "policy [0-9.]+"

run:
  gates:
    - type: pass_rate       # compliance floor over the whole run
      min: 0.95
```

- `targets` is what's evaluated. `shell` runs a command with each case's
  input piped to stdin; whatever it writes to stdout is the answer. `bot.sh` is
  a stand-in support agent that returns canned, policy-grounded answers, so the
  suite runs with no model and no network. This is where your real app goes
  (step 5).
- `datasets` are JSONL files of test cases, resolved relative to the config
  file. Each case carries an `id`, the customer's `input`, and a `context`
  policy chunk: the retrieved passage the answer is supposed to be grounded in.
- `scorers` decide how each answer is judged. `contains` passes when the output
  mentions a policy at all; `regex` insists it cites a *specific* numbered rule
  (`policy 4.2`), so the answer is auditable. A case passes only when **every**
  scorer passes.
- `run.gates` set a floor over the whole suite. `pass_rate: 0.95` means at
  least 95% of cases must pass, and `GATE PASS pass_rate >= 0.95 (actual 1.00)`
  reports that the run cleared it.

Here is one case from `cases.jsonl`:

```jsonl
{"id": "late-refund", "input": "It has been three weeks and my refund still has not shown up. Where is it?", "context": ["Policy 4.2: Approved refunds are processed within 30 business days, counted from the date the return is approved."]}
```

The bot answers "…refunds are processed within 30 business days per policy 4.2…",
which contains `policy` (grounding) and matches `policy [0-9.]+` (specificity),
so `late-refund` passes both scorers.

## 4. Read every line

- `PASS late-refund (12ms)` gives the case id, then the target's latency in
  milliseconds. Every passing case is one line.
- `4 passed, 0 failed, 4 total` is the summary line.
- `GATE PASS pass_rate >= 0.95 (actual 1.00)` is a gate line. Every gate prints
  its own line with the threshold and the actual value, so a `GATE FAIL` tells
  you exactly how far short the run fell.

`evalcore run` **exits `0` when every case passes and its gates hold, `1`
otherwise**. That is the entire CI contract. A target that *errors* (a 500, a
timeout) is not a crash: it becomes a failed case with a reason, the run still
finishes and reports every other case, and it still exits `1`. Failures are
data. See [Core concepts](/evalcore/getting-started/core-concepts/).

## 5. Turn it into a shareable report

Add `--html` and EvalCore writes a self-contained HTML report alongside the
terminal output (it never replaces it):

```sh
evalcore run examples/quickstart/evals.yaml --html report.html
```

That is one file, with no assets and no server, that you can open in a browser or
attach to a PR. It shows the pass/fail summary, the gate outcomes, and every
case's answer with its per-scorer scores expandable inline. In CI, the same
report is the audit artifact a reviewer clicks straight from the pull request.
More in [HTML reports](/evalcore/guides/html-reports/).

## 6. Wire up your real app

Swap the shell target for an `openai-compatible` one. The datasets and scorers
stay exactly the same. Secrets never live in the YAML: name an environment
variable with `api_key_env` and EvalCore reads it at run time.

```yaml
targets:
  support-bot:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    system: "You are a support agent. Cite the relevant policy in every answer."
    params:
      temperature: 0
```

The first run goes live against the model and **records every call to a local
SQLite cassette**. Commit that cassette, and CI runs `--cache replay`: every
call is served from the recording, with no network, no API key, and no flaky
judge, so even LLM-graded suites run offline, deterministically, and for $0.
That is what turns an eval suite into a blocking CI check. See
[Record / replay](/evalcore/guides/record-replay/) for the cassette lifecycle
and the four cache modes.

## Where next

- [What teams use it for](/evalcore/getting-started/what-teams-use-it-for/):
  the real jobs EvalCore is built for, end to end.
- [Comparing models](/evalcore/guides/comparing-models/): run one suite across
  several targets with `--matrix` and get per-case winners and per-target cost.
- [Trials and statistics](/evalcore/guides/trials-and-statistics/): a
  stochastic model needs more than one sample; gate on how *often* a case passes.
