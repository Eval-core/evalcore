# RAG-metric shims (Ragas / DeepEval)

Drop-in [subprocess scorer](../crates/evalcore-scorers/CLAUDE.md) scripts that let
you score EvalCore cases with Ragas or DeepEval RAG metrics
(faithfulness, context/contextual recall) when your team is standardized on
those exact definitions.

These are shims, not the recommended default. Read the positioning below before
wiring one into a suite.

## When to use these vs. the native judge

| | PR / CI path | Nightly / scheduled tier |
|---|---|---|
| Use | EvalCore's native `judge` scorer + per-case `context` | these Ragas / DeepEval shims |
| Cost | one cached LLM call per case, reused across runs | one **live, billable** LLM call per case, every run |
| Determinism | deterministic under `--cache replay` | non-deterministic; results move run to run |

Each shim metric calls an LLM *itself*, inside Ragas/DeepEval. Those
calls do **not** go through EvalCore's record/replay cache, so they cost money
per case and are not reproducible. That makes them a bad fit for the pull-request
path, where you want fast, free, deterministic gating. Given the same per-case
`context`, the native `judge` scorer is the deterministic, cached alternative,
and it belongs on the PR path.

Reach for these shims when you specifically need Ragas' or DeepEval's metric
definitions (e.g. to match an existing dashboard or paper), and run them in a
scheduled/nightly suite, not on every PR.

## What's here

```
shims/
  ragas/
    requirements.txt
    faithfulness.py        # Ragas Faithfulness      (needs context)
    context_recall.py      # Ragas LLMContextRecall  (needs context + expected)
  deepeval/
    requirements.txt
    faithfulness.py        # DeepEval FaithfulnessMetric       (needs context)
    contextual_recall.py   # DeepEval ContextualRecallMetric   (needs context + expected)
```

Each script speaks EvalCore's subprocess scorer protocol. It reads one JSON
object on stdin:

```json
{"input": "...", "output": "...", "expected": "...|null", "context": ["...", "..."]}
```

It then writes one JSON object on stdout: `{"score": float, "passed": bool, "reason": str}`.
`context` is the list of retrieved chunks EvalCore attaches per case; it is
omitted when a case has none.

## Install

Pick the library you want and install its pinned deps:

```sh
pip install -r shims/ragas/requirements.txt
# or
pip install -r shims/deepeval/requirements.txt
```

The scripts are Python 3.9+ and import only the stdlib until they actually score,
so `--check` (below) runs without any of these packages installed.

## Provider credentials

Ragas and DeepEval call an LLM through their own clients, which read provider
keys (e.g. `OPENAI_API_KEY`) from the environment. Export the key before
running a real suite:

```sh
export OPENAI_API_KEY=sk-...
```

The shim scripts never read, handle, or print key values. They rely on the
libraries picking the key up from the environment.

## Wiring into `evals.yaml`

```yaml
scorers:
  - type: subprocess
    cmd: "python3 shims/ragas/faithfulness.py"
```

Swap the path for any of the four scripts. The recall metrics additionally
require each case to have a non-empty `expected` (the ground truth); all four
require per-case `context`.

### Context is required

These are RAG metrics, so they need the retrieved chunks. If a case reaches a shim
without `context`, the script exits non-zero with:

```
this metric requires per-case context — add "context" to your dataset cases
```

Make sure your dataset cases carry `context` (and `expected`, for the recall
metrics) before wiring these in.

## `--check` self-test

Every script supports `--check`, a fully offline self-test that runs the protocol
path against a canned fake result: read stdin, validate the required
`input`/`output` fields, emit a well-formed verdict. It does **not** import
Ragas/DeepEval and does **not** call an LLM, so it needs no packages and no API
key. This is what EvalCore's own CI runs.

```sh
echo '{"input":"q","output":"a","expected":"g","context":["c"]}' \
  | python3 shims/ragas/faithfulness.py --check
# -> {"score": 1.0, "passed": true, "reason": "self-test"}
```

Malformed JSON on stdin makes `--check` exit non-zero with a message on stderr.
Note that `--check` validates the *protocol*, not a metric's preconditions: a
payload missing `context` (or `expected`) still passes `--check`, because those
are metric-input requirements enforced only in normal scoring mode.
