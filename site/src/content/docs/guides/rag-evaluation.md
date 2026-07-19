---
title: RAG evaluation
description: Evaluate a retrieval-augmented app with EvalCore — attach retrieved context to cases, grade groundedness with the cached native judge, assert on what your pipeline retrieved, and wire the shipped Ragas/DeepEval shims into a nightly tier.
---

A retrieval-augmented (RAG) app answers from chunks it retrieved, so its evals
need one thing the others don't: the **retrieved context** the answer was
supposed to be grounded in. Since 0.6.0 a dataset case can carry that context,
and the scoring side — the `judge` scorer and `subprocess` scorers — can grade
against it. Targets never see it: your app does its own retrieval.

This guide covers attaching context, grading groundedness with the native
cached judge (the PR-path approach), asserting on what your pipeline retrieved,
and the shipped Ragas/DeepEval shims for teams standardized on those metric
definitions.

## Attach context to your cases

Add a `context` field to a case in your JSONL dataset. It is either a single
string or an array of strings (one per retrieved chunk):

```jsonl
{"id": "rag-1", "input": "How long do refunds take?", "context": "Refunds are processed within 30 days of return."}
{"id": "rag-2", "input": "What do I need for a refund?", "context": ["Refunds require an order number.", "Keep your original receipt."]}
{"id": "rag-3", "input": "Do you ship internationally?", "expected": "Yes, to 40 countries."}
```

Both forms are equivalent to the scorers — a single string normalizes to a
one-chunk list. A case with no `context` (like `rag-3`) is a normal case;
scorers that need context simply have nothing to grade against.

An **empty array** (`"context": []`) normalizes to *no context* — it is treated
exactly like an absent field, not as an empty-but-present list. Any other shape
(a number, an object, a mixed array like `["ok", 7]`) is a dataset error that
names the offending `file:line`, so a malformed case fails loudly at load time
rather than scoring against garbage.

See the [configuration reference](/evalcore/reference/configuration/#jsonl-case-format)
for the full case schema.

## Grade groundedness with the native judge

The [`judge` scorer](/evalcore/guides/llm-as-judge/) is the recommended way to
score groundedness, because — like every LLM call in EvalCore — its verdicts go
through the record/replay cache and replay deterministically. When a case carries
`context`, the judge prompt gains a clearly delimited, **numbered** context
section, placed before the answer, so a rubric can point at it:

```yaml
targets:
  my-rag:
    type: http
    url: https://api.myapp.com/chat        # your app runs its own retrieval
    body:
      question: "{{input}}"
    response_path: /answer

datasets:
  - file: rag-cases.jsonl

scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    rubric: "Is the answer fully supported by the provided context? Answer high only if every claim is backed by a numbered context chunk and nothing is fabricated."
    api_key_env: OPENAI_API_KEY
    threshold: 0.7
```

Under the hood the judge sees the context as a numbered block between the input
and the answer it is grading:

```text
<context>
[1] Refunds require an order number.
[2] Keep your original receipt.
</context>
```

Numbering lets a rubric reference chunks ("backed by a numbered context chunk"),
and the delimiters keep the retrieved text from bleeding into the instructions.

### The cache story: what replays, what re-records

The judge's cache key is its full prompt, and **the context is part of that
prompt**. That gives you the property that makes an LLM-graded RAG suite CI-safe:

- **Verdicts replay deterministically.** Once recorded, `--cache replay` returns
  each groundedness verdict byte-for-byte — offline, keyless, `$0`. This is the
  **PR-path** approach: gate every pull request on grounded answers without
  spending money or flaking on model nondeterminism.
- **Editing the context re-records.** Change a case's `context` (or the rubric,
  or the answer) and the judge prompt changes, so the next `--cache auto` run
  re-grades that case and the new verdict lands in your cassette diff — review it
  like any behavior change.
- **Contextless prompts are unchanged.** A case with no context produces exactly
  the prompt it did before this feature existed, so pre-existing cassettes keep
  replaying untouched.

```sh
evalcore run evals.yaml --cache auto     # records groundedness verdicts
evalcore run evals.yaml --cache replay   # replays them: deterministic, keyless, $0
```

For rubric design (keep it binary-decidable, name the evidence) and the full
re-record table, see [LLM-as-judge](/evalcore/guides/llm-as-judge/).

## Targets never see the context

Context lives on the **scoring side only**. Your target — the RAG app — is
responsible for its own retrieval; EvalCore does not inject the case's `context`
into the request, and cache keys hash only the target identity plus `input`, so
context never reaches the target or the cache path.

The practical consequences:

- **If the target needs something at request time, put it in `input`.** The
  `context` field is what the answer *should* be grounded in for grading — not a
  channel for feeding your app. A RAG service retrieves its own chunks from the
  question you send as `input`.
- **To evaluate retrieval itself, assert on what your pipeline retrieved.** Put
  the chunks your pipeline actually returned into the case's `context` (captured
  from a real run), then score them. The `subprocess` scorer receives the context
  list on stdin, so a check like "did we retrieve the chunk that contains the
  answer?" is a few lines in any language:

  ```json
  {"input": "How long do refunds take?", "output": "...", "expected": "30 days", "context": ["Refunds are processed within 30 days.", "..."]}
  ```

  The `context` array is present on stdin **only when the case carries it**
  (omitted otherwise), and keys stay alphabetically ordered. See the
  [subprocess protocol](/evalcore/reference/subprocess-protocol/) for the full
  payload contract, and the shims below for retrieval-recall metrics you can drop
  in as-is.

## The shipped shims: Ragas and DeepEval

EvalCore ships four `subprocess` scorer scripts under `shims/` that score cases
with **Ragas** or **DeepEval** RAG metrics, for teams standardized on those exact
definitions (e.g. to match an existing dashboard or paper):

| Script | Metric | Needs |
|---|---|---|
| `shims/ragas/faithfulness.py` | Ragas Faithfulness | `context` |
| `shims/ragas/context_recall.py` | Ragas LLMContextRecall | `context` + `expected` |
| `shims/deepeval/faithfulness.py` | DeepEval FaithfulnessMetric | `context` |
| `shims/deepeval/contextual_recall.py` | DeepEval ContextualRecallMetric | `context` + `expected` |

### Install

Each library's deps are pinned in a `requirements.txt`; install the one you want:

```sh
pip install -r shims/ragas/requirements.txt
# or
pip install -r shims/deepeval/requirements.txt
```

The scripts are Python 3.9+ and import only the stdlib until they actually score,
so the `--check` self-test (below) runs with nothing installed. Ragas and
DeepEval read provider credentials (e.g. `OPENAI_API_KEY`) from the
**environment** — the shim scripts never read, handle, or print key values.

### Wire one in

Point a `subprocess` scorer at the script:

```yaml
scorers:
  - type: subprocess
    cmd: "python3 shims/ragas/faithfulness.py"
```

The recall metrics additionally require each case to have a non-empty `expected`
(the ground truth); all four require per-case `context`. A case that reaches a
shim without the context it needs exits non-zero with a clear message
(`this metric requires per-case context — add "context" to your dataset cases`),
which EvalCore surfaces as a failing score — never a crashed run.

### Self-test with `--check`

Every script supports `--check`, a fully offline self-test that exercises the
protocol path (read stdin, validate `input`/`output`, emit a well-formed verdict)
with a canned result. It does **not** import Ragas/DeepEval and does **not** call
an LLM, so it needs no packages and no API key — this is what EvalCore's own CI
runs:

```sh
echo '{"input":"q","output":"a","expected":"g","context":["c"]}' \
  | python3 shims/ragas/faithfulness.py --check
# -> {"score": 1.0, "passed": true, "reason": "self-test"}
```

`--check` validates the *protocol*, not a metric's preconditions: a payload
missing `context` still passes `--check`, because those requirements are enforced
only in normal scoring mode.

### Be honest: these call LLMs themselves — nightly tier

The shims are **not** the recommended default, and it matters why. Each shim
metric calls an LLM *itself*, inside Ragas/DeepEval. Those calls do **not** go
through EvalCore's record/replay cache, so:

- they cost real money **per case, every run**, and
- they are **non-deterministic** — the same case can score differently run to
  run.

That is the opposite of what you want on the pull-request path, where the native
cached judge gives you fast, free, deterministic gating. Put the shims in a
**scheduled/nightly tier**, the same place you catch [model drift](/evalcore/guides/running-in-ci/#7-split-pr-from-nightly-catching-model-drift):

```yaml
# .github/workflows/rag-nightly.yml
name: RAG metrics (nightly)
on:
  schedule:
    - cron: "0 7 * * *"      # 07:00 UTC daily
  workflow_dispatch:          # allow manual runs

jobs:
  rag:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "3.11"
      - run: pip install -r shims/ragas/requirements.txt
      - uses: eval-core/evalcore@v0.7.0
        with:
          config: evals/rag-nightly.yaml
          # Target answers replay from the committed cassette; the shim scorers
          # call the provider live and bill per case — nightly, never per PR.
          args: --cache replay
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
```

The nightly job is the only place that needs the provider key and spends money —
mirroring the [Running in CI](/evalcore/guides/running-in-ci/) split between a
free, deterministic PR path and a scheduled tier that touches the network.

## Choosing between the judge and the shims

| | Native `judge` scorer | Ragas / DeepEval shims |
|---|---|---|
| **Path** | Pull request / CI | Scheduled / nightly |
| **Cost** | One cached LLM call per case, reused across runs | One live, billable LLM call per case, every run |
| **Determinism** | Deterministic under `--cache replay` | Non-deterministic; results move run to run |
| **Metric** | Your rubric, in plain language | Ragas / DeepEval's exact metric definitions |
| **Reach for it when** | You want to gate PRs on groundedness, offline and free | Your team is standardized on a specific library's metrics |

Start with the native judge on the PR path. Add a shim in a nightly tier only
when you specifically need Ragas' or DeepEval's metric definitions.

For the case schema see the [configuration
reference](/evalcore/reference/configuration/#jsonl-case-format); for the scorer
payload, the [subprocess protocol](/evalcore/reference/subprocess-protocol/); for
rubric design and cache mechanics, [LLM-as-judge](/evalcore/guides/llm-as-judge/).
</content>
</invoke>
