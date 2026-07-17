---
title: Record / replay
description: The cassette lifecycle — what's in the cache key, the four cache modes, when to re-record, the lockfile analogy, and why shell targets are never cached.
---

Record/replay caching is what makes EvalCore deterministic in CI. Every call to
a cacheable target is recorded to `.evalcore/cache.db` (SQLite), content-addressed
by a hash of the request. Reruns replay from the cache: **free, offline,
deterministic**. Treat the cache file like VCR cassettes — commit it, and CI
replays with zero LLM spend and zero flakiness.

## What's in the cache key

The cache key is a hash of the **canonical request JSON**. If any part of that
request changes, the key changes and the old recording no longer matches — so a
stale hit can never lie to you. For an `openai-compatible` target the identity
includes:

- the target **`url`** and **`model`**,
- the **`system`** prompt and any pass-through **`params`** (temperature,
  `max_tokens`, …),
- the **case input**.

For an `http` target the identity is the request shape: `url`, `method`,
`headers`, `body`, and `response_path`.

Two things are deliberately **excluded** from the key:

- **Secrets.** The API key from `api_key_env` never enters the cache, so
  `--cache replay` runs offline with no key configured. (This is also why
  credentials must go in `api_key_env`, never in static `headers:` — header
  values *are* hashed into the identity and stored in the committed cassette.)
- **`timeout_seconds`.** It doesn't change the response, so cassettes recorded
  before you tuned a timeout keep their keys.

Unset fields are omitted from the identity, so cassettes recorded by older
versions (before `system`/`params` existed) keep their keys.

## The four modes

`--cache <mode>` selects how a run uses the cassette:

| Mode | Behavior |
|---|---|
| `auto` *(default)* | Replay on a hit, record on a miss. The everyday development mode. |
| `replay` | Cache only. A miss **fails the case** — no network call. This is CI mode: offline, keyless, `$0`. |
| `live` | Always call the target and re-record, overwriting existing entries. |
| `off` | Bypass the cache entirely — call every time, record nothing. |

```sh
evalcore run evals.yaml                  # auto: replay hits, record misses
evalcore run evals.yaml --cache replay   # CI: cache only, a miss fails the case
evalcore run evals.yaml --cache live     # re-record everything
evalcore run evals.yaml --cache off      # bypass
```

Replayed runs still report the **recorded** token usage and cost, so spend stays
visible even when actual spend is `$0`.

## When to re-record

There are two distinct reasons a recording goes stale, and they are handled
differently:

- **You changed the eval.** Editing the model, URL, system prompt, params, or a
  case's input changes the cache key **automatically**. On the next `auto` run
  the new request is a miss and gets recorded — no manual step. The change shows
  up in your diff (new cassette rows) where review can see it.
- **The model drifted.** The provider silently updated the model behind the same
  name, so the *same request* would now return a *different response*. The key is
  unchanged, so replay keeps serving the old recording — which is what you want
  on the PR path. Detecting drift is a **separate, scheduled concern**: run a
  nightly job with `--cache live` to re-record against the live provider and
  surface the diff, rather than letting drift leak into every PR.

## The lockfile analogy

Think of the cassette as a lockfile for model behavior:

- `--cache replay` is like building against a committed lockfile — reproducible,
  offline, exactly what CI should do.
- `--cache live` is like `cargo update` — you deliberately refresh the pinned
  behavior, review the resulting diff, and commit it.

Your PR tests protect against *your* changes; the nightly `--cache live` job is
where you choose to accept new model behavior.

## Shell targets are never cached

`shell` targets run your local code, whose behavior can change without the
config changing — so caching them would record a lie. They always execute, and
no `.evalcore/` directory appears for a purely local shell-only run. If you want
a cassette, evaluate the deployed service over the `http` target instead.
