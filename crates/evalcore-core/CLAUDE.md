# evalcore-core

> Parent: [root CLAUDE.md](../../CLAUDE.md) · architecture: [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) · map: [MAP.md](../../MAP.md)

Domain types (`TestCase`, `TargetOutput`, `Score`, `CaseResult`, `RunSummary`), the **`Target` and `Scorer` traits**, dataset loading, and the run engine. Depends only on `evalcore-config`.

## Layout

- `types.rs` — domain types + the `Scorer` trait (trait here, implementations in `evalcore-scorers`, so there's no dependency cycle).
- `baseline.rs` — pure `compare(&baseline, &current) -> BaselineDiff`, matched by case id. Gate semantics are user-facing contract: regressions and new-failing cases fail the gate; accepted failures, fixes, and removals don't. Storage is `evalcore-store`'s job, rendering is `evalcore-report`'s.
- `dataset.rs` — JSONL loading. Errors must cite `file:line`.
- `target.rs` — `Target` trait, `ShellTarget`, `OpenAiCompatTarget`, `TraceTarget`, and the `build_target`/`build_target_with` factories (env-var secrets resolve here; `SecretPolicy::Optional` exists only for replay runs).
- `http_target.rs` — `HttpTarget`: evaluate any HTTP/JSON endpoint (your own deployed app) through the same cache and retry policy.
- `engine.rs` — `run_suite(target, cases, scorers, RunOptions)`: concurrent execution via `buffered(n)`, which **preserves dataset order** in results. That ordering is load-bearing (stable reports, baseline diffs) — don't switch to `buffer_unordered` without re-sorting. Costs each case from `RunOptions::cost_rates`; `budget_usd` turns over-budget cases into failed-with-reason results, never a mid-run abort.

## HTTP target invariants

- Shared `pub(crate)` helpers, one implementation for every HTTP target, never forked: `retry_with_backoff`/`AttemptError` (deterministic backoff honoring `Retry-After`; 429/5xx/transport/timeout are transient and retried), `resolve_api_key`, `build_http_client` (fallible — never `Client::new`, which panics on TLS init; factories fail fast).
- One pooled `reqwest::Client` per target. Per-attempt `timeout_seconds` (config default 120); timeout errors name the budget (`timed out after Ns`). Errors include status + first ~200 chars of body. `latency_ms` is measured around the call only. OpenAI `usage` is captured into `TokenUsage`.
- `HttpTarget`: `{{input}}` is percent-encoded into `url` and substituted verbatim into every string value of the JSON `body` (keys untouched). `response_path` is an RFC 6901 pointer into a 2xx JSON body (omit for the raw text). `tokens` is always `None` — generic APIs have no standard usage shape, so v1 has no cost. User-facing YAML lives in the site docs (`reference/configuration.md`), not here.

## cache_identity invariants

- IN: everything that changes the request — model, url, `system`, params; for `HttpTarget` the request shape `{"http": {url, method, [headers], [body], [response_path]}}`.
- OUT: secrets, `max_retries`, `timeout_seconds`, cost settings. They change how we call, not what the model answers, and secrets must never be persisted. Because `HttpTarget` header **values** are hashed into the committed cache, secrets never go in `headers:` — use `api_key_env` (a `headers:` name that collides case-insensitively with the auth header is rejected at config validation).
- Unset optional fields are OMITTED from the identity JSON, never serialized as null — otherwise adding a config field would invalidate every existing cassette. Shape-pinning tests guard the exact bytes.
- `TraceTarget` ingests a recorded trace (`normalize_trace`) instead of calling anything and always attaches `TargetOutput.trajectory` for the `trajectory` scorer; `text` is the trace's final answer (`final_output` / OTel `output.value`|`gen_ai.completion`) when present, else the serialized trajectory JSON. Its `cache_identity` is `None`, so `trajectory` rides live outputs only, never a cassette (shape-pinning test in `types.rs`).

## Rules

- A target error produces a failed `CaseResult` with `error` set and no scores — never a panic, never a skipped case.
- A scorer `Err` is converted by the engine into a failing `Score` with `reason: "scorer error: …"` — one bad scorer must not abort the run.
- Adding a target type? Follow the `new-target` skill. Prefer protocol-shaped targets over vendor SDKs.
- Tests: unit tests inline; HTTP behavior in `tests/openai_target.rs` via wiremock. The root CLAUDE.md testing conventions apply.
