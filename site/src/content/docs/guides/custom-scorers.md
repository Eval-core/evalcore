---
title: Custom scorers
description: The subprocess scorer protocol, with a complete runnable Python faithfulness scorer, a Node one-liner, how to debug with echo, and the process-per-case performance note.
---

When the built-in scorers aren't enough, the `subprocess` scorer is the
any-language escape hatch. Your command receives the case as JSON on stdin and
prints a score as JSON on stdout. Write it in Python, Node, Go, Ruby, or
anything else that reads stdin and writes stdout. You never write Rust.

```yaml
scorers:
  - type: subprocess
    cmd: python3 scorers/faithfulness.py
```

## The protocol

EvalCore runs your command once per case, writing this JSON object to its
stdin:

```json
{"input": "the case input", "output": "the target's output", "expected": "the case's expected field, if any"}
```

Your command must print a single JSON object to stdout:

```json
{"score": 0.0, "passed": true, "reason": "one short sentence"}
```

- `score` (required) is a number. Deterministic 0/1 scorers emit exactly
  `0.0` or `1.0`; graded scorers use the full range.
- `passed` (optional) is an explicit pass/fail. If you omit it, EvalCore uses
  `score >= 0.5`.
- `reason` (optional) is shown under a failing case; make it state what was
  expected and what was seen.

A command that crashes or prints malformed JSON becomes a *failing score with a
reason*, never a crash, so one bad scorer can't abort the run. The exact protocol
is the [subprocess protocol reference](/reference/subprocess-protocol/).

## A complete Python faithfulness scorer

Here is a real, runnable scorer: a self-contained heuristic that scores how much
of the `expected` answer's keywords actually appear in the `output`. It calls
nothing external, so it runs offline and deterministically:

```python
# scorers/faithfulness.py
import sys, json

data = json.load(sys.stdin)
output = (data.get("output") or "").lower()
expected = (data.get("expected") or "").lower()

# Heuristic "faithfulness": fraction of expected keywords present in the output.
keywords = [w for w in expected.split() if len(w) > 3]
if not keywords:
    print(json.dumps({"score": 1.0, "reason": "no expected keywords to check"}))
    sys.exit(0)

hits = sum(1 for w in keywords if w in output)
score = hits / len(keywords)
print(json.dumps({
    "score": round(score, 3),
    "passed": score >= 0.5,
    "reason": f"{hits}/{len(keywords)} expected keywords grounded in the output",
}))
```

Wire it up and run it against a two-case dataset (a grounded answer and an
ungrounded one):

```yaml
scorers:
  - type: subprocess
    cmd: python3 scorers/faithfulness.py
```

```
PASS grounded (4ms)
FAIL ungrounded
     subprocess: 0/4 expected keywords grounded in the output

1 passed, 1 failed, 2 total
```

The failing case surfaces your `reason` verbatim under it.

:::note
This is a heuristic example, deliberately dependency-free so it's runnable as
written. A production faithfulness scorer would call a real embedding or
NLI model. Wiring EvalCore to an off-the-shelf metric library (Ragas-style) is on
the roadmap. For now, `subprocess` is the seam: put whatever Python you like
behind it.
:::

## A Node one-liner

Any language works. A minimal Node scorer that passes iff the output contains the
expected string:

```yaml
scorers:
  - type: subprocess
    cmd: >
      node -e 'const d=JSON.parse(require("fs").readFileSync(0));
      const ok=(d.output||"").includes(d.expected||"");
      process.stdout.write(JSON.stringify({score: ok?1:0}))'
```

Reading file descriptor `0` is reading stdin; writing to stdout returns the
score. That's the entire contract.

## Debugging tips

- Test it standalone with `echo`. Your scorer is just a program that reads
  stdin, so drive it directly without EvalCore:

  ```sh
  echo '{"input":"q","output":"Refunds within 30 days","expected":"refunds within 30 days"}' | python3 scorers/faithfulness.py
  # -> {"score": 1.0, "passed": true, "reason": "3/3 expected keywords grounded in the output"}
  ```

- Always print valid JSON, even on the error path. If your logic can throw,
  wrap it and emit `{"score": 0, "reason": "..."}` so the failure is legible
  rather than a parse error.
- Read stdin fully before exiting. A command that exits without consuming
  stdin can race the writer. Reading to EOF (as both examples do) avoids it.
- Resolve paths from the config, not the shell. `cmd` runs from wherever you
  invoke `evalcore`; keep the scorer path stable (e.g. `python3 scorers/x.py`
  with the suite committed alongside).

## Performance note: one process per case

The `subprocess` scorer spawns your command once per case. For a
Python-with-heavy-imports scorer over a large dataset, that startup cost adds up.
Keep the hot path cheap:

- Prefer a fast interpreter start or a compiled helper for big suites.
- Do expensive one-time setup lazily and keep per-invocation work minimal.
- If a check is deterministic and simple, a built-in (`contains`, `regex`,
  `exact`) avoids the process spawn entirely. Reach for `subprocess` when you
  genuinely need custom logic.

For everything the protocol guarantees, see the
[subprocess protocol reference](/reference/subprocess-protocol/).

## See also

- [Subprocess scorer protocol](/reference/subprocess-protocol/): the
  stdin/stdout contract and payload fields a scorer command must handle.
- [RAG evaluation](/guides/rag-evaluation/): the shipped Ragas and
  DeepEval shims, real subprocess scorers you can wire in as-is.
- [LLM-as-judge](/guides/llm-as-judge/): a built-in alternative when
  the check is a graded rubric, not custom logic.
- [Gates and baselines](/guides/gates-and-baselines/): how a
  subprocess scorer's `score` feeds a `mean_score` suite gate.
