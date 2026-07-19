---
title: Comparing models
description: Run one suite against several targets in a single invocation and get a side-by-side comparison — model A vs B, prompt v1 vs v2 — with per-case winners, per-target cost, and one exit code for CI.
---

:::note[Requires an unreleased build]
`run.matrix` lands in **0.7.0** (unreleased; the current release is 0.6.0).
Until 0.7.0 ships, install from source —
`cargo install --git https://github.com/eval-core/evalcore` — or build the
workspace locally.
:::

Every eval so far has pointed one suite at one target. But the questions that
actually drive a model decision are comparative: *is the cheaper model good
enough?* *did the new system prompt regress anything?* *which of these two
retrieval configs answers more cases correctly?* Answering those with the
single-target runner means running the suite twice and diffing two reports by
eye.

A **matrix run** does it in one invocation: the whole suite runs once per target
and EvalCore prints a side-by-side comparison. Because a target is just "a thing
that answers a case," the two things you compare can be **two models** or **two
prompts** or **two deployed endpoints** — anything you can configure as a target.

## Two things to compare are two targets

The unit of comparison is a target. To compare **model A against model B**,
define both, each with its own model name and cost rates:

```yaml
targets:
  gpt:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    cost: { input_per_1m: 0.40, output_per_1m: 1.60 }
  claude:
    type: openai-compatible
    url: https://api.anthropic.com/v1
    model: claude-sonnet-5
    api_key_env: ANTHROPIC_API_KEY
    cost: { input_per_1m: 3.00, output_per_1m: 15.00 }
```

To compare **prompt v1 against prompt v2**, the two targets are the same model
with different `system` prompts — the matrix machinery doesn't care what varies:

```yaml
targets:
  terse:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    system: "Answer in one sentence."
  detailed:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    system: "Answer thoroughly, citing the policy."
```

## Turn on the matrix

List the targets to compare under `run.matrix`, in the order you want them
reported:

```yaml
run:
  matrix: [gpt, claude]   # at least two distinct names, each defined in `targets`
```

Or select them on the command line — a comma list that **overrides**
`run.matrix`:

```sh
evalcore run evals.yaml --matrix gpt,claude
```

Either way, the suite runs once per named target, **sequentially, in the list
order you wrote** (not the alphabetical map order). Sequential, ordered arms keep
the run deterministic and predictable against a provider's rate limits. Within
each arm, everything behaves exactly as a single-target run does today —
concurrency, gates, classification, and trials all apply per arm.

A matrix needs at least two distinct, defined names. The validation is identical
whether it comes from `run.matrix` or `--matrix`:

```
matrix must list at least two targets, got 1
matrix lists target "gpt" more than once
matrix target "mystery" is not defined; available: claude, gpt
```

## Reading the comparison

Here is a real matrix run of a two-case suite over two shell targets — `echo`
(passes the input through) and `upper` (uppercases it) — scored by
`contains: "REFUND"`:

```
== target: echo
PASS refund-1 (7ms)
FAIL refund-2
     contains: expected output to contain "REFUND", got: "refund please"

1 passed, 1 failed, 2 total

== target: upper
PASS refund-1 (5ms)
PASS refund-2 (6ms)

2 passed, 0 failed, 2 total

== comparison
case        echo    upper
refund-1    PASS    PASS     tie
refund-2    FAIL    PASS     upper
wins: echo 0 · upper 1 · ties 1
```

Each arm gets its **full terminal block** — the same PASS/FAIL lines, summary,
and gate lines a single-target run prints — under a `== target: <name>` header.
Then the `== comparison` section lays the arms side by side: one row per case (in
dataset order), a PASS/FAIL cell per arm, and a **winner** column. The `wins:`
footer tallies each arm's outright wins and the tie count.

## How the winner is decided

The winner of a case is the arm with the **strictly highest mean case score** —
the mean of that case's scorer values for that arm. It is *not* PASS-vs-FAIL: two
arms can both PASS and still have a winner if one scored higher (say a `judge` or
`similarity` scorer gave 0.9 vs 0.7).

- A case is a **tie** when the top arms are within a `1e-9` tolerance of each
  other, so floating-point rounding never crowns a spurious winner. In the run
  above, `refund-1` is `PASS`/`PASS` with identical `contains` scores of 1.0, so
  it ties.
- A case where **no arm produced a score** (every arm errored, or the budget
  skipped it) is also a tie — nothing to compare.
- The rule generalizes to **any number of arms**: the winner is the unique
  maximum, or a tie if two or more share it.

The `wins:` line counts only outright wins, so wins across all arms plus ties
equals the case count.

## Per-target cost, per-arm budget

Matrix mode closes the "one set of cost rates per run" gap: **each arm prices
with its own target's `cost:` block**. Point a matrix at a cheap model and an
expensive one and each arm's cost reflects its own rates — which is exactly the
comparison you want when the question is "is the cheaper model good enough?"

`run.budget_usd` applies **per arm**, not across the whole matrix: every arm gets
the full budget. A `budget_usd: 5.0` with three arms allows up to \$5 of spend
*per target*, so the same suite costs the same whether you run one arm or three.
(This mirrors how a budget bounds a single-target run — it is a ceiling on one
suite's spend, and a matrix runs the suite once per arm.)

Trials compose the same way: each arm honors `run.trials` independently, so
`trials: 3` runs three trials of every case *for every arm*.

## Caching and replay across arms

A matrix shares **one cassette store** across all arms, and replay works per arm
because **each arm has its own cache identity**. An LLM target's cache key hashes
its model, URL, system prompt, and params — so `gpt` and `claude` (or `terse`
and `detailed`) never collide, and each arm records and replays its own
responses.

Judge and embedding (`similarity`) scorer calls also share the store, and that
is correct: a judge grades each arm's **distinct output**, so its prompt differs
per arm anyway, and each arm gets its own recorded verdict. The practical
consequence is the usual one — record once with `--cache auto` (or `live`),
commit the cassette, and `--cache replay` reruns the whole matrix offline,
keyless, and deterministically. See [Record / replay](/evalcore/guides/record-replay/).

## The exit code

A matrix run exits `0` **iff every arm satisfies today's whole contract** — all
of that arm's cases pass *and* every configured gate holds — and `1` otherwise.
One arm regressing or tripping a gate fails the run, so a matrix drops into CI
exactly like a single-target suite: gate the job on the exit code.

In the two-shell-target run above, `echo` has a failing case, so the run exits
`1` even though `upper` is all green. When both arms pass every case and gate:

```
== comparison
case        echo    upper
refund-1    PASS    PASS     tie
refund-2    PASS    PASS     tie
wins: echo 0 · upper 0 · ties 2
```

the run exits `0`.

The comparison is a *report*, not a gate: the winner column tells you which arm
did better, but the exit code still asks "did every arm meet its bar?" A matrix
never fails just because one arm lost cases to another.

## What a matrix can't combine with

Matrix mode is mutually exclusive with target selection and with baselines, and
each combination is a **hard error** rather than a silent choice:

```sh
$ evalcore run evals.yaml --matrix gpt,claude --target gpt
Error: cannot combine --target with a matrix: a matrix already runs the suite
against several targets. Drop --target, or drop the matrix.

$ evalcore run evals.yaml --matrix gpt,claude --baseline main
Error: baselines are per-run; run targets separately with --target to baseline them
```

`--save-baseline` is rejected the same way as `--baseline`. **Baselines are
per-run in v1**: a stored baseline is a single suite's per-case snapshot, and a
matrix has no single set of results to save or diff. To baseline a target, run it
on its own with `--target <name>` and the usual `--baseline` / `--save-baseline`
flags (see [Gates and baselines](/evalcore/guides/gates-and-baselines/)).

## See also

- [Cost and budgets](/evalcore/guides/cost-and-budgets/) — the `cost:` rates each
  arm prices with, and how `budget_usd` bounds a suite's spend.
- [Gates and baselines](/evalcore/guides/gates-and-baselines/) — per-arm gates
  and why baselines stay single-target in v1.
- [Record / replay](/evalcore/guides/record-replay/) — the cache the arms share,
  and per-arm replay.
- [Configuration reference](/evalcore/reference/configuration/#run-block) — the
  `run.matrix` schema and validation rules.
</content>
</invoke>
