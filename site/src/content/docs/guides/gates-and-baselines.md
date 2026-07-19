---
title: Gates and baselines
description: Baseline save, compare and rolling; what regressed/new-failing/fixed/removed mean; pass_rate and mean_score gates; the 1e-9 tolerance and no-scores failure mode; the additive exit-code truth table; and when to re-baseline.
---

Two mechanisms let you gate CI on more than "did every case pass". Baselines
tolerate accepted failures and block *regressions*. Suite gates enforce absolute
floors over the whole run. They are additive, and this guide covers both, plus
exactly how they fold into the exit code.

## Baselines: block regressions, not imperfection

Real eval suites are rarely 100% green. What you want to block is *getting
worse*. Save an accepted state, then gate against it:

```sh
evalcore run evals.yaml --save-baseline main     # record the accepted state
evalcore run evals.yaml --baseline main          # exit 0 iff NO regressions
```

Baselines are stored in the same `.evalcore/cache.db` as the cassettes (commit
it, and CI gates offline). A baseline is a pure per-case snapshot (which cases
passed, which failed) with no gate results or timing baked in.

## Reading a baseline diff

`--baseline` compares the current run to the stored snapshot, case by case,
matched by `id`. Here is a real diff, where a case that passed in the baseline
now fails:

```text
baseline "main": 2/2 passed -> current: 1/2 passed
REGRESSED greeting
     contains: expected output to contain "refund", got: "Hello there, nice weather today."
baseline gate: FAIL (1 regressed, 0 new failing)
```

The four categories, precisely:

| Category | Meaning | Fails the baseline gate? |
|---|---|---|
| **REGRESSED** | A case that **passed** in the baseline now **fails**. | **Yes** |
| **NEW FAIL** | A case **not in the baseline** that **fails** now. | **Yes** |
| **FIXED** | A case that **failed** in the baseline now **passes**. | No (good news) |
| **REMOVED** | A case in the baseline that is **gone** from the current run. | No |

An accepted failure, a case that failed in the baseline and still fails, is
tolerated: it is neither regressed nor new-failing, so it doesn't fail the gate.
That is the point. Your PR is judged on whether *it* made things worse, not on
pre-existing debt.

## Rolling baselines

Combine both flags to compare first, then re-record the accepted state in one
run:

```sh
evalcore run evals.yaml --baseline main --save-baseline main
```

The order is deliberate: it compares the current run against the stored `main`,
prints the diff and computes the gate, and *then* overwrites `main` with the
current results. Use this once you've accepted the current state as the new
normal.

## Suite gates: absolute floors

Baselines and per-case scorers ask "did any single case fail (or regress)?"
Suite gates add a floor over the *whole* run. Declare them under `run.gates`:

```yaml
run:
  gates:
    - type: pass_rate
      min: 0.95                # fraction of cases passing EVERY scorer, in [0,1]
    - type: mean_score
      scorer: contains         # optional: restrict to one scorer type
      min: 0.4                 # any finite number
```

Outcomes print after the summary and ride along in the JSON report:

```text
1 passed, 1 failed, 2 total
GATE FAIL pass_rate >= 0.95 (actual 0.50)
GATE PASS mean_score(contains) >= 0.4 (actual 0.50)
```

### `pass_rate`

The fraction of cases that pass **every** scorer, in `[0, 1]`. One detail to
watch: target-error cases count in the denominator. A case whose target errored
(a 500, a timeout, a replay miss) is a failure, so an error storm drags
`pass_rate` down. Failures are data, and this gate sees them.

### `mean_score`

The mean of scorer `value`s. With `scorer:` set, only that scorer *type*'s scores
are averaged; omitted, all scorer values are. Two subtleties:

- `scorer:` must name a configured scorer. A typo'd scorer name is rejected at
  config validation, so the gate fails fast rather than silently averaging
  nothing.
- Error cases produce no scores. A case whose target errored contributes
  *nothing* to the mean, so a run with a high mean but many errors can still
  look healthy to `mean_score` alone. **Pair a `mean_score` gate with a
  `pass_rate` gate** to catch error storms that `mean_score` would miss.

### The 1e-9 tolerance

Floors compare with a `1e-9` absolute tolerance, so a run that *exactly* meets
its floor passes. Floating-point rounding won't fail a run that mathematically
hits its target (e.g. a `pass_rate` of exactly `0.95`).

## The additive exit-code contract

The exit code folds three verdicts together. In plain terms, a run exits **`0`
only when all of these hold**:

1. The per-case / baseline contract passes. Without `--baseline`, every case
   passed; with `--baseline`, no case regressed and no new case failed
   (accepted failures tolerated).
2. Every suite gate is at or above its floor.

If either fails, the run exits `1`. The truth table:

| Per-case / baseline result | Suite gates | Exit |
|---|---|---|
| All cases passed (no baseline) | pass (or none) | **0** |
| All cases passed (no baseline) | any gate fails | **1** |
| A case failed (no baseline) | pass (or none) | **1** |
| No regressions (with `--baseline`) | pass (or none) | **0** |
| No regressions (with `--baseline`) | any gate fails | **1** |
| A regression or new failing (with `--baseline`) | pass (or none) | **1** |
| Accepted baseline failure only (with `--baseline`) | pass (or none) | **0** |
| Accepted baseline failure only (with `--baseline`) | `pass_rate` gate below floor | **1** |

The last two rows are the interaction to remember: with `--baseline`, an accepted
failure stays tolerated per-case, yet it still counts toward `pass_rate` and can
sink a gate it drops below. Baselines forgive specific known failures; gates hold
an absolute line regardless.

Gates leave JUnit output unchanged. The exit code carries the gate result for CI.

## Team workflow: when to re-baseline

- Re-baseline when you deliberately accept a new state: you fixed some cases, or
  you've decided a newly-failing case is acceptable for now. Run
  `--save-baseline main` (or the rolling form) and commit the updated
  `.evalcore/cache.db`.
- **Don't re-baseline to make a red build green.** If a PR regressed a case, the
  fix is the code or the prompt, not silently accepting the regression into the
  baseline. Re-baselining is a reviewed decision, visible in the committed store.
- Gates are your floor; baselines are your ratchet. Keep a `pass_rate` floor that
  protects the minimum you'll ship, and let the baseline track the moving
  accepted state above it.

For where these fit in the CI pipeline, see
[Running in CI](/guides/running-in-ci/); for field details, the
[configuration reference](/reference/configuration/).

## See also

- [Running in CI](/guides/running-in-ci/): the full pipeline that
  commits cassettes, replays offline, and gates on this exit code.
- [Comparing models](/guides/comparing-models/): why matrix runs
  reject `--baseline` and how gates still apply per arm.
- [HTML reports](/guides/html-reports/): the gates panel and
  baseline diff rendered for a reviewer to open.
- [Configuration reference](/reference/configuration/): the full
  `run.gates` and baseline field schema.
