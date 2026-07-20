---
title: Running in CI
description: "The full EvalCore CI story: commit cassettes, replay offline at $0, gate on regressions, and split PR checks from nightly drift detection."
---

EvalCore is built to run in CI on every pull request: **offline, free, and
deterministic**, with the job gated on the exit code. This guide walks the whole
story end to end and ends with copy-paste workflows for GitHub Actions
(PR + nightly), GitLab, and Jenkins.

## 1. Commit the cassettes

Every call to a cacheable target (LLM APIs, `http` endpoints) is recorded to
`.evalcore/cache.db`, a local SQLite cassette. **Commit it.** Once the responses
are recorded, CI never needs the network or an API key again.

Step by step, the first time:

```sh
# 1. Run locally with real credentials so every case records a response.
export OPENAI_API_KEY=sk-...
evalcore run evals.yaml --cache auto        # replay hits, record misses

# 2. The recordings now live in .evalcore/cache.db — commit them.
git add .evalcore/cache.db evals.yaml cases.jsonl
git commit -m "Record eval cassettes"
```

Baselines live in the same store file, so committing `.evalcore/` carries both
the cassettes and any accepted baselines into CI. Treat `.evalcore/cache.db`
like a lockfile: a reviewed artifact that pins behavior. See
[Record / replay](/guides/record-replay/) for the full lifecycle.

## 2. Replay offline, keyless, at $0

In CI, run with `--cache replay`: the cache is the only source of truth. A cache
hit replays the recorded response byte-for-byte; a miss **fails the case**
rather than silently calling out. No network, no API keys, no spend, no flake:

```sh
evalcore run evals.yaml --cache replay
```

Because replay never touches the network, missing API keys are fine in this
mode. That is what lets CI replay a committed cache with no secrets
configured.

## 3. Gate on regressions, not perfection

Real eval suites are rarely 100% green, so what you want to block is *getting
worse*. Save an accepted state, then gate against it:

```sh
evalcore run evals.yaml --save-baseline main     # record the accepted state
evalcore run evals.yaml --baseline main          # exit 0 iff NO regressions
```

With `--baseline`, the exit contract changes: failures already present in the
baseline are tolerated; a case that **regresses** (passed → failing) or a **new
failing** case exits `1`, with a diff:

```text
baseline "main": 2/2 passed -> current: 1/2 passed
REGRESSED greeting
     contains: expected output to contain "refund", got: "Hello there, nice weather today."
baseline gate: FAIL (1 regressed, 0 new failing)
```

Combine both flags for a **rolling baseline** (`--baseline main --save-baseline
main`): compare against the accepted state first, then re-record it. The full
semantics (regressed vs new-failing vs fixed vs removed, and when to
re-baseline) are in [Gates and baselines](/guides/gates-and-baselines/).

## 4. Add suite gates as floors

Baselines and per-case scorers ask "did any single case fail?" Suite gates add
an absolute floor over the *whole* run: "at least 95% of cases pass", or "the
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

Gate outcomes print after the summary and ride along in the JSON report:

```text
1 passed, 1 failed, 2 total
GATE FAIL pass_rate >= 0.95 (actual 0.50)
GATE PASS mean_score(contains) >= 0.4 (actual 0.50)

FAILED
```

Gates are **additive absolute floors**: the run exits `1` if the existing
contract fails (any case failed, or with `--baseline` a regression) **or** any
gate falls below its floor. JUnit output is unchanged, because the exit code
carries the gate result.

## 5. The GitHub Action

One step installs the release binary (with a `cargo` fallback), runs the suite,
writes the report to the job step summary, and exits with the gate's code:

```yaml
- uses: eval-core/evalcore@v0.7.5
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
```

`config` points at your suite; `args` are passed straight to `evalcore run`.
Because the step exits with the suite's code, the job passes or fails with your
evals, without any extra scripting.

### A full PR workflow

```yaml
# .github/workflows/evals.yml
name: Evals
on:
  pull_request:

jobs:
  evals:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4        # brings the committed .evalcore/cache.db
      - uses: eval-core/evalcore@v0.7.5
        with:
          config: evals/evals.yaml
          args: --cache replay --baseline main
          # html-artifact defaults to "evalcore-report" — a shareable report is
          # uploaded even when the suite fails. Set to "" to disable.
```

No `OPENAI_API_KEY` secret is configured, and it does not need to be: replay is
offline and keyless. The run is free and deterministic on every PR.

## 6. HTML reports for humans

Exit codes gate the job; an HTML report is what an engineer opens to see *why*.
The GitHub Action produces one automatically via its `html-artifact` input
(default `"evalcore-report"`): it passes `--html` for you and uploads the file
as a CI artifact **even when the run fails**, since reports matter most on a
failure.

```yaml
- uses: eval-core/evalcore@v0.7.5
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
    html-artifact: evalcore-report   # uploaded even on failure; "" to disable
```

Locally or in any other pipeline, add `--html report.html` to write the same
self-contained document. Full details in
[HTML reports](/guides/html-reports/).

## 7. Split PR from nightly: catching model drift

There are two distinct failure modes, and they belong on two different
schedules:

- **Your changes**: a prompt edit, a scorer threshold, a dropped case. Caught
  on the PR path with `--cache replay`, offline and deterministic.
- **Model drift**: the provider silently changes the model behind the same
  name, so the *same request* now returns a *different response*. This must
  **not** flake your PRs. Catch it on a schedule with `--cache live`, which
  re-records against the live provider and surfaces the diff.

A real nightly drift-detection workflow:

```yaml
# .github/workflows/evals-nightly.yml
name: Evals (nightly drift)
on:
  schedule:
    - cron: "0 7 * * *"     # 07:00 UTC daily
  workflow_dispatch:         # allow manual runs

jobs:
  drift:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: eval-core/evalcore@v0.7.5
        with:
          config: evals/evals.yaml
          # live: call the real provider and re-record; compare to the accepted
          # baseline so drift shows up as a regression in the diff.
          args: --cache live --baseline main
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
```

The nightly job is the *only* place that needs the API key and spends money.
When it goes red, a real model change has moved your evals. Review the
re-recorded cassette diff, and if you accept it, commit the refreshed
`.evalcore/cache.db` (and re-baseline if appropriate). The PR path never touches
the network.

## 8. Other CI systems: JUnit

For CI systems that consume JUnit XML (GitLab, Jenkins, Buildkite, …), emit a
JUnit report and let your platform surface per-case results:

```sh
evalcore run evals.yaml --cache replay --reporter junit --output results.xml
```

With `--output`, the report is written to the file and a one-line summary goes
to stderr; the process still exits `0`/`1` on the same contract, so the job gate
is unchanged. (Suite-gate outcomes are carried by the exit code; JUnit output
itself is unchanged by gates.)

### GitLab CI

```yaml
# .gitlab-ci.yml
evals:
  image: debian:stable-slim
  before_script:
    - apt-get update && apt-get install -y curl
    - curl -fsSL "https://github.com/eval-core/evalcore/releases/download/v0.7.5/evalcore-v0.7.5-x86_64-unknown-linux-gnu.tar.gz" | tar -xz -C /usr/local/bin
  script:
    - evalcore run evals/evals.yaml --cache replay --baseline main --reporter junit --output report.xml
  artifacts:
    when: always
    reports:
      junit: report.xml
```

GitLab renders the JUnit report inline on the merge request, and the job's exit
code gates the pipeline.

### Jenkins

```groovy
// Jenkinsfile (declarative)
pipeline {
  agent any
  stages {
    stage('Evals') {
      steps {
        sh 'evalcore run evals/evals.yaml --cache replay --baseline main --reporter junit --output report.xml'
      }
      post {
        always { junit 'report.xml' }
      }
    }
  }
}
```

`sh` fails the stage on a non-zero exit code, and the `junit` step publishes
per-case results to the Jenkins UI.

## See also

- [Record / replay](/guides/record-replay/): the full cassette
  lifecycle, cache key, and team workflow for committing the cache.
- [Gates and baselines](/guides/gates-and-baselines/): the exact
  regressed/new-failing/fixed/removed semantics behind `--baseline`.
- [HTML reports](/guides/html-reports/): the `--html` flag and
  report contents the Action uploads as an artifact.
- [CLI reference](/reference/cli/): every flag used across the
  GitHub Action, GitLab, and Jenkins examples.
