# EvalCore Trajectory Spec — v0

This document specifies (1) the **canonical trajectory format** EvalCore
normalizes agent traces into, and (2) the **assertion rules** that evaluate a
trajectory. It is versioned independently of the EvalCore binary; tools other
than EvalCore are welcome to produce or consume it.

> Status: v0 — semantics below are stable in intent, field names may still
> change before v1. Breaking changes will be called out in release notes.

## 1. Canonical trajectory format

A trajectory is a JSON object with a single `steps` array. Each step is one
**tool call**, in chronological order:

```json
{
  "steps": [
    {
      "tool": "search_kb",
      "input": {"query": "refund policy time limits"},
      "output": "Refunds are honored within 30 days."
    }
  ]
}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `tool` | string | yes | Tool/function name as the agent framework knows it |
| `input` | any JSON | no (default `null`) | The call's arguments |
| `output` | any JSON | no | The call's result |

Notes:

- LLM completions, retrievals modeled as plain spans, and other non-tool
  activity are **not** steps. A trajectory describes what the agent *did*.
- Producers MAY include extra fields on a step; consumers MUST ignore fields
  they don't understand.

## 2. Accepted input formats

EvalCore's `trace` target auto-detects and normalizes:

1. **Native** — the format above, verbatim (`{"steps": [...]}`).
2. **OTel JSON export** (`{"resourceSpans": [...]}`), mapping per span:

| Concept | OTel GenAI semconv | OpenInference |
|---|---|---|
| "this span is a tool call" | `gen_ai.tool.name` present | `openinference.span.kind == "TOOL"` |
| tool name | `gen_ai.tool.name` | `tool.name`, else span name |
| input | `gen_ai.tool.call.arguments` | `input.value` |
| output | `gen_ai.tool.call.result` | `output.value` |
| token usage (any span) | `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens` | `llm.token_count.prompt` / `llm.token_count.completion` |

- Spans are ordered by `startTimeUnixNano`; spans without timestamps keep
  document order after timestamped ones.
- String-valued inputs/outputs that parse as JSON are unwrapped, so matchers
  can address fields.
- Token usage is summed across **all** spans (an LLM span isn't a tool call,
  but its cost belongs to the run) and feeds EvalCore's cost accounting.
- Trace latency is `max(endTimeUnixNano) − min(startTimeUnixNano)`.

## 3. Assertion rules

Rules appear under the `trajectory` scorer in `evals.yaml`:

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

Holds iff at least one step calls `T` **and** satisfies every `with`
constraint.

- `with:` — map of argument field → matcher (see §4). A call matches only if
  **all** listed fields match.
- `after: U` — only steps strictly after the **first** call of `U` count.
  If `U` never runs, the rule **fails** (the required precondition never
  happened).

### `must_not_call: T`

Holds iff no step calls `T`.

- `before: U` — only steps before the **first** call of `U` are considered.
  If `U` never runs, **every** call of `T` violates the rule (conservative:
  the guard `T` was waiting for never happened).

### `max_steps: N`

Holds iff the trajectory contains at most `N` steps (tool calls).

### Composition

A `trajectory` scorer passes iff **all** its rules hold. Each failed rule
contributes a human-readable reason naming the rule and the offending step.

## 4. Field matchers

```yaml
with:
  query: { contains: "refund" }     # substring on the field as a string
  amount: { equals: 42 }            # exact JSON equality
```

- `contains` — substring test; non-string fields are compared against their
  compact JSON rendering.
- `equals` — strict JSON equality.
- Both may be given; all present constraints must hold.
- A missing field never matches.

## 5. Design intent

The unit of agent evaluation is the **run the app already emitted**, not a
response EvalCore invoked. Apps keep emitting the telemetry they emit today;
EvalCore consumes the export. This keeps agent evaluation language-agnostic
and framework-agnostic by construction (see PRD §3, "Traces as the unit of
agent evaluation").
