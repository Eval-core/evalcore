# Agent trajectory evaluation

Scoring an agent from a recorded trace, with no invocation and no network. EvalCore ingests traces
(its native trajectory format, or an OTel/OpenInference JSON export) from `traces/` and asserts on
what the agent actually did: the final answer it gave and the path it took to get there.

## Run it

```sh
evalcore run examples/agent-trace/evals.yaml
```

## What the suite checks

- The answer: a `contains` scorer grades the extracted final answer (`30 days`). Trace targets
  pull the answer from the native `final_output` or the OTel root span, so text scorers grade the
  answer, not the raw trajectory JSON.
- The path: a `trajectory` scorer asserts on the steps, in parallel with the answer check: the
  agent must call `search_kb` with a refund query, must not call `issue_refund` before
  `verify_identity`, and must finish in at most five steps.
- Cost: the target prices the token usage found in the trace spans.

See the [agents and traces guide](https://evalcore.cc/guides/agents-and-traces/) and the
[trajectory format reference](https://evalcore.cc/reference/trajectory-format/).
