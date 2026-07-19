---
title: Record / replay
description: "The cassette lifecycle walked end to end: the four cache modes, what's in the cache key, what re-records on what change (table), reviewing cassette diffs, and team workflows."
---

Record/replay caching is what makes EvalCore deterministic in CI. Every call to
a cacheable target is recorded to `.evalcore/cache.db` (SQLite), content-addressed
by a hash of the request. Reruns replay from the cache: free, offline,
deterministic. Treat the cache file like VCR cassettes. Commit it, and CI replays
with zero LLM spend and zero flakiness.

## A worked cassette lifecycle

Walk one suite from empty cache to committed cassette to a CI replay.

**1. First run, record.** With no cassette yet, `auto` mode calls the target
and records every response into `.evalcore/cache.db`:

```sh
export OPENAI_API_KEY=sk-...
evalcore run evals.yaml --cache auto
# → real API calls; .evalcore/cache.db is created next to evals.yaml
```

**2. Second run, replay.** Nothing changed, so every request is a cache hit.
The run is instant, offline, and `$0`. It still reports the *recorded* token
usage and cost, so spend stays visible even though nothing was actually spent:

```sh
evalcore run evals.yaml --cache auto     # all hits — no network, no spend
```

**3. Commit the cassette.** It is a reviewed project artifact:

```sh
git add .evalcore/cache.db
git commit -m "Record eval cassettes"
```

**4. CI replays it, keyless.** With the cassette committed, CI runs `--cache
replay`: cache only, no network, no `OPENAI_API_KEY` needed. A miss is a failure,
never a silent live call:

```sh
evalcore run evals.yaml --cache replay
```

If a case was never recorded, replay says so plainly and fails that case. The run
still finishes, because failures are data:

```
FAIL new
     target error: cache miss for case "new" in replay mode — record it first with --cache auto (or live)
```

## The four modes

`--cache <mode>` selects how a run uses the cassette:

| Mode | Behavior |
|---|---|
| `auto` *(default)* | Replay on a hit, record on a miss. The everyday development mode. |
| `replay` | Cache only. A miss **fails the case**, with no network call. This is CI mode: offline, keyless, `$0`. |
| `live` | Always call the target and re-record, overwriting existing entries. |
| `off` | Bypass the cache entirely: call every time, record nothing. |

```sh
evalcore run evals.yaml                  # auto: replay hits, record misses
evalcore run evals.yaml --cache replay   # CI: cache only, a miss fails the case
evalcore run evals.yaml --cache live     # re-record everything
evalcore run evals.yaml --cache off      # bypass
```

## What's in the cache key

The cache key is a SHA-256 of the canonical request JSON:
`{"identity": <target identity>, "input": <case input>}`. If any part of that
changes, the key changes and the old recording no longer matches, so a stale hit
can never lie to you.

For an `openai-compatible` target the identity includes the `url`, `model`, the
`system` prompt, and any pass-through `params` (temperature, `max_tokens`, …).
For an `http` target it is the request shape: `url`, `method`, `headers`, `body`,
and `response_path`. The case input is always part of the key on top of the
identity.

Two things are deliberately excluded:

- Secrets. The API key from `api_key_env` never enters the cache, so
  `--cache replay` runs offline with no key configured. (This is also why
  credentials must go in `api_key_env`, never in static `headers:`. Header
  values *are* hashed into the identity and stored in the committed cassette.)
- `timeout_seconds` and `max_retries`. They change *how* you call, not *what*
  the model would answer, so tuning them keeps existing cassettes valid.

Unset optional fields are omitted from the identity (not serialized as `null`),
so cassettes recorded by older versions, from before `system`/`params` existed,
keep their keys. Full invariants are in the
[cache and determinism reference](/evalcore/reference/cache-and-determinism/).

## What re-records on what change

| You change… | Re-records? | Why |
|---|---|---|
| A case's `input` | Yes | The input is part of the key. |
| `model` or `url` | Yes | Different endpoint/model → different answer. |
| `system` prompt | Yes | Changes the request the model sees. |
| `params` (temperature, `max_tokens`, …) | Yes | Changes the request. |
| An `http` target's `method`, `body`, `headers`, or `response_path` | Yes | The request shape is the identity. |
| A judge scorer's `rubric` | Yes | The rubric is embedded in the judge prompt, which *is* the judge call's input. |
| `api_key_env` value (rotating a key) | No | Secrets are excluded from the key. |
| `timeout_seconds` / `max_retries` | No | They change how you call, not the answer. |
| `cost:` rates | No | Accounting only; usage is unchanged. |
| The scorer thresholds, gates, concurrency | No | These never reach the target. |
| The model drifts behind the same name | No (key unchanged) | Detected separately; see below. |

## When to re-record

There are two distinct reasons a recording goes stale, and they are handled
differently:

- You changed the eval. Editing the model, URL, system prompt, params, a judge
  rubric, or a case's input changes the cache key automatically. On the next
  `auto` run the new request is a miss and gets recorded, with no manual step.
  The change shows up in your diff (new cassette rows) where review can see it.
- The model drifted. The provider silently updated the model behind the same
  name, so the *same request* would now return a *different response*. The key
  is unchanged, so replay keeps serving the old recording, which is what you
  want on the PR path. Detecting drift is a separate, scheduled concern: run a
  nightly job with `--cache live` to re-record against the live provider and
  surface the diff, rather than letting drift leak into every PR. The nightly
  workflow is in [Running in CI](/evalcore/guides/running-in-ci/).

## The lockfile analogy

Think of the cassette as a lockfile for model behavior:

- `--cache replay` is like building against a committed lockfile: reproducible,
  offline, exactly what CI should do.
- `--cache live` is like `cargo update`. You deliberately refresh the pinned
  behavior, review the resulting diff, and commit it.

Your PR tests protect against *your* changes; the nightly `--cache live` job is
where you choose to accept new model behavior.

## Reviewing cassette diffs

`.evalcore/cache.db` is SQLite, a binary file, so `git diff` won't render it
readably. Review recorded behavior the way you review any recorded fixture:

- Keep commits small and focused. Record and commit cassettes in the same PR as
  the eval change that produced them, so the reason for the new rows is in the
  diff description.
- Read the report, not the DB. When behavior changes, the signal you review is
  the run's output (the terminal diff, or the `--html` report), not the raw
  SQLite bytes. A `--cache live` re-record that changes an answer shows up as a
  regression or a score change in the report.
- Regenerate, don't hand-edit. A cassette is a recording; if it looks wrong,
  re-record it (`--cache live`) rather than editing the database.

## Team workflows: who commits cassettes

- Whoever changes the eval records it. If your PR edits a prompt, adds a case,
  or bumps a model, run `--cache auto` locally with credentials, then commit the
  updated `.evalcore/cache.db` alongside your config change. CI then replays it
  keyless.
- One person owns the nightly. Drift re-records (`--cache live`) come from the
  scheduled job or a designated maintainer, reviewed and committed deliberately
  rather than smeared across feature PRs.
- **Corruption is loud, never silent.** A corrupt cache entry is an error that
  tells you to delete the cache file. It never falls back to a live call, which
  would un-determinize a replay run. If replay reports corruption, delete
  `.evalcore/cache.db` and re-record.

## Shell targets are never cached

`shell` targets run your local code, whose behavior can change without the
config changing, so caching them would record a lie. They always execute, and
no `.evalcore/` directory appears for a purely local shell-only run. If you want
a cassette for a service you deploy, evaluate it over the
[`http` target](/evalcore/guides/evaluating-rest-apis/) instead.
