---
title: Running in CI
description: The full EvalCore CI story — commit cassettes, replay offline at $0, gate on regressions with baselines, add suite gates, and integrate via the GitHub Action or JUnit.
---

EvalCore is built to run in CI on every pull request: **offline, free, and
deterministic**, gating the job on the exit code. This guide walks the whole
story end-to-end.

## 1. Commit the cassettes

Every call to a cacheable target (LLM APIs, `http` endpoints) is recorded to
`.evalcore/cache.db`, a local SQLite cassette. **Commit it.** Once the responses
are recorded, CI never needs the network or an API key again:

```sh
git add .evalcore/cache.db
git commit -m "Record eval cassettes"
```

Baselines live in the same store file, so committing `.evalcore/` carries both
the cassettes and any accepted baselines into CI.

## 2. Replay offline, keyless, at $0

In CI, run with `--cache replay`: the cache is the only source of truth. A cache
hit replays the recorded response byte-for-byte; a miss **fails the case**
rather than silently calling out. No network, no API keys, no spend, no flake:

```sh
evalcore run evals.yaml --cache replay
```

Because replay never touches the network, missing API keys are fine in this
mode — that is exactly what lets CI replay a committed cache with no secrets
configured. See [Record / replay](/evalcore/guides/record-replay/) for the full
cassette lifecycle and when to re-record.

## 3. Gate on regressions, not perfection

Real eval suites are rarely 100% green — what you want to block is *getting
worse*. Save an accepted state, then gate against it:

```sh
evalcore run evals.yaml --save-baseline main     # record the accepted state
evalcore run evals.yaml --baseline main          # exit 0 iff NO regressions
```

With `--baseline`, the exit contract changes: failures already present in the
baseline are tolerated; a case that **regresses** (passed → failing) or a **new
failing** case exits `1`, with a diff:

```
baseline "main": 11/12 passed -> current: 10/12 passed
REGRESSED refund-2
     judge: answer no longer cites the policy
baseline gate: FAIL (1 regressed, 0 new failing)
```

Combine both flags for a **rolling baseline** (`--baseline main --save-baseline
main`): compare against the accepted state first, then re-record it.

## 4. Add suite gates as floors

Baselines and per-case scorers ask "did any single case fail?" Suite gates add
an absolute floor over the *whole* run — "at least 95% of cases pass", "the
judge's mean score is at least 0.8":

```yaml
run:
  gates:
    - type: pass_rate
      min: 0.95                # fraction of cases passing every scorer, in [0,1]
    - type: mean_score
      scorer: judge            # optional; omit to average all scores
      min: 0.8
```

Gates are **additive absolute floors**. The run exits `1` if the existing
contract fails (any case failed, or with `--baseline` a regression) **or** any
gate falls below its floor — so with `--baseline`, an accepted failure stays
tolerated per-case yet still sinks a `pass_rate` gate it drops below. Target-error
cases count in `pass_rate`'s denominator but contribute no scores to
`mean_score`, so pair the two to catch error storms. Outcomes print after the
summary and ride along in the JSON report:

```
GATE PASS pass_rate >= 0.95 (actual 1.00)
```

Floors compare with a `1e-9` tolerance, so a run that exactly meets its floor
passes.

## 5. The GitHub Action

One step installs the release binary (with a `cargo` fallback), runs the suite,
writes the report to the job step summary, and exits with the gate's code:

```yaml
- uses: eval-core/evalcore@v0.5.0
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
```

`config` points at your suite; `args` are passed straight to `evalcore run`.
Because the step exits with the suite's code, the job passes or fails with your
evals — no extra scripting.

## 6. HTML reports for humans

Exit codes gate the job; an HTML report is what an engineer opens to see *why*.
The `--html <path>` flag writes a **fully self-contained** report — a single
file, inline CSS, zero JavaScript — so it opens offline and works in air-gapped
environments. It is written **in addition to** the primary `--reporter` output,
not instead of it:

```sh
evalcore run evals.yaml --cache replay --html report.html
```

The report contains the summary counts (passed / failed / total, plus tokens and
cost), the suite-gate results, and an expandable row per case showing its output,
each scorer's score and reason, and — for `trace` cases — the agent's trajectory
steps. When you pass `--baseline`, the baseline diff is included too.

The GitHub Action produces this automatically. It gained an `html-artifact`
input (default `"evalcore-report"`): the Action passes `--html` for you and
uploads the file as a CI artifact **even when the run fails** — reports matter
most on a failure. Set it to `""` to disable.

```yaml
- uses: eval-core/evalcore@v0.5.0
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
    html-artifact: evalcore-report   # uploaded even on failure; "" to disable
```

## 7. Other CI systems: JUnit

For CI systems that consume JUnit XML (GitLab, Jenkins, Buildkite, …), emit a
JUnit report and let your platform surface per-case results:

```sh
evalcore run evals.yaml --cache replay --reporter junit --output results.xml
```

With `--output`, the report is written to the file and a one-line summary goes
to stderr; the process still exits `0`/`1` on the same contract, so the job gate
is unchanged. (Suite-gate outcomes are carried by the exit code — JUnit output
itself is unchanged by gates.)
