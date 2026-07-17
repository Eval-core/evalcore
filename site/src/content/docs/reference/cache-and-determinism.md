---
title: Cache & determinism
description: The record/replay cache — location, cache-key contents per target type, the four modes, the cassette-commit workflow, baseline storage, and EvalCore's determinism guarantees.
---

EvalCore's record/replay cache makes reruns free, offline, and deterministic:
every cacheable target call is stored keyed by a content hash of the canonical
request, so a committed cache lets CI replay a suite with zero LLM spend and no
API keys. This page documents what goes into a cache key, the four modes, and
the determinism guarantees the cache is built on.

## Cache location

The cache is a single SQLite file at `.evalcore/cache.db`, resolved relative to
the config file's directory. It is a project artifact — like a VCR cassette —
and committing it is the intended workflow. The directory and file are created
on demand: a purely local run (a `shell` target, no judge, no baseline) never
opens the store, so no `.evalcore/` directory appears.

The file holds two tables: `llm_cache` (the record/replay entries) and `runs`
(saved [baselines](#baselines)).

## What is in a cache key

A cache key is the SHA-256 of the canonical request JSON:

```json
{"identity": <target.cache_identity()>, "input": <case input>}
```

`serde_json` serializes objects with **sorted keys**, which is what makes the
canonical string stable — the `preserve_order` feature is banned workspace-wide
because enabling it would silently invalidate every cache on disk. The
`identity` is the target's `cache_identity()`; anything that changes what would
be sent must be inside it, and secrets must never be.

Two design rules govern every identity: **unset optional fields are omitted**
(not serialized as `null`), so adding a new config field never invalidates
existing cassettes; and **secrets and call-mechanics are excluded**, so they
never get persisted and changing them never re-records.

### `openai-compatible` identity

```json
{"type": "openai-compatible", "url": "…", "model": "…",
 "system": "…", "params": {…}}
```

| Field | In the key? |
|---|---|
| `url`, `model` | yes |
| `system` | yes, when set (omitted otherwise) |
| `params` | yes, when set and non-empty (omitted otherwise) |
| `api_key_env` / the resolved key | **no** — a secret |
| `max_retries` | **no** — changes how we call, not what the model answers |
| `timeout_seconds` | **no** — operational knob (since v0.5.0, excluded so older cassettes keep their keys) |
| `cost` rates | **no** — accounting, not request content |

### `http` identity

```json
{"http": {"url": "…", "method": "…", "headers": {…},
          "body": {…}, "response_path": "…"}}
```

| Field | In the key? |
|---|---|
| `url` (the pre-substitution template) | yes |
| `method` (normalized uppercase) | yes |
| `headers` | yes, when non-empty; names lowercased and sorted |
| `body` (the pre-substitution template) | yes, when set |
| `response_path` | yes, when set |
| `api_key_env` / the resolved key | **no** — a secret |
| `auth_header`, `auth_prefix` | **no** |
| `max_retries`, `timeout_seconds` | **no** |

Because header **values** are in the identity — and thus hashed into the
committed `.evalcore/cache.db` — never put secrets in `headers:`. Use
`api_key_env`, which never enters the cache. A `headers:` name that collides
case-insensitively with the auth header is rejected at config validation (see
the [http validation rules](../configuration/#http)).

### Uncacheable targets

`shell` and `trace` targets return `cache_identity() == None`. They are never
cached and pass straight through in every mode. Shell targets are uncacheable by
design: local code can change behavior without the config string changing, so a
recording could go stale silently. Traces are local files, never worth caching.

## The four modes

`--cache <mode>` selects behavior for cacheable targets. See the [CLI
reference](../cli/#cache-modes).

| Mode | Behavior |
|---|---|
| `auto` (default) | Replay on a hit; on a miss, call live and record the result. |
| `replay` | Cache only — a miss fails the case (`cache miss for case "<id>" in replay mode — record it first with --cache auto (or live)`), and it never calls live. |
| `live` | Always call live and overwrite the recording. |
| `off` | Bypass the cache entirely — no reads, no writes. |

Replayed outputs are returned **verbatim**, including the recorded
`latency_ms` and any recorded token usage, so cost accounting stays consistent
offline. Different identities never share entries: the same input under a
different model or URL misses and records separately.

## Cassette-commit workflow

The cache is meant to be recorded once and committed:

1. Record locally with a live-capable mode: `evalcore run evals.yaml --cache
   auto` (records misses) or `--cache live` (re-records everything).
2. Commit `.evalcore/cache.db` alongside the config and datasets.
3. In CI, run `evalcore run evals.yaml --cache replay`. Replay never calls the
   network and never reads API keys — a miss is a failure, so CI is
   deterministic and free.

Because `--cache replay` uses the optional-secret policy, unset `api_key_env`
variables are fine there — the committed cassette carries every recorded answer.

## Determinism guarantees

Identical inputs produce identical outputs everywhere:

- **Dataset order.** Results stay in dataset order even though cases run
  concurrently — the engine buffers completions back into input order. Reports
  and diffs are therefore stable.
- **Pure reporters.** Every reporter is a pure `&RunSummary -> String` function
  — no I/O, no clock, no environment — so identical runs render byte-identical
  reports (they are snapshot-tested).
- **Sorted JSON keys.** Canonical request JSON and every rendered JSON payload
  sort object keys (`preserve_order` is banned), so cache keys and reports don't
  drift with map iteration order.
- **No clock reads** except latency measurement. Replay returns recorded
  latencies, so even timing is reproducible under `--cache replay`.
- **Deterministic retries.** Transient failures back off on a fixed schedule
  (500ms, 1s, 2s, … capped at 10s, honoring `Retry-After`) with no jitter.

A corrupt cache entry is surfaced as an error telling you to delete the cache
file — never a silent live call, which would un-determinize a replay run.

## Baselines

Baselines are stored in the same `.evalcore/cache.db` file, in the `runs` table.
`--save-baseline <label>` appends this run's per-case snapshot under the label;
`--baseline <label>` loads the **newest** row with that label (labels are not
unique, so each save appends and the latest wins) and compares. Labels are
independent of each other.

A saved baseline is a pure per-case snapshot: suite-gate results are run-scoped
acceptance criteria, not case data, so they are cleared before a summary is
persisted (and old rows that predate the `gates` field still deserialize and
re-serialize byte-identically). Because the store also holds baselines, a
`--baseline` or `--save-baseline` flag opens `.evalcore/cache.db` even for a
`shell` target that never touches the cache.

See the [CLI reference](../cli/#baselines) for how the baseline diff prints and
how it flips the exit-code contract.
