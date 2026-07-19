---
title: Semantic similarity
description: "Grade an answer by how close it is in meaning to the expected answer: cosine similarity over embeddings, cached and deterministic like the judge, via any OpenAI-compatible /embeddings endpoint."
---

`contains` and `exact` grade an answer by its exact characters, which is too
strict when the right answer can be phrased many ways. The `similarity` scorer
grades by **meaning**: it embeds the case's `expected` answer and the target's
output through an OpenAI-compatible `/embeddings` endpoint and scores their
**cosine similarity**. "Refunds take 30 days" and "You'll get your money back
within a month" land close in embedding space even though they share almost no
words.

Like the [`judge`](/evalcore/guides/llm-as-judge/) scorer, similarity is an
LLM-backed check whose calls ride the record/replay cache, so once recorded it
replays offline, keyless, and deterministically in CI.

## Configure the scorer

```yaml
scorers:
  - type: similarity
    url: https://api.openai.com/v1     # any OpenAI-compatible /embeddings API
    model: text-embedding-3-small
    api_key_env: OPENAI_API_KEY        # optional; resolved from the environment
    threshold: 0.8                     # pass iff cosine similarity >= threshold
```

| Field | Required | Default | Description |
|---|---|---|---|
| `url` | yes | — | Base URL of the embeddings API; `/embeddings` is appended. Must be non-empty. |
| `model` | yes | — | Embedding model name. |
| `api_key_env` | no | none | Name of the environment variable holding the API key. Secrets never appear inline in YAML. |
| `threshold` | no | `0.8` | Minimum cosine similarity to pass; a finite value in `[-1, 1]`. |

The case must define an **`expected`** answer to embed against. That is the
reference the output is compared to. A case with no `expected` is a failing
score with a reason (the scorer requires the case to define `expected` to embed
against), never a crashed run. Failures are data.

```jsonl
{"id": "refund-time", "input": "How long do refunds take?", "expected": "Refunds are processed within 30 days."}
```

## How the score works

The score is the **raw cosine similarity** of the two embedding vectors, and the
scorer passes when `score >= threshold` (with a `1e-9` tolerance, so an answer
that lands exactly on the threshold passes, matching the suite-gate tolerance):

- Cosine ranges over `[-1, 1]`. Identical direction is `1.0`, orthogonal is
  `0.0`, opposite is `-1.0`. The reported `value` is that raw number. It **can
  be negative**, and is not clamped to `[0, 1]`.
- The default `threshold` is `0.8`. Embedding models rarely push unrelated text
  below about `0.1` to `0.3`, so a useful "same meaning" bar sits high; tune it
  against your own model with a handful of known-good and known-bad pairs.
- A failing case reports the gap, e.g. `cosine similarity 0.4213 is below
  threshold 0.8`.

Because `value` is the mean-friendly raw cosine, a
[`mean_score`](/evalcore/guides/gates-and-baselines/) gate restricted to
`scorer: similarity` gates on the average semantic closeness across your suite.

## The cache story: deterministic replays

The scorer never builds its own HTTP client. The CLI injects an embeddings
target wrapped in the record/replay cache, exactly as it does for judge calls.
Each embedding call is content-addressed by the request: the cache identity is
the endpoint `url`, the `model`, and an `embeddings` discriminator (so an
embeddings call can never collide with a chat/judge call at the same URL and
model), and the cached input is the exact text embedded.

The consequences mirror the judge:

- **Replayed scores are free, offline, and deterministic.** Record once, commit
  the cassette, and `--cache replay` recomputes every similarity score from the
  recorded vectors: no key, no network, `$0`, byte-identical.
- **Changing the text re-records.** Edit a case's `expected` or the output
  changes, and the embedded text changes, so the next `--cache auto` run
  re-embeds and the new vector lands in your cassette diff.

```sh
evalcore run evals.yaml --cache auto     # records the embedding vectors
evalcore run evals.yaml --cache replay   # replays them: deterministic, keyless, $0
```

Under [trials](/evalcore/guides/trials-and-statistics/), similarity calls
re-key per trial the same way judge calls do, so each trial's embedding is its
own cache entry.

## Secrets stay in the environment

Provide credentials through `api_key_env`, naming an environment variable. The
key value never appears in the YAML and never enters the cache. The key is sent
as `Authorization: Bearer <key>` to the embeddings endpoint. Because scores
replay from the cache without a key, `--cache replay` runs in CI need no secret
configured at all; only a recording run (`--cache auto`/`live`) touches the
provider.

## See also

- [LLM-as-judge](/evalcore/guides/llm-as-judge/): the other LLM-backed scorer,
  for rubric grading rather than reference comparison.
- [Configuration reference](/evalcore/reference/configuration/#similarity): the
  full `similarity` schema and validation rules.
- [Record / replay](/evalcore/guides/record-replay/): the caching the scorer
  is built on.
