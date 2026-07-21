# Live model calls: OpenAI-compatible target

The one example that talks to a real model. It evaluates `gpt-4.1-mini` through the
`openai-compatible` target with a deterministic check and an LLM judge, and shows the pieces you
only meet with live calls: token usage, declared cost, and record/replay.

## Run it

Needs `OPENAI_API_KEY` in the environment (see the repo `.env`).

```sh
evalcore run examples/openai/evals.yaml --cache live   # records responses into .evalcore/cache.db
evalcore run examples/openai/evals.yaml --cache replay # re-runs offline, for $0
```

Record once, commit the cassette, and every later run replays deterministically with no network
and no spend. See [record / replay](https://evalcore.cc/guides/record-replay/).

## What the suite checks

- Deterministic check: a `contains` scorer asserts the refund window (`30`) appears in the
  answer.
- Judge: an LLM judge grades whether the answer states a concrete refund window in days,
  with a `0.6` threshold. See [LLM-as-judge](https://evalcore.cc/guides/llm-as-judge/).
- Cost: the target declares input/output token prices, so the report attributes per-case and
  total spend. Verify the numbers against current pricing; the rates are yours to declare. See
  [cost and budgets](https://evalcore.cc/guides/cost-and-budgets/).
