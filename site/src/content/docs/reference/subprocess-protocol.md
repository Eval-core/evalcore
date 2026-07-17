---
title: Subprocess scorer protocol
description: The any-language scorer contract — JSON on stdin, a verdict on stdout, error handling, and worked Python and Node examples.
---

The `subprocess` scorer is EvalCore's any-language escape hatch. It runs a
command once per case, hands it the case as JSON on **stdin**, and reads a
verdict as JSON from **stdout**. Any language that can read stdin and write
stdout can implement a scorer — no Rust required.

This is versioned API surface (protocol **v0**). Field names and semantics are
stable; changes are breaking.

## Configuration

```yaml
scorers:
  - type: subprocess
    cmd: python3 scorers/faithfulness.py
```

`cmd` is run via `sh -c`, once per case.

## Input (stdin)

The scorer receives a single JSON object on stdin:

```json
{
  "input": "How long do refunds take?",
  "output": "Refunds are honored within 30 days.",
  "expected": "30 days"
}
```

| Field | Type | Description |
|---|---|---|
| `input` | string | The case's input, as sent to the target. |
| `output` | string | The target's output text for this case. |
| `expected` | any JSON, or `null` | The case's `expected` field, verbatim. `null` when the case has none. |

The command **must read stdin** (even if only to discard it). A command that
exits before EvalCore finishes writing the payload can race and fail the write.
In shell, drain it with `cat >/dev/null` before doing anything else.

## Output (stdout)

The scorer must print a single JSON object on stdout:

```json
{"score": 0.9, "passed": true, "reason": "grounded in the provided context"}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `score` | number | yes | Must be within `0.0..=1.0`. A score outside that range is an error. |
| `passed` | bool | no | Whether the case passes. When omitted, defaults to `score >= 0.5`. |
| `reason` | string | no | Human-readable explanation, shown in reports for failing cases. Omitted when absent. |

Stdout is trimmed before parsing, so trailing newlines are fine. Only these
three fields are read.

## Error handling

Failures are data — a misbehaving scorer surfaces as a failing/errored score,
never a panic and never a crashed run.

| Condition | Result |
|---|---|
| Non-zero exit | The case's score errors; the message includes the exit status and the command's trimmed **stderr**. |
| Invalid / non-JSON stdout | Errors: `scorer "<cmd>" printed invalid verdict JSON: "<stdout>"`. |
| `score` outside `0.0..=1.0` | Errors: `scorer "<cmd>" returned score <n> outside 0.0..=1.0`. |
| Spawn failure | Errors: `failed to spawn subprocess scorer: <cmd>`. |

The engine converts a scorer error into a failing `Score` with the reason
prefixed `scorer error: …`, so one bad scorer never aborts the suite.

## Worked example — Python

```python
#!/usr/bin/env python3
import json
import sys

case = json.load(sys.stdin)          # read stdin fully
output = case["output"]
expected = case.get("expected") or ""

# toy metric: fraction of expected words present in the output
wanted = str(expected).lower().split()
hits = sum(1 for w in wanted if w in output.lower())
score = hits / len(wanted) if wanted else 1.0

print(json.dumps({                    # write exactly one JSON object
    "score": score,
    "passed": score >= 0.8,
    "reason": f"{hits}/{len(wanted)} expected terms present",
}))
```

```yaml
scorers:
  - type: subprocess
    cmd: python3 scorers/coverage.py
```

## Worked example — Node

```js
#!/usr/bin/env node
let raw = "";
process.stdin.on("data", (chunk) => (raw += chunk));   // read stdin fully
process.stdin.on("end", () => {
  const { output, expected } = JSON.parse(raw);
  const want = String(expected ?? "").toLowerCase();
  const passed = output.toLowerCase().includes(want);
  process.stdout.write(                                 // one JSON object
    JSON.stringify({
      score: passed ? 1.0 : 0.0,
      passed,
      reason: passed ? "expected text found" : `missing ${JSON.stringify(want)}`,
    })
  );
});
```

```yaml
scorers:
  - type: subprocess
    cmd: node scorers/contains.js
```

Both examples read stdin to completion before writing, and emit exactly one JSON
object with a required `score` — the shape the protocol requires.
