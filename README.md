<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="design/assets/mark-dark.svg">
    <img src="design/assets/mark.svg" alt="" width="88" height="88">
  </picture>
</p>

<h1 align="center">EvalCore</h1>

<p align="center">
  <strong>Know when your AI gets worse, before your users do.</strong>
</p>

<p align="center">
  <a href="https://github.com/eval-core/evalcore/actions/workflows/ci.yml"><img src="https://github.com/eval-core/evalcore/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/evalcore"><img src="https://img.shields.io/crates/v/evalcore.svg" alt="crates.io"></a>
  <a href="https://evalcore.cc"><img src="https://img.shields.io/badge/docs-evalcore.cc-2dd4a0" alt="Documentation"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-informational" alt="Apache-2.0"></a>
</p>

<p align="center">
  <img src="design/assets/social-preview.png" alt="EvalCore: snapshot testing for AI behavior. Know when your AI gets worse, before your users do." width="760">
</p>

<p align="center">
  <img src="site/public/casts/quickstart.gif" alt="EvalCore grading a support bot against the policy each answer must cite: four cases pass, a compliance gate holds, and the run exits 0, all offline." width="760">
</p>

---

EvalCore is snapshot testing for AI behavior. Change a prompt, swap a model,
bump a dependency. Did anything break? You usually find out from a user.

EvalCore records how your AI behaves and checks every change against that
recording. It is one binary, driven by a YAML file, and it runs offline for $0,
so an eval suite can be a blocking check on every pull request instead of a
weekly job someone remembers to run.

You never write Rust to use it. Targets speak HTTP or shell, custom scorers speak
JSON over stdin and stdout, and judges are any OpenAI-compatible endpoint.

**[Documentation](https://evalcore.cc/)** ·
[Quickstart](https://evalcore.cc/getting-started/quickstart/) ·
[crates.io](https://crates.io/crates/evalcore) ·
[Releases](https://github.com/eval-core/evalcore/releases) ·
[Changelog](CHANGELOG.md)

> **Status:** pre-1.0. Config and APIs may still shift between minor versions.

## Contents

- [Install](#install)
- [Quickstart](#quickstart)
- [Reading the output](#reading-the-output)
- [How it works](#how-it-works)
- [What you can do with it](#what-you-can-do-with-it)
- [Targets and scorers](#targets-and-scorers)
- [Running in CI](#running-in-ci)
- [Design principles](#design-principles)
- [Maintainers](#maintainers)
- [Contributing](#contributing)

## Install

```sh
cargo install evalcore
```

Or download a prebuilt binary for Linux x64 or macOS (x64 / arm64) from the
[releases page](https://github.com/eval-core/evalcore/releases). Full options in
the [installation guide](https://evalcore.cc/getting-started/installation/).

## Quickstart

The repository ships a runnable example: a small bank support bot, graded on
whether every answer cites the policy it relied on. It needs no API key and makes
no network calls.

```sh
git clone https://github.com/eval-core/evalcore
cd evalcore
cargo run -p evalcore -- run examples/quickstart/evals.yaml
```

```
PASS late-refund (8ms)
PASS fee-dispute (8ms)
PASS card-lost (8ms)
PASS wire-eta (8ms)

4 passed, 0 failed, 4 total
GATE PASS pass_rate >= 0.95 (actual 1.00)

PASSED
```

On an interactive terminal the status words are colored (green pass, red fail),
a spinner counts cases on stderr while the run works, and the closing `PASSED`
/ `FAILED` is bold, but the words are always there, so nothing depends on
color. Piped, redirected, or under `NO_COLOR`, the output is exactly the plain
text above: deterministic, greppable, and safe to paste into a pull request.
`--color auto|always|never`, `--progress auto|never`, and `-q/--quiet` (failures
only) tune it; machine reporters and `--output` files are never colored.

A suite is two files. The YAML says what to run and how to grade it:

```yaml
# evals.yaml
targets:
  support-bot:
    type: shell                       # your real app goes here
    cmd: "sh examples/quickstart/bot.sh"

datasets:
  - file: cases.jsonl

scorers:
  - type: contains                    # grounding: cite a policy
    value: "policy"
    case_sensitive: false
  - type: regex                       # specificity: cite a numbered rule
    pattern: "policy [0-9.]+"

run:
  gates:
    - type: pass_rate                 # a floor over the whole run
      min: 0.95
```

The JSONL holds the cases. `context` is the retrieved evidence a RAG suite grades
against. Scorers see it, targets never do:

```jsonl
{"id": "late-refund", "input": "It has been three weeks and my refund still has not shown up.", "context": ["Policy 4.2: Approved refunds are processed within 30 business days."]}
{"id": "wire-eta", "input": "How long will an international wire transfer take?", "context": ["Policy 5.3: International wire transfers settle within 3 to 5 business days."]}
```

## Reading the output

Every character of the terminal report means something specific. Here is a run
with one failing case, annotated.

**Per-case lines**, one per case, always in dataset order:

```
PASS late-refund (8ms)
──┬─ ─────┬───── ──┬─
  │       │        └─ latency, measured by the target itself
  │       └────────── case id, straight from your cases.jsonl
  └────────────────── every scorer passed

FAIL fee-dispute
     exact: expected "60 days", got "we will look into it"
     ──┬──  ─────────────────────┬──────────────────────
       │                         └─ that scorer's reason, verbatim
       └─ which scorer objected (one line per failing scorer)
```

A case that never produced output reports the cause instead of a score, because
a target error is a failed case with a reason, not a crash:

```
FAIL wire-eta
     target error: connection refused
```

**The summary line**, then one line per gate:

```
2 passed, 1 failed, 3 total · 210 tokens · $0.0020 · 1 flaky
─────────┬──────────────────   ────┬─────   ───┬───   ───┬──
         │                         │           │         └─ cases whose trials
         │                         │           │            disagreed with each other
         │                         │           └─ your declared rates × those tokens
         │                         └─ reported by the provider (or read from a trace)
         └─ a case passes when every scorer passes

GATE FAIL pass_rate >= 0.95 (actual 0.67)
──┬─ ──┬─ ────────┬───────   ────┬─────
  │    │          │              └─ what the run actually scored
  │    │          └─ the floor you declared under run.gates
  │    └─ this gate's verdict
  └─ gate lines appear only when you configure gates
```

The last three segments of the summary line appear only when they apply: tokens
and cost when the run reported usage, `flaky` when a case ran multiple trials.
A run with none of them prints just `4 passed, 0 failed, 4 total`.

**The verdict**, always last, one word:

```
FAILED · 2 regressed, 1 new
──┬───   ────────┬─────────
  │              └─ a clause only when a baseline explains the failure
  └─ PASSED / FAILED, matching the exit code exactly
```

It reflects the *whole* contract (cases, gates, and any baseline), so it is
never at odds with the exit code. `evalcore run` exits **0** when the verdict is
`PASSED` and **1** otherwise. That is the whole CI contract.

Other output formats: `--reporter json` for the full result tree,
`--reporter junit` for CI test panels, and `--html report.html` for a
self-contained page you can attach to a pull request. See the
[CLI reference](https://evalcore.cc/reference/cli/).

## How it works

```
   cases.jsonl              evals.yaml
        │                        │
        │   id, input,           │   targets, scorers,
        │   expected, context    │   gates, trials
        └───────────┬────────────┘
                    ▼
            ┌───────────────┐        ┌──────────────────────────┐
            │    TARGET     │◄──────►│  record / replay cache   │
            │               │        │  .evalcore/cache.db      │
            │ shell · http  │        │                          │
            │ openai · trace│        │  keyed on a hash of the  │
            └───────┬───────┘        │  canonical request       │
                    │                └──────────────────────────┘
                    │ output text, tokens, latency, trajectory
                    ▼
            ┌───────────────┐
            │    SCORERS    │   run in order, all of them, every case
            │               │
            │ contains · exact · regex · json-schema · similarity
            │ judge · subprocess · trajectory
            └───────┬───────┘
                    │ a case passes when every scorer passes
                    ▼
            ┌───────────────┐
            │     GATES     │   floors over the whole run
            │               │   pass_rate · mean_score · accuracy · macro_f1
            └───────┬───────┘
                    │
                    ▼
              exit 0  or  1   ──►  CI
```

Three properties hold everywhere, and they are the reason the tool is useful
rather than merely convenient:

**Determinism.** Identical inputs produce identical outputs. Results stay in
dataset order, reporters are pure functions, and nothing user-visible reads the
clock except latency. That is what makes the cache trustworthy.

**Record once, replay forever.** Every call to a cacheable target is recorded to
a local SQLite file, keyed on a hash of the canonical request. Commit that file
and CI replays it, so the job makes no network calls, needs no API keys, and
costs nothing.

```sh
evalcore run evals.yaml                  # auto (default): replay hits, record misses
evalcore run evals.yaml --cache replay   # CI: cache only, a miss fails the case
evalcore run evals.yaml --cache live     # re-record everything
evalcore run evals.yaml --cache off      # bypass
```

Changing the model, the URL, or a case's input changes the key, so a stale
recording can never quietly answer for a request you did not make. Shell targets
are never cached, because they run local code, which can change without the
config changing.

**Failures are data.** A target error becomes a failed case with a reason, a
scorer error becomes a failing score with a reason. A run never panics, and one
bad case never aborts the suite.

## What you can do with it

| You want to | Use | Guide |
|---|---|---|
| Block regressions instead of demanding perfection | `--baseline` | [Gates and baselines](https://evalcore.cc/guides/gates-and-baselines/) |
| Set a quality floor over the whole run | `run.gates` | [Gates and baselines](https://evalcore.cc/guides/gates-and-baselines/) |
| Stop trusting a single lucky sample | `run.trials` | [Trials and statistics](https://evalcore.cc/guides/trials-and-statistics/) |
| Decide whether the cheaper model is good enough | `--matrix a,b` | [Comparing models](https://evalcore.cc/guides/comparing-models/) |
| Grade what an agent *did*, not just what it said | `trace` + `trajectory` | [Agents and traces](https://evalcore.cc/guides/agents-and-traces/) |
| Evaluate your own deployed REST API | `type: http` | [Evaluating REST APIs](https://evalcore.cc/guides/evaluating-rest-apis/) |
| Grade against retrieved context | case `context` | [RAG evaluation](https://evalcore.cc/guides/rag-evaluation/) |
| Score with a rubric no assertion can express | `type: judge` | [LLM-as-judge](https://evalcore.cc/guides/llm-as-judge/) |
| Track spend and cap it | `cost` + `budget_usd` | [Cost and budgets](https://evalcore.cc/guides/cost-and-budgets/) |
| Score in Python, or any language | `type: subprocess` | [Custom scorers](https://evalcore.cc/guides/custom-scorers/) |
| Browse past runs and diff any two | `evalcore serve` | [Run history](https://evalcore.cc/guides/run-history-and-serve/) |

Three of these are worth seeing in their real output.

**Baselines gate on getting worse.** Save an accepted state, then compare. Cases
that were already failing stay tolerated; only genuine regressions fail the run:

```
baseline "main": 2/3 passed -> current: 0/3 passed
REGRESSED refund-window
     exact: expected "30 days", got "I do not know"
REGRESSED wire-eta
     exact: expected "3 to 5 days", got "I do not know"
baseline gate: FAIL (2 regressed, 0 new failing)
```

All three cases are failing here, but only two are reported. The third was
already failing when the baseline was saved.

**Trials measure instead of sampling.** One run of a stochastic model is one
sample. `run.trials` runs each case N times and folds the results with
`all`, `majority`, or `any`. The same unstable case, under two policies:

```
run:  trials: { count: 3, require: majority }
      PASS fee-dispute (10ms) [2/3 trials]
      3 passed, 0 failed, 3 total · 1 flaky        exit 0

run:  trials: { count: 3, require: all }
      FAIL fee-dispute [2/3 trials]
           exact: trial 2: expected "60 days", got "unsure"
      2 passed, 1 failed, 3 total · 1 flaky        exit 1
```

Either way you learn the case is flaky. `require` only decides whether that
fails your build.

**A matrix compares targets side by side** in one invocation: two models, two
prompts, or two deployed endpoints.

```
== comparison
case             baseline-bot    improved-bot
refund-window    PASS            PASS            tie
fee-dispute      FAIL            PASS            improved-bot
wire-eta         PASS            PASS            tie
wins: baseline-bot 0 · improved-bot 1 · ties 2
```

## Targets and scorers

A **target** is the thing under evaluation.

| Type | Evaluates | Cached |
|---|---|---|
| `shell` | Any command. The case input arrives on stdin, stdout is the output. | No |
| `openai-compatible` | Any OpenAI-format chat endpoint: OpenAI, vLLM, Ollama, a gateway. | Yes |
| `http` | Any HTTP/JSON API, typically your own deployed app. | Yes |
| `trace` | A recorded agent run, in EvalCore's native format or an OTel / OpenInference export. Nothing is invoked. | No |

A **scorer** decides whether an output is acceptable. Every scorer runs on every
case, and a case passes only when all of them pass.

| Type | Passes when | Score |
|---|---|---|
| `contains` | The output contains a substring. | 0 or 1 |
| `exact` | The output equals `value`, or the case's `expected`. | 0 or 1 |
| `regex` | A regular expression matches anywhere in the output. | 0 or 1 |
| `json-schema` | The output parses as JSON and validates against a draft 2020-12 schema. | 0 or 1 |
| `similarity` | Cosine similarity between the output and `expected` clears a threshold. | the cosine |
| `judge` | An LLM grading against your rubric scores at or above a threshold. | the judge's score |
| `subprocess` | Your own command prints a passing verdict. Any language. | your score |
| `trajectory` | An agent's tool calls satisfy every rule (`must_call`, `must_not_call`, `max_steps`). | 0 or 1 |

Judge and similarity calls go through the same record/replay cache as targets, so
LLM-graded suites replay deterministically and for free. Every field of every
type is documented in the
[configuration reference](https://evalcore.cc/reference/configuration/).

## Running in CI

One step runs a suite and gates the job. The terminal report lands in the step
summary, and a self-contained HTML report is uploaded as an artifact a reviewer
can open straight from the pull request:

```yaml
- uses: eval-core/evalcore@v0.7.5
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
    html-artifact: evalcore-report   # default; set to "" to disable
```

`--cache replay` means the job needs no API keys and spends nothing.
`--baseline main` means it fails on regressions rather than on imperfection. The
HTML report uploads even when the suite fails, which is when it matters.

The action is a convenience, not a requirement. The binary's exit code is the
whole contract, so any CI system works. See
[Running in CI](https://evalcore.cc/guides/running-in-ci/) for
GitLab, Jenkins, and bare-shell setups.

## Design principles

**Protocols over SDKs.** Every extension point is language-agnostic. Targets
speak HTTP or shell, custom scorers speak JSON over stdin and stdout, judges are
any OpenAI-compatible endpoint, agent traces arrive as OTel or OpenInference
JSON. Rust is the engine, never the interface.

**Config first.** Features begin as YAML. If something is worth doing, it is
worth describing as data that a reviewer can read in a diff.

**Deterministic by construction.** Same inputs, same bytes out. The cache,
baselines, and CI gating are all built on that.

**Local first.** Your suite, your recordings, and your run history live in a
SQLite file next to your repository. There is no server to run and no account to
create, and the tool sends nothing anywhere.

## Maintainers

EvalCore is built by [Abhishek Manyam](https://github.com/abhishekmanyam) and
[Kuladeep Mantri](https://github.com/kuladeepmantri).

## Contributing

Bug reports, feature requests, and pull requests are welcome. Start with
[CONTRIBUTING.md](CONTRIBUTING.md) for the workspace layout, the four
architectural rules, and the checks CI runs. Security issues go through
[SECURITY.md](SECURITY.md), not the public tracker.

```sh
cargo build
cargo nextest run --workspace                          # or: cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## License

Apache-2.0. See [LICENSE](LICENSE).
