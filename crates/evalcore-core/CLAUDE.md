# evalcore-core

> Parent: [root CLAUDE.md](../../CLAUDE.md) · architecture: [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) · map: [MAP.md](../../MAP.md)

Domain types (`TestCase`, `TargetOutput`, `Score`, `CaseResult`, `RunSummary`), the **`Target` and `Scorer` traits**, dataset loading, and the run engine. Depends only on `evalcore-config`.

## Layout

- `types.rs` — domain types + the `Scorer` trait (trait here, implementations in `evalcore-scorers`, so there's no dependency cycle).
- `baseline.rs` — pure `compare(&baseline, &current) -> BaselineDiff`, matched by case id. Gate semantics are user-facing contract: regressions and new-failing cases fail the gate; accepted failures, fixes, and removals don't. Storage is `evalcore-store`'s job, rendering is `evalcore-report`'s.
- `dataset.rs` — JSONL loading. Errors must cite `file:line`.
- `target.rs` — `Target` trait, `ShellTarget`, `TraceTarget`, the shared HTTP policy (`retry_with_backoff`, `build_http_client`, `resolve_api_key`, `resolve_sigv4`), and the `build_target`/`build_target_with` factories (env-var secrets resolve here; `SecretPolicy::Optional` exists only for replay runs).
- `chat.rs` — the `ChatWireFormat` trait and the shared `ChatTarget<F>`. **At three wire formats the abstraction earned itself**: the retry loop, client construction, status/transient classification, body-read timeout, and latency measurement were byte-identical in all three copies and now live here once. A format supplies only endpoint, request body, key header, extra headers, response parsing, and cache identity. The trait is `pub` solely because the per-provider type aliases are — it is an internal seam, not an extension point (rule 1: no user should write Rust to add a provider).
- `openai.rs` / `anthropic.rs` / `gemini.rs` — the three formats, as `ChatTarget` type aliases (`OpenAiCompatTarget`, `AnthropicTarget`, `GeminiTarget`) so existing call sites and the public API are unchanged.
- `sigv4.rs` — AWS Signature Version 4, hand-rolled over `sha2` + `hmac`. Deliberately not `aws-config`: EvalCore needs signing, not credential *resolution*, and the AWS chain (env → profile → IMDS → assumed role) would put a network round-trip and a clock read behind every target build. `sign_request` is pure given a timestamp; `sign_post` is the one place that reads the clock.
- `http_target.rs` — `HttpTarget`: evaluate any HTTP/JSON endpoint (your own deployed app) through the same cache and retry policy.
- `engine.rs` — `run_suite(target, cases, scorers, RunOptions)`: concurrent execution via `buffered(n)`, which **preserves dataset order** in results. That ordering is load-bearing (stable reports, baseline diffs) — don't switch to `buffer_unordered` without re-sorting. Costs each case from `RunOptions::cost_rates`; `budget_usd` turns over-budget cases into failed-with-reason results, never a mid-run abort.

## HTTP target invariants

- Shared `pub(crate)` helpers, one implementation for every HTTP target, never forked: `retry_with_backoff`/`AttemptError` (deterministic backoff honoring `Retry-After`; 429/5xx/transport/timeout are transient and retried), `resolve_api_key`, `build_http_client` (fallible — never `Client::new`, which panics on TLS init; factories fail fast).
- One pooled `reqwest::Client` per target. Per-attempt `timeout_seconds` (config default 120); timeout errors name the budget (`timed out after Ns`). Errors include status + first ~200 chars of body. `latency_ms` is measured around the call only. OpenAI `usage` is captured into `TokenUsage`.
- `AnthropicTarget`: auth is `x-api-key` plus a pinned `anthropic-version` header (it moves only with the parser); `system` and `max_tokens` are top-level body fields, so both are reserved in `params`. The response `content` is an array of typed blocks — filter to `type == "text"` and concatenate, never `content[0].text`, which is absent whenever thinking is on. Usage is `input_tokens`/`output_tokens`. A policy decline is **HTTP 200** with `stop_reason: "refusal"`: report it as a permanent failure naming the refusal, never a malformed-body error.
- **Auth and dialect are independent axes** (`ChatAuth` × `AnthropicDialect`), and must stay that way — SigV4 signs a plain Messages request against the Messages-shaped AWS endpoints (Bedrock Mantle, Claude Platform on AWS) exactly as it signs a legacy `bedrock-runtime` InvokeModel one. Collapsing them into one knob is the mistake to avoid; neither belongs on the `type:` axis.
- `GeminiTarget`: the model rides in the URL path after a colon, the request is `contents[].parts[]` with a `systemInstruction` *Content* value, and generation tunables nest under `generationConfig` (so `params` nests too — pass the structure through, don't invent a translation layer). **The API key goes in the `x-goog-api-key` header, never the `?key=` query parameter** — request URLs are quoted in error messages and reports, so a query-string secret would leak into them. Skip `parts[]` entries flagged `thought: true`; they are reasoning, not the answer. `promptFeedback.blockReason` and a non-`STOP` `finishReason` are readable failures, not parse errors. Gemini's OpenAI-compatible endpoint needs no code, so document that before reaching for this target.
- `HttpTarget`: `{{input}}` is percent-encoded into `url` and substituted verbatim into every string value of the JSON `body` (keys untouched). `response_path` is an RFC 6901 pointer into a 2xx JSON body (omit for the raw text). `tokens` is always `None` — generic APIs have no standard usage shape, so v1 has no cost. User-facing YAML lives in the site docs (`reference/configuration.md`), not here.

## cache_identity invariants

- IN: everything that changes the request — model, url, `system`, params; `max_tokens` for `AnthropicTarget` (a lower cap truncates the answer, so it is request content, not an operational knob); `dialect` when it is `bedrock-invoke` (different path and body); for `HttpTarget` the request shape `{"http": {url, method, [headers], [body], [response_path]}}`.
- The `type` discriminator keeps wire formats in disjoint keyspaces: an `anthropic` identity can never collide with an `openai-compatible` one at the same url/model, so adding this target invalidated no cassette.
- OUT: secrets (including every AWS credential), the `auth` mode, `max_retries`, `timeout_seconds`, cost settings. They change how we call, not what the model answers, and secrets must never be persisted. Because `HttpTarget` header **values** are hashed into the committed cache, secrets never go in `headers:` — use `api_key_env` (a `headers:` name that collides case-insensitively with the auth header is rejected at config validation).
- Unset optional fields are OMITTED from the identity JSON, never serialized as null — otherwise adding a config field would invalidate every existing cassette. Shape-pinning tests guard the exact bytes.
- `TraceTarget` ingests a recorded trace (`normalize_trace`) instead of calling anything and always attaches `TargetOutput.trajectory` for the `trajectory` scorer; `text` is the trace's final answer (`final_output` / OTel `output.value`|`gen_ai.completion`) when present, else the serialized trajectory JSON. Its `cache_identity` is `None`, so `trajectory` rides live outputs only, never a cassette (shape-pinning test in `types.rs`).

## Rules

- A target error produces a failed `CaseResult` with `error` set and no scores — never a panic, never a skipped case.
- A scorer `Err` is converted by the engine into a failing `Score` with `reason: "scorer error: …"` — one bad scorer must not abort the run.
- Adding a target type? Follow the `new-target` skill. Prefer protocol-shaped targets over vendor SDKs.
- Tests: unit tests inline; HTTP behavior per format in `tests/{openai,anthropic,gemini,sigv4}_target.rs` via wiremock. The root CLAUDE.md testing conventions apply. SigV4 signing is tested as a pure function against a fixed timestamp — never by calling AWS.
