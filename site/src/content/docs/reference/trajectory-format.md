---
title: Trajectory format
description: EvalCore's canonical agent-trajectory format, the OTel/OpenInference mapping, and the trajectory rule semantics.
---

This page specifies the canonical trajectory format the `trace` target
normalizes agent traces into, the accepted input formats and how they map onto
it, and the assertion rules the `trajectory` scorer evaluates. It is the
trajectory spec (**v0**) adapted for reference; field names may still change
before v1.

## Canonical trajectory format

A trajectory is a JSON object with a `steps` array and an optional `final_output`
string. Each step is one **tool call**, in chronological order.

```json
{
  "final_output": "Refunds are honored within 30 days.",
  "steps": [
    {
      "tool": "search_kb",
      "input": {"query": "refund policy time limits"},
      "output": "Refunds are honored within 30 days."
    }
  ]
}
```

Top-level fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `steps` | array | yes | The tool calls, in chronological order. |
| `final_output` | string | no | The agent's final answer, graded by text/judge scorers. When present it MUST be a string; a non-string value is a loading error rather than a silently dropped field. |

Each step:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `tool` | string | yes | Tool/function name as the agent framework knows it. |
| `input` | any JSON | no (default `null`) | The call's arguments. |
| `output` | any JSON | no | The call's result. |

LLM completions, retrievals modeled as plain spans, and other non-tool activity
are **not** steps, because a trajectory describes what the agent *did*. Producers may
include extra fields on a step; consumers ignore fields they don't understand.

## Accepted input formats

The `trace` target auto-detects and normalizes two formats:

1. **Native**: the format above, verbatim (`{"steps": [...]}`).
2. **OTel JSON export** (`{"resourceSpans": [...]}`), mapped per span as below.

### OTel / OpenInference mapping

Per-span mapping (a span is a tool call when it carries the "tool call"
attribute):

| Concept | OTel GenAI semconv | OpenInference |
|---|---|---|
| "this span is a tool call" | `gen_ai.tool.name` present | `openinference.span.kind == "TOOL"` |
| tool name | `gen_ai.tool.name` | `tool.name`, else span name |
| input | `gen_ai.tool.call.arguments` | `input.value` |
| output | `gen_ai.tool.call.result` | `output.value` |
| token usage (any span) | `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens` | `llm.token_count.prompt` / `llm.token_count.completion` |

The **final answer** is read from a **root span**. A root candidate is a span
whose `parentSpanId` is empty/absent, or references a span not present in the
export (a partial trace rooted at a dropped parent):

| Concept | OTel GenAI semconv | OpenInference |
|---|---|---|
| final answer (root span) | `gen_ai.completion` | `output.value` |

Root-span selection and extraction rules:

- Among root candidates that carry one of these attributes, the one with the
  **latest** `startTimeUnixNano` is the final answer. For a proper single-root
  trace this is just that root; on a flat export where every span is a candidate
  (e.g. a planner LLM emitting an interim thought before the responder answers),
  the last thing said is the answer, not the first.
- Precedence when both attributes are present on the chosen span:
  `output.value` (OpenInference) first, then `gen_ai.completion` (OTel GenAI).
- The final answer is kept as a raw string. A stringified-JSON answer is not
  unwrapped, since the final answer is text rather than a payload to address
  fields on. If no root candidate carries one of these attributes, the trace has
  no final answer.
- Spans are ordered by `startTimeUnixNano`; spans without timestamps keep
  document order after timestamped ones.
- String-valued step inputs/outputs that parse as JSON **are** unwrapped, so
  matchers can address their fields.
- Token usage is summed across **all** spans (an LLM span isn't a tool call, but
  its cost belongs to the run) and feeds cost accounting.
- Trace latency is `max(endTimeUnixNano) − min(startTimeUnixNano)`.

## Final answer vs. the path

When a trace carries a final answer, the `trace` target's text output **is that
answer**, so `judge`, `contains`, `regex`, and `exact` scorers grade the agent's
actual answer, not the trajectory JSON. The `trajectory` scorer always operates
on the steps. Both can run on the same case: a judge grades whether the answer
was right while trajectory rules check whether the path was safe. When a trace
carries no final answer, the text output stays the serialized trajectory JSON,
so existing suites that grade it with `contains` keep working.

## Assertion rules

Rules appear under the `trajectory` scorer in `evals.yaml`. A `trajectory`
scorer passes iff **all** its rules hold; each failed rule contributes a
human-readable reason naming the rule and the offending step.

```yaml
scorers:
  - type: trajectory
    rules:
      - must_call: search_kb
        with:
          query: { contains: "refund" }
      - must_call: issue_refund
        after: verify_identity
      - must_not_call: issue_refund
        before: verify_identity
      - max_steps: 8
```

### `must_call: T`

Holds iff at least one step calls `T` **and** satisfies every `with` constraint.

- `with:` is a map of argument field to [matcher](#field-matchers). A call
  matches only if **all** listed fields match.
- `after: U` counts only steps strictly after the **first** call of `U`. If `U`
  never runs, the rule fails, because the required precondition never happened.

### `must_not_call: T`

Holds iff no step calls `T`.

- `before: U` considers only steps before the **first** call of `U`. If `U`
  never runs, **every** call of `T` violates the rule. This is deliberately
  conservative: the guard `T` was waiting for never happened.

### `max_steps: N`

Holds iff the trajectory contains at most `N` steps (tool calls).

## Field matchers

```yaml
with:
  query: { contains: "refund" }     # substring on the field as a string
  amount: { equals: 42 }            # exact JSON equality
```

- `contains` is a substring test; non-string fields are compared against their
  compact JSON rendering.
- `equals` is strict JSON equality.
- Both may be given; all present constraints must hold.
- A missing field never matches.

## Trace cases in the dataset

A `trace` case names its trace file in the dataset row's `trace` field, resolved
relative to the dataset file:

```jsonl
{"id": "agent-flow", "trace": "traces/run1.json"}
```

See the [Configuration reference](../configuration/#trace) for the `trace`
target and the [dataset format](../configuration/#jsonl-case-format).
