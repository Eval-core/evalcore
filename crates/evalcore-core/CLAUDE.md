# evalcore-core

> Parent: [root CLAUDE.md](../../CLAUDE.md) · architecture: [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) · map: [MAP.md](../../MAP.md)

Domain types (`TestCase`, `TargetOutput`, `Score`, `CaseResult`, `RunSummary`), the **`Target` and `Scorer` traits**, dataset loading, and the run engine. Depends only on `evalcore-config`.

## Layout

- `types.rs` — domain types + the `Scorer` trait (trait here, implementations in `evalcore-scorers`, so there's no dependency cycle).
- `baseline.rs` — pure `compare(&baseline, &current) -> BaselineDiff`, matched by case id. Gate semantics are user-facing contract: regressions and new-failing cases fail the gate; accepted failures, fixes, and removals don't. Storage is `evalcore-store`'s job, rendering is `evalcore-report`'s.
- `dataset.rs` — JSONL loading. Errors must cite `file:line`.
- `target.rs` — `Target` trait, `ShellTarget`, `OpenAiCompatTarget`, `TraceTarget`, and the `build_target`/`build_target_with` factories (env-var secrets resolve here; `SecretPolicy::Optional` exists only for replay runs). The OpenAI target retries transient failures (429/5xx/transport) with deterministic backoff honoring `Retry-After`, and captures `usage` into `TokenUsage`. `cache_identity` invariants: everything that changes the request (model, url, `system`, `params`) is IN; retry/cost settings stay OUT (they change how we call, not what the model answers); unset optional fields are OMITTED from the identity JSON, not serialized as null — otherwise adding a config field would invalidate every existing cassette (there's a shape-pinning test). The retry policy (`AttemptError`, `retry_with_backoff`), secret resolution (`resolve_api_key`), and the client builder (`build_http_client`) live here as `pub(crate)` helpers so every HTTP target shares one implementation — never fork the backoff schedule. Both HTTP-based targets take a per-attempt `timeout_seconds` (config default 120): `new` builds the pooled `reqwest::Client` fallibly through `build_http_client` (never `Client::new`, which panics on TLS init) and the factory fails fast; a timeout is classified transient (retried) and its message names the budget (`timed out after Ns`). Like `max_retries`, `timeout_seconds` is excluded from `cache_identity` — it changes how we call, not what the model answers. `TraceTarget` ingests a recorded trace (`crate::trace::normalize_trace`) instead of calling anything, and **always** attaches the structured `TargetOutput.trajectory` so the `trajectory` scorer grades the steps; its `text` is the trace's final answer (native `final_output` / the OTel root span's `output.value`|`gen_ai.completion`) when present — so judge/`contains`/`regex`/`exact` grade the answer — else the serialized trajectory JSON as before. `cache_identity` stays `None` (traces are local files), so the new `trajectory` channel only ever rides live outputs, never a cassette (where it is always `None` and omitted from the JSON — a shape-pinning test in `types.rs` guards the old-cassette bytes).
- `http_target.rs` — `HttpTarget`: evaluate an arbitrary HTTP/JSON endpoint (your own deployed app) through the same cache and retry policy. `{{input}}` is percent-encoded into `url` (every non-alphanumeric byte) and substituted verbatim into every string value of the JSON `body` (keys untouched); `response_path` is an RFC 6901 pointer into a 2xx JSON body (omit for the raw text). `tokens` is always `None` — generic APIs have no standard usage shape, so v1 has no cost. `cache_identity` is `{"http": {url, method, [headers], [body], [response_path]}}` — the request shape only; `api_key_env`/`auth_header`/`auth_prefix`/`max_retries` and every secret stay OUT. Because header **values** are in the identity — and thus hashed into the committed `.evalcore/cache.db` — never put secrets in `headers:`; use `api_key_env`, which never enters the cache. A `headers:` name that collides (case-insensitively) with the auth header is rejected at config validation, since reqwest would otherwise send two header lines.

  ```yaml
  targets:
    my-rag:
      type: http
      url: https://api.myapp.com/chat   # {{input}} percent-encoded when substituted
      method: POST                       # default POST; GET/PUT/PATCH too (GET forbids a body)
      headers: { x-tenant: acme }        # static, NON-secret (values are hashed into the cache)
      api_key_env: MYAPP_API_KEY         # -> authorization: Bearer <key>; never cached
      auth_header: authorization         # default; x-api-key style = auth_header: x-api-key + auth_prefix: ""
      body: { question: "{{input}}" }    # {{input}} fills string values verbatim
      response_path: /answer             # RFC 6901 pointer; omit for raw body text
  ```
- `engine.rs` — `run_suite(target, cases, scorers, RunOptions)`: concurrent execution via `buffered(n)`, which **preserves dataset order** in results. That ordering is load-bearing (stable reports, future baseline diffs) — don't switch to `buffer_unordered` without re-sorting. Costs each case from `RunOptions::cost_rates`; `budget_usd` turns over-budget cases into failed-with-reason results, never a mid-run abort.

## Rules

- A target error produces a failed `CaseResult` with `error` set and no scores — never a panic, never a skipped case.
- A scorer `Err` is converted by the engine into a failing `Score` with `reason: "scorer error: …"` — one bad scorer must not abort the run.
- HTTP targets: one `reqwest::Client` per target (pooling); errors include status + first ~200 chars of body; measure `latency_ms` around the call only.
- Adding a target type? Follow the `new-target` skill. Prefer protocol-shaped targets over vendor SDKs.
- Tests: unit tests inline; HTTP behavior in `tests/openai_target.rs` via wiremock (happy path + non-200 + malformed body). No real network in tests, ever.

## Roadmap hooks (PRD §6)

Record/replay cache and rate-limit-aware scheduling land in this crate (engine + a future `cache.rs` keyed on canonicalized request hashes). Keep `run_case` pure enough to slot a cache between target and scorers.
