---
title: Cost and budgets
description: Declare token rates, read per-case and total cost, cap spend with budget_usd, understand why replayed runs report $0 actual, and price agent-trace runs from span tokens.
---

EvalCore reports token usage and cost per case and across the run, and can stop a
run before it overspends. There is no shipped pricing table. You declare
your provider's rates in config, where code review can see them, and a stale
table can't produce confidently-wrong dollars.

## Declaring rates

Add a `cost:` block to an `openai-compatible` or `trace` target, in USD per 1
million tokens:

```yaml
targets:
  openai:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    cost:
      input_per_1m: 0.40           # your provider's price per 1M input tokens
      output_per_1m: 1.60          # per 1M output tokens
```

Without a `cost:` block, EvalCore still reports token counts when the provider
returns usage, but no `$` figure. It will not invent a price.

## The formula

Cost per case is computed from the token counts the provider reported:

```text
cost = (input_tokens × input_per_1m + output_tokens × output_per_1m) / 1_000_000
```

Totals are the sum across cases. The terminal summary appends tokens and cost
when they're available:

```text
12 passed, 0 failed, 12 total · 48210 tokens · $0.0341
```

A `shell` or `http` target reports neither (no standard usage shape), so those
runs simply have no token/cost line.

## Per-case and total cost

Cost is tracked at both granularities:

- Per case: each case's cost rides along in the JSON report (`cost_usd`) and
  appears in the `--html` report's per-case row.
- Totals: the tokens and `$` on the summary line, and in the HTML header.

Because reporters are pure functions of recorded usage, these numbers are
deterministic: the same run renders the same dollars every time.

## Budgets: `run.budget_usd`

Cap what a run may spend with `run.budget_usd`. Once accumulated cost reaches the
cap, EvalCore stops dispatching new cases, but the run completes and exits
cleanly rather than aborting mid-flight:

```yaml
run:
  budget_usd: 5.0                  # stop dispatching new cases past $5 of spend
```

Semantics to internalize:

- The run completes. Cases already in flight finish; new ones are not
  started once the budget is exhausted.
- Skipped cases are failures with a reason, following the "failures are data"
  rule. They count as failed cases, so the run exits `1`, and you can see exactly
  which cases were skipped and why in the report.
- A budget requires rates. `budget_usd` needs the target to declare `cost:`
  rates, since cost is measured from token usage and without rates there's
  nothing to accumulate against.
- Validation rejects a non-positive `budget_usd`.

## Cost in replay mode: recorded usage, $0 actual

A replayed run reports the recorded token usage and cost from the cassette, so
cost stays visible in CI even though the *actual* spend is `$0` (nothing was
called). This is deliberate: your CI report shows what the suite *would* cost at
your declared rates, while replay itself is free.

```sh
evalcore run evals.yaml --cache replay   # $ line reflects recorded usage; real spend is $0
```

The same holds for `budget_usd` under replay: the *recorded* (virtual) cost
accumulates and can trip the budget, so a budget gate behaves consistently
whether you record or replay.

## Trace-run costs from span tokens

Agent-trace runs have no live call to meter, so their cost comes from the token
usage found in the trace spans. Declare `cost:` on the `trace` target and
EvalCore prices the summed span usage:

```yaml
targets:
  support-agent:
    type: trace
    cost:
      input_per_1m: 0.40
      output_per_1m: 1.60
```

The shipped agent-trace example shows this. One case's OTel trace carries
`220` input + `48` output tokens, so the run reports:

```text
2 passed, 0 failed, 2 total · 268 tokens · $0.0002
```

See [Agents and traces](/guides/agents-and-traces/) for how usage is
extracted from spans.

:::caution[Known gap]
**LLM-judge calls are not yet included in cost totals or budgets.**
The reported tokens and `$` reflect the *target's* usage only. A judge-heavy
suite spends real money on grading that the `$` line does not show, and those
calls do not count against `run.budget_usd`. This is a documented roadmap gap.
Budget for judge calls separately for now. See
[LLM-as-judge](/guides/llm-as-judge/).
:::

For field-level details, see the
[configuration reference](/reference/configuration/).

## See also

- [Comparing models](/guides/comparing-models/): per-target cost across
  a matrix, and how `budget_usd` bounds each arm.
- [Trials and statistics](/guides/trials-and-statistics/): how cost sums
  across trials, and the token-total gap under `run.trials`.
- [LLM-as-judge](/guides/llm-as-judge/): why judge calls are not yet
  counted in the run's cost.
- [Configuration reference](/reference/configuration/#cost-rates): the
  `cost:` input and output rate fields.
