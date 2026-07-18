---
title: Trials and statistics
description: Run every case N times and aggregate — because one sample of a stochastic model isn't a measurement. Trials, require policies, how verdicts and scores fold together, and the determinism story.
---

:::note[Requires an unreleased build]
`run.trials` lands in **0.7.0** (unreleased; the current release is 0.6.0).
Until 0.7.0 ships, install from source —
`cargo install --git https://github.com/eval-core/evalcore` — or build the
workspace locally.
:::

An LLM is a stochastic system: send the same prompt twice and you can get two
different answers. A single run samples that distribution exactly once, so a
green suite might mean "the model reliably does the right thing" or "the model
does the right thing 60% of the time and you got lucky." One sample is not a
measurement.

`run.trials` runs every case several times and folds the per-trial verdicts
into one case verdict, so a suite can assert on how *often* a case passes rather
than on a single roll of the dice.

## Configure trials

Add `trials` to the `run` block. The integer shorthand runs each case that many
times and requires **every** trial to pass:

```yaml
run:
  trials: 3        # run each case 3 times; the case passes only if all 3 pass
```

The full form sets the fold policy explicitly:

```yaml
run:
  trials:
    count: 5           # >= 1
    require: majority  # all | majority | any (default: all)
```

`count` must be at least 1 (`trials: 0` is rejected: `run.trials count must be
at least 1`). `trials: 1` — the default when `trials` is absent — behaves
**exactly** like a run with no trials configured: identical results, identical
cache keys, byte-identical reporter output. Trials are opt-in and cost nothing
until you ask for more than one.

## The `require` policy

A **trial** passes when every scorer passes for that trial (the same rule a
non-trial case uses). The `require` policy folds the per-trial pass/fail results
into the **case** verdict:

| `require` | The case passes when… |
|---|---|
| `all` (default) | every trial passes |
| `majority` | strictly more than half the trials pass |
| `any` | at least one trial passes |

`majority` is *strictly* more than half: 2 of 3 passes, 1 of 3 fails, and with
an even count 2 of 4 fails (2 is not more than half of 4). Pick the policy that
matches the claim you want to make — `all` for "this must never regress,"
`majority` for "this should reliably work," `any` for "the model can do this at
least sometimes."

## How trials aggregate

A multi-trial case reports one aggregated `CaseResult`, computed from its trials:

- **Per-scorer score** is the **mean** of that scorer's value across the trials.
  Three trials scoring 1, 1, 0 for a scorer give a case score of 0.667. This
  mean is what suite gates and baselines see — a `mean_score` gate averages
  these per-case means, so trials feed statistical gating directly (see [Gates
  and baselines](/evalcore/guides/gates-and-baselines/)). A trial whose target
  errored contributes no score to the mean.
- **Verdict** (`passed`) follows the `require` policy above, *not* the mean — a
  case can have a 0.667 mean score yet fail under `require: all`.
- **Latency** is the **mean** of the per-trial latencies. The individual
  per-trial latencies are kept in the case's trial detail.
- **Cost** is the **sum** across trials — every trial's tokens are billed, and
  every trial's cost counts toward `run.budget_usd`. Running 3 trials of a
  costed target spends roughly 3× a single-trial run, and the budget stops
  dispatching new cases once that accumulated spend is exhausted.

The full per-trial breakdown (each trial's pass/fail, scores, latency, and any
target error) is preserved in the JSON and HTML reports; the terminal report
summarizes it with a tag.

## Reading the terminal report

When a case runs more than one trial, its PASS/FAIL line gains a ` [k/N trials]`
tag — `k` passing trials out of `N`. Single-trial cases are untagged, so their
output stays byte-identical to a non-trial run.

Here is a real run of a two-case suite with `trials: 3`, where the first case
passes every trial and the second fails every trial:

```
PASS greeting (6ms) [3/3 trials]
FAIL mismatch [0/3 trials]
     contains: trial 0: expected output to contain "hello", got: "goodbye"

1 passed, 1 failed, 2 total
```

The reason line names the scorer and the **first failing trial** (in trial
order), so it stays actionable; the reason for every trial lives in the JSON and
HTML detail. The same suite with no `trials` configured drops the tag entirely:

```
PASS greeting (7ms)
FAIL mismatch
     contains: expected output to contain "hello", got: "goodbye"

1 passed, 1 failed, 2 total
```

The example above uses a deterministic shell target, so every trial agrees and
the tag reads `[3/3]` or `[0/3]`. Trials earn their keep against a *stochastic*
target — a live model, or a judge grading fuzzy output — where the trials
genuinely diverge and `[2/3 trials]` tells you the case is flaky rather than
solid.

## Determinism and the cache

Trials are built on EvalCore's record/replay cache without breaking the
determinism guarantee that makes cassettes replay in CI:

- **Trial 0 keeps the pre-trials cache key**, byte-for-byte. A cassette recorded
  before trials existed — or by a `trials: 1` run — replays as trial 0 with no
  re-recording.
- **Trials 1..N re-key** by adding the trial index to the cache key, so each
  trial is its own cache entry. This is what lets replayed trials differ instead
  of every trial returning the same recorded answer — without it, trials over a
  cached target would measure nothing.
- **Judge and similarity calls re-key per trial the same way.** An LLM-as-judge
  or embedding call made while scoring trial *i* is salted with *i* too, so a
  cached judge verdict varies across trials exactly as the target output does.

The practical consequence: record once with `--cache auto` (or `--cache live`),
commit the cassette, and `--cache replay` reruns all N trials of every case
offline, keyless, and deterministically — the same CI story as a single-trial
suite, just with N recorded samples per case.

## Known gap: token totals under trials

With `run.trials` greater than 1, the **cost** totals are correct — they sum
every trial. The **token** totals in the terminal and HTML summaries currently
reflect one trial per case rather than the full N. If you gate on cost or
budget, that path is accurate; if you read the token count as an exact
multi-trial total, treat it as a per-trial figure for now. This is tracked for a
follow-up alongside judge-call cost attribution.

## See also

- [Gates and baselines](/evalcore/guides/gates-and-baselines/) — the per-scorer
  mean that trials produce is what a `mean_score` gate averages.
- [Record / replay](/evalcore/guides/record-replay/) — the cache the trial
  re-keying is built on.
- [Configuration reference](/evalcore/reference/configuration/#run-block) — the
  `run.trials` schema and validation rules.
</content>
</invoke>
