# Live model calls: Anthropic target

Evaluates Claude through the `anthropic` target — the Messages wire format, not an SDK — with a
deterministic check and a Claude judge. Alongside [`examples/openai/`](../openai/), it's the
other live-call example: same suite shape, different provider, so the diff between the two YAML
files is exactly what changing providers costs you.

## Run it

Needs `ANTHROPIC_API_KEY` in the environment (see the repo `.env`).

```sh
evalcore run examples/anthropic/evals.yaml --cache live   # records responses into .evalcore/cache.db
evalcore run examples/anthropic/evals.yaml --cache replay # re-runs offline, for $0
```

Record once, commit the cassette, and every later run replays deterministically with no network
and no spend. See [record / replay](https://evalcore.cc/guides/record-replay/).

## What's different from the OpenAI target

- `max_tokens` is a first-class field, not a `params` passthrough: the Messages API rejects a
  request without it. EvalCore defaults it to 4096. Because a lower cap truncates the answer,
  it's part of the cache key.
- The API key goes out as `x-api-key`, and every request carries `anthropic-version`.
- `system` is a top-level body field — the Messages API has no `system` message role.
- Responses are arrays of typed content blocks, so a `thinking` block never breaks parsing.
- A policy decline arrives as HTTP 200 with `stop_reason: "refusal"`. That becomes a failed case
  with a readable reason, not a parse error.

The `type:` discriminator is part of the cache identity, so `anthropic` and `openai-compatible`
keys can never collide — switching a suite between them re-records rather than replaying the
wrong provider's answer.

## Bedrock and gateways

Any endpoint that speaks the Messages format and accepts an `x-api-key` credential works by
setting `url:` — self-hosted gateways, proxies, and the Messages-shaped AWS endpoints (Claude
Platform on AWS, Bedrock Mantle). For an endpoint that wants a different credential header, use
the `http` target, which lets you set `auth_header` and `auth_prefix` yourself. Legacy
`bedrock-runtime` InvokeModel with SigV4 signing is not supported.

## What the suite checks

- Deterministic check: a `contains` scorer asserts the refund window (`30`) appears in the answer.
- Judge: `provider: anthropic` points the judge at the Messages format; it grades whether the
  answer states a concrete refund window in days, with a `0.6` threshold. See
  [LLM-as-judge](https://evalcore.cc/guides/llm-as-judge/).
- Cost: the target declares input/output token prices, so the report attributes per-case and
  total spend. Verify the numbers against current pricing; the rates are yours to declare. See
  [cost and budgets](https://evalcore.cc/guides/cost-and-budgets/).
