---
title: LLM-as-judge
description: "Grade open-ended answers against a rubric with any OpenAI-compatible endpoint: designing binary-decidable rubrics, choosing a threshold, why cached verdicts are CI-safe, what actually re-records a judge, and how judge tokens and cost count toward totals and budgets."
---

Some answers can't be checked with `contains` or a regex. Is this grounded in the
context? Is the tone appropriate? Does it actually answer the question? The
`judge` scorer grades the output against a natural-language rubric using any
OpenAI-compatible endpoint. Its verdicts go through the record/replay cache, so a
replayed judge is deterministic and CI-safe.

```yaml
scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    rubric: "Does the answer state a concrete refund window in days?"
    api_key_env: OPENAI_API_KEY
    threshold: 0.6                   # pass iff score >= threshold (default 0.5)
    cost:                            # optional; prices the judge's own tokens
      input_per_1m: 0.15
      output_per_1m: 0.60
```

The judge is prompted to return `{"score": 0.0..1.0, "reason": "..."}`; the case
**passes** iff `score >= threshold`. Code-fenced verdicts are tolerated, and an
un-parseable or out-of-range verdict becomes a *failing score with a reason*,
never a crash.

## Rubric design: specific and binary-decidable

A judge is only as reliable as its rubric. Write rubrics a careful human could
grade the same way twice:

- Make it a yes/no question about one property. "Does the answer state a
  concrete refund window in days?" is binary-decidable. "Is the answer good?" is
  not, because it smears several separate qualities into one fuzzy number.
- Name the evidence. "Is the answer grounded in the provided context?" tells the
  judge what to check against. Prefer rubrics that reference the input or the
  case's `expected` field (both are included in the judge prompt).
- One rubric, one concern. If you care about two properties, use two `judge`
  scorers with two rubrics. You get two independently gradeable scores and can
  gate them separately.
- Avoid asking the judge to be creative. It should *assess*, not rewrite.

```yaml
scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    rubric: "Is every factual claim in the answer supported by the <input> context? Answer high only if nothing is fabricated."
    threshold: 0.7
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    rubric: "Is the answer a single sentence, as instructed?"
    threshold: 0.9
```

## Choosing a threshold

`threshold` (default `0.5`) is where you draw the pass/fail line on the judge's
0.0 to 1.0 score:

- Strict gates, such as safety, grounding, or format compliance, want higher
  thresholds around `0.7` to `0.9`, so only confident passes get through.
- Soft-quality signals you're tracking but not blocking on want lower thresholds.
  Let a `mean_score` gate watch the aggregate instead of failing individual cases
  (see [Gates and baselines](/guides/gates-and-baselines/)).
- Calibrate against real cassettes rather than guessing. Record a batch, look at
  the actual scores the judge assigns to answers you consider good vs bad, and
  set the threshold between the two clusters.

## Judge caching makes judges CI-safe

An un-cached LLM judge would be nondeterministic. The same answer could score 0.8
today and 0.6 tomorrow, flaking your build. EvalCore wraps the judge target in the
same record/replay cache as the main target: the first run records the verdict,
and every replay returns it byte-for-byte. Under `--cache replay`, judge verdicts
are fixed, offline, and free, which is what makes an LLM-graded suite usable as a
CI gate.

```sh
evalcore run evals.yaml --cache auto     # records judge verdicts alongside answers
evalcore run evals.yaml --cache replay   # replays them: deterministic, keyless, $0
```

## What actually re-records a judge verdict

This is worth stating precisely, because it is subtle. The judge's cache key is
`{"identity": <judge target identity>, "input": <the judge prompt>}`, and the
judge prompt embeds the rubric, the case input, the case `expected`, and the
answer being graded. So a judge verdict re-records when any of these change:

| You change… | Re-records the verdict? | Why |
|---|---|---|
| The **rubric** | **Yes** | The rubric is part of the judge prompt, which *is* the judge call's cached input. |
| The judge `model` or `url` | Yes | Part of the judge target's identity. |
| The graded answer (because the main target changed) | Yes | The answer is embedded in the judge prompt. |
| A case's `input` or `expected` | Yes | Both are embedded in the judge prompt. |
| `threshold` | No | It's applied *after* the verdict; the cached score is reused, only the pass/fail line moves. |
| The judge `cost` block | No | Pricing is metadata, not part of the request; it re-costs the cached tokens without re-grading. |

The rubric is **not** a field of the judge target's `cache_identity()`, which is
just `url`/`model`/`system`/`params`. Instead it enters the key through the judge
*prompt*, which becomes the cached call's `input`. The net effect is the same,
editing a rubric re-records, but the mechanism is "the rubric is in the prompt,"
not "the rubric is in the target identity." If you ever inspect a cassette,
that's why you won't find the rubric listed as an identity field.

## Keeping judges honest

- Spot-check the cassettes. A judge verdict is a recording of a real model call.
  When you record a batch, read a sample of the verdicts and their `reason`
  strings and ask whether the judge agrees with you. A judge that rubber-stamps
  everything at 0.9 is a miscalibrated rubric, not a passing suite.
- Re-record on a rubric change, deliberately. Because editing the rubric changes
  the cache key, the next `--cache auto` run re-grades every case and the new
  verdicts land in your cassette diff. Review them like any behavior change.
- Watch drift on a schedule, not on PRs. The judge model can drift just like any
  model. Catch it with the nightly `--cache live` job, not by un-determinizing PR
  runs. See [Record / replay](/guides/record-replay/).

## Judge tokens and cost count toward totals and budgets

Judge calls consume real tokens, and EvalCore attributes them. The summary's
token total includes the tokens a judge reported (from the endpoint's `usage`
block) alongside the target's, so a grading-heavy suite's true token footprint
shows up in the `$`/token line rather than being silently dropped.

To turn judge tokens into dollars, give the `judge` scorer its own `cost` block
(same shape as a target's: USD per 1M input and output tokens, priced for the
judge model, which is often different from the target's):

```yaml
scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    rubric: "Is the answer grounded in the provided context?"
    cost:
      input_per_1m: 0.15
      output_per_1m: 0.60
```

With `cost` set, each judge call's cost folds into that case's cost, the run's
`total_cost_usd`, and the `run.budget_usd` accumulator, so a judge-heavy suite
can exhaust the budget and stop dispatching new cases, exactly like target
spend. Without `cost`, judge tokens still count toward token totals; they just
carry no dollar figure. An endpoint that returns no `usage` block contributes
neither tokens nor cost, and never errors; failures are data.

Only the judge scorer is costed today; the `similarity` scorer's embedding
calls are not yet attributed. See [Cost and budgets](/guides/cost-and-budgets/).

For the exact judge protocol and verdict parsing, see the
[configuration reference](/reference/configuration/); for the cache
mechanics, [Record / replay](/guides/record-replay/).

## See also

- [RAG evaluation](/guides/rag-evaluation/): a groundedness rubric is
  the recommended judge use, graded against retrieved context.
- [Semantic similarity](/guides/semantic-similarity/): a cheaper,
  deterministic scorer when a full rubric is more than you need.
- [Cost and budgets](/guides/cost-and-budgets/): how judge calls are
  counted in the run's cost when the judge declares rates.
- [Record / replay](/guides/record-replay/): how judge verdicts are
  cached so graded suites replay for $0.
