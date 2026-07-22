---
title: RAG faithfulness cookbook
description: "Build a copy-pasteable RAG suite that checks citations deterministically, judges faithfulness against retrieved context, and replays for $0 in CI."
---

This recipe turns two retrieval questions into a pull-request gate. Each case
keeps the question, retrieved chunks, and reference answer together. A cheap
`contains` check catches missing citations immediately; an LLM judge then checks
that every factual claim is supported by those chunks.

The first run records both your RAG endpoint's answers and the judge verdicts.
CI replays those real recordings without a network connection, provider key, or
new model spend.

## 1. Create the cases

Save this as `cases.jsonl`:

```jsonl
{"id":"refund-window","input":"How long does an approved refund take?","context":["Policy 4.2: Approved refunds are processed within 30 business days from approval."],"expected":"An approved refund is processed within 30 business days. Source: Policy 4.2."}
{"id":"international-wire","input":"When should an international wire arrive?","context":["Policy 5.3: International wires settle within 3 to 5 business days."],"expected":"An international wire settles within 3 to 5 business days. Source: Policy 5.3."}
```

Use chunks captured from the retrieval step you are evaluating, not passages
written to make the answer pass. `expected` is a concise reference answer for
recall and reviewer context; the judge still grades the target's actual output.

## 2. Configure the suite

Save this as `evals.yaml` beside the dataset. Replace the target URL and response
pointer with your RAG service, and choose an OpenAI-compatible judge available
to your team:

```yaml
targets:
  support-rag:
    type: http
    url: http://127.0.0.1:8000/answer
    method: POST
    body:
      question: "{{input}}"
    response_path: /answer

datasets:
  - file: cases.jsonl

scorers:
  # Deterministic guard: every answer must identify its evidence.
  - type: contains
    value: "Source:"
    case_sensitive: true

  # Semantic guard: every claim must be supported by retrieved context.
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    rubric: >-
      Is every factual claim in the answer supported by the numbered retrieved
      context, with no fabrication? Give a high score only when the answer also
      addresses the reference answer's essential fact.
    threshold: 0.8

run:
  gates:
    - type: pass_rate
      min: 1.0
    - type: mean_score
      scorer: judge
      min: 0.8
```

The target never receives `context` or `expected`; it performs retrieval from
the question as usual. EvalCore sends both to scorers so the judge can compare
the produced answer with the evidence and reference answer.

## 3. Record real answers and verdicts

Start your RAG service, export the judge credential, and run:

```sh
export OPENAI_API_KEY=...
evalcore validate evals.yaml
evalcore run evals.yaml --cache live
```

Read every recorded answer and judge reason before accepting the cassette. Do
not copy scores from this guide: the useful evidence is the output from your
service and your chosen judge. If it is sound, commit the suite and cassette:

```sh
git add cases.jsonl evals.yaml .evalcore/cache.db
git commit -m "test: add RAG faithfulness gate"
```

Re-record with `--cache live` whenever you deliberately want to refresh model
behavior. Changing a question, retrieved chunk, target configuration, rubric,
or answer changes the relevant cache key, so stale recordings cannot silently
stand in for the new request.

## 4. Prove replay is offline

Remove the provider key, stop the RAG service, and replay:

```sh
unset OPENAI_API_KEY
evalcore run evals.yaml --cache replay
```

Both the target answers and judge verdicts now come from
`.evalcore/cache.db`. A missing recording fails its case instead of making a
surprise live request. That makes the replay deterministic and `$0`, while the
report still shows the recorded token usage and configured virtual cost.

For a repository-owned offline example with real captured output, run:

```sh
evalcore run examples/support-rag/evals.yaml --no-history
```

That example uses the same case-level `context` shape and deterministic
citation guard, but keeps its optional production judge commented so the
repository smoke test never needs credentials. Its checked-in
[`README`](https://github.com/eval-core/evalcore/tree/main/examples/support-rag)
contains the captured transcript from the runnable suite rather than invented
scores.

## 5. Gate pull requests

The cassette is committed, so CI needs neither the target service nor the judge
secret:

```yaml
- uses: eval-core/evalcore@v0.7.5
  with:
    config: evals.yaml
    args: --cache replay
```

Keep live re-recording in a scheduled job if you also want to detect provider
drift. Pull requests should stay on replay so an unrelated model update cannot
flake the build.

## What each check catches

| Check | Catches | Does not prove |
|---|---|---|
| `contains: "Source:"` | Missing citation marker | That the cited chunk supports the answer |
| Faithfulness judge | Unsupported or fabricated claims | That retrieval found every relevant fact |
| `expected` reference | The essential fact reviewers expect | A score by itself; add a recall scorer if needed |
| `--cache replay` | Eval/config regressions against recorded behavior | Live provider drift |

For deeper retrieval-recall metrics, add the Ragas or DeepEval subprocess shims
from [RAG evaluation](/guides/rag-evaluation/) in a scheduled job. Those shims
call their providers directly and are not cassette-backed, so they should not
replace the cached native judge on the pull-request path.

## See also

- [RAG evaluation](/guides/rag-evaluation/): context semantics and metric shims.
- [LLM-as-judge](/guides/llm-as-judge/): rubric design and threshold calibration.
- [Record / replay](/guides/record-replay/): cassette keys and refresh workflows.
