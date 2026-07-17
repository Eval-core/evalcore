---
title: Agents and traces
description: Evaluate what the agent DID — export OTel/OpenInference traces, grade the trajectory (tool calls, ordering, step budgets) and the final answer in one suite, and read token, cost, and latency straight from the spans.
---

An agent isn't judged by its final answer alone — the *path* matters too. Did it
verify identity before issuing a refund? Did it call the knowledge base? Did it
stay under a step budget? EvalCore evaluates the run your agent **already
emitted** as telemetry: it ingests recorded traces and grades the **answer and
the path in one suite**. No SDK, no integration, any language or framework.

## Why traces

The unit of agent evaluation is the run the app already produced, not a response
EvalCore invoked. Your framework emits OpenTelemetry (or OpenInference) spans
today; EvalCore consumes the export. That keeps agent evaluation
framework-agnostic by construction — you assert on tool calls, ordering, and
budgets without EvalCore ever driving the agent.

```yaml
targets:
  support-agent:
    type: trace                      # ingest a recorded trace, don't invoke anything

datasets:
  - file: cases.jsonl                # each case names a trace file

scorers:
  - type: contains                   # grade the ANSWER (the trace's final output)
    value: "30 days"
  - type: trajectory                 # grade the PATH (what the agent did)
    rules:
      - must_call: search_kb
        with:
          query: { contains: "refund" }
      - must_not_call: issue_refund
        before: verify_identity      # never refund before verifying identity
      - max_steps: 5
```

```jsonl
{"id": "refund-flow-native", "trace": "traces/refund-native.json"}
{"id": "refund-flow-otel", "trace": "traces/refund-otel.json"}
```

Run the shipped example — offline, no network:

```sh
evalcore run examples/agent-trace/evals.yaml
```

```
PASS refund-flow-native (0ms)
PASS refund-flow-otel (4400ms)

2 passed, 0 failed, 2 total · 268 tokens · $0.0002
```

Note the second case: its latency (`4400ms`) and the run's tokens and cost come
from the **trace spans themselves**, not from any call EvalCore made.

## Exporting traces from your framework

EvalCore reads a trace **per case** as a single JSON file. You produce those
files however your stack already emits telemetry. The generic pattern:

1. **Instrument your agent** with an OTel or OpenInference exporter. Libraries
   like OpenLLMetry (Traceloop) and the OpenInference instrumentors emit exactly
   the `gen_ai.*` / OpenInference span attributes EvalCore reads — tool name,
   tool arguments, tool result, token usage, and the final completion.
2. **Run the agent once per eval case**, capturing that run's spans as an OTel
   JSON export (`{"resourceSpans": [...]}`).
3. **Write one JSON file per case** into your dataset directory, and name it in
   `cases.jsonl`.

A runner script (pseudocode) that builds the dataset:

```text
for case in eval_cases:
    trace = capture_otel_spans(lambda: run_agent(case.input))
    write_json(f"traces/{case.id}.json", trace)      # the OTel export
    append_jsonl("cases.jsonl", {"id": case.id, "trace": f"traces/{case.id}.json"})
```

Once the trace files exist, they are static fixtures — EvalCore never re-runs the
agent, so evaluation is offline and deterministic.

## Accepted formats

The `trace` target auto-detects and normalizes two input shapes.

### OTel / OpenInference JSON export

An `{"resourceSpans": [...]}` document. Per span, EvalCore maps:

| Concept | OTel GenAI semconv | OpenInference |
|---|---|---|
| "this span is a tool call" | `gen_ai.tool.name` present | `openinference.span.kind == "TOOL"` |
| tool name | `gen_ai.tool.name` | `tool.name`, else span name |
| input | `gen_ai.tool.call.arguments` | `input.value` |
| output | `gen_ai.tool.call.result` | `output.value` |
| token usage (any span) | `gen_ai.usage.input_tokens` / `.output_tokens` | `llm.token_count.prompt` / `.completion` |
| final answer (root span) | `gen_ai.completion` | `output.value` |

Only tool-call spans become trajectory **steps**; LLM/other spans are not steps,
but their token usage still counts toward the run's cost. Token usage is summed
across all spans, and latency is `max(endTime) − min(startTime)`.

### Native trajectory format

If you'd rather emit EvalCore's format directly — no OTel plumbing — write the
canonical JSON: a `steps` array plus an optional `final_output`:

```json
{
  "final_output": "Yes — refunds are honored within 30 days of purchase.",
  "steps": [
    {"tool": "verify_identity", "input": {"user": "u-123"}, "output": {"verified": true}},
    {"tool": "search_kb", "input": {"query": "refund policy time limits"}, "output": "Refunds are honored within 30 days."},
    {"tool": "issue_refund", "input": {"order": "o-987", "amount": 42.5}, "output": {"status": "issued"}}
  ]
}
```

Both formats are graded identically. The full spec is the
[trajectory format reference](/evalcore/reference/trajectory-format/).

## Grading the path: trajectory rules

The `trajectory` scorer always operates on the **steps**. It passes only if
**every** rule holds; each failed rule contributes a reason naming the offending
step. Three rule types, with realistic policy examples:

```yaml
scorers:
  - type: trajectory
    rules:
      # A required tool ran, with a matching argument.
      - must_call: search_kb
        with:
          query: { contains: "refund" }

      # Ordering: the refund only happens after identity is verified.
      - must_call: issue_refund
        after: verify_identity

      # Safety: never refund before verifying identity. If verify_identity
      # never runs at all, ANY issue_refund call violates this (conservative).
      - must_not_call: issue_refund
        before: verify_identity

      # Budget: no runaway tool loops.
      - max_steps: 8
```

- `must_call: T` holds iff at least one step calls `T` and satisfies every `with`
  matcher. `after: U` restricts counting to steps strictly after `U`'s first
  call — and **fails** if `U` never ran.
- `must_not_call: T` holds iff no step calls `T`. `before: U` restricts to steps
  before `U`'s first call; if `U` never ran, any `T` call violates it.
- `max_steps: N` holds iff there are at most `N` tool calls.
- Field matchers: `{ contains: "..." }` (substring on the field as a string) and
  `{ equals: <json> }` (strict JSON equality). A missing field never matches.

## Grading the answer: final-answer extraction

When a trace carries a final answer, the `trace` target's **text output is that
answer** — so `contains`, `regex`, `exact`, and `judge` grade the *answer*, not
the trajectory JSON:

- **Native format:** the top-level `final_output` string.
- **OTel export:** the final answer is read from the **root span** —
  OpenInference `output.value`, else OTel GenAI `gen_ai.completion`. Among root
  candidates, the one with the latest start time wins (the last thing said is
  the answer).

This lets you grade the answer and the path on the **same case**, combining a
judge with trajectory rules in one suite:

```yaml
scorers:
  - type: judge                      # was the answer right?
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    rubric: "Does the answer state a concrete refund window in days?"
    threshold: 0.6
  - type: trajectory                 # was the path safe?
    rules:
      - must_not_call: issue_refund
        before: verify_identity
```

"Was the answer right *and* was the path safe" — one suite, one exit code. If a
trace carries no final answer, the text output stays the serialized trajectory
JSON, so older suites that grade it with `contains` keep working.

## Token, cost, and latency from spans

Because usage is in the spans, trace runs report tokens and cost with no extra
wiring — declare `cost:` rates on the `trace` target and EvalCore prices the
summed span usage:

```yaml
targets:
  support-agent:
    type: trace
    cost:                            # prices the token usage found in the spans
      input_per_1m: 0.40
      output_per_1m: 1.60
```

Latency per case is the trace's own span (`max(endTime) − min(startTime)`),
which is why the OTel example above reports `4400ms`. Trace targets also respect
`run.budget_usd`; see [Cost and budgets](/evalcore/guides/cost-and-budgets/).

## The full example, walked

<figure class="ec-cast">
	<img
		src="/evalcore/casts/agent-trace.gif"
		alt="Terminal recording: evalcore grades a recorded agent trace — the trajectory rules and final answer both pass, with tokens and cost read straight from the spans."
		width="920"
		height="575"
		loading="lazy"
	/>
	<figcaption>Grade the answer and the path from one recorded trace — offline and deterministic.</figcaption>
</figure>

The shipped `examples/agent-trace/` suite grades two cases — one native, one OTel
— against the same scorers:

- `contains: "30 days"` grades the **answer**. Both traces carry the final answer
  "…refunds are honored within 30 days…", so both pass the answer check.
- The `trajectory` scorer grades the **path**: `search_kb` was called with a
  refund query, `issue_refund` never ran before `verify_identity`, and there
  were at most 5 steps. Both traces satisfy all three rules.

The OTel case additionally carries an LLM span with token usage
(`220` input + `48` output), which is why the run reports `268 tokens` and, at
the declared rates, `$0.0002`. Nothing here touched the network — it is all read
from the recorded traces, so the suite is a deterministic CI gate.

For the exhaustive rule semantics and normalization rules, see the
[trajectory format reference](/evalcore/reference/trajectory-format/).
