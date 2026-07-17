# evalcore-core

Domain types (`TestCase`, `TargetOutput`, `Score`, `CaseResult`, `RunSummary`), the **`Target` and `Scorer` traits**, dataset loading, and the run engine. Depends only on `evalcore-config`.

## Layout

- `types.rs` — domain types + the `Scorer` trait (trait here, implementations in `evalcore-scorers`, so there's no dependency cycle).
- `baseline.rs` — pure `compare(&baseline, &current) -> BaselineDiff`, matched by case id. Gate semantics are user-facing contract: regressions and new-failing cases fail the gate; accepted failures, fixes, and removals don't. Storage is `evalcore-store`'s job, rendering is `evalcore-report`'s.
- `dataset.rs` — JSONL loading. Errors must cite `file:line`.
- `target.rs` — `Target` trait, `ShellTarget`, `OpenAiCompatTarget`, and the `build_target`/`build_target_with` factories (env-var secrets resolve here; `SecretPolicy::Optional` exists only for replay runs). The OpenAI target retries transient failures (429/5xx/transport) with deterministic backoff honoring `Retry-After`, and captures `usage` into `TokenUsage`. `cache_identity` invariants: everything that changes the request (model, url, `system`, `params`) is IN; retry/cost settings stay OUT (they change how we call, not what the model answers); unset optional fields are OMITTED from the identity JSON, not serialized as null — otherwise adding a config field would invalidate every existing cassette (there's a shape-pinning test).
- `engine.rs` — `run_suite(target, cases, scorers, RunOptions)`: concurrent execution via `buffered(n)`, which **preserves dataset order** in results. That ordering is load-bearing (stable reports, future baseline diffs) — don't switch to `buffer_unordered` without re-sorting. Costs each case from `RunOptions::cost_rates`; `budget_usd` turns over-budget cases into failed-with-reason results, never a mid-run abort.

## Rules

- A target error produces a failed `CaseResult` with `error` set and no scores — never a panic, never a skipped case.
- A scorer `Err` is converted by the engine into a failing `Score` with `reason: "scorer error: …"` — one bad scorer must not abort the run.
- HTTP targets: one `reqwest::Client` per target (pooling); errors include status + first ~200 chars of body; measure `latency_ms` around the call only.
- Adding a target type? Follow the `new-target` skill. Prefer protocol-shaped targets over vendor SDKs.
- Tests: unit tests inline; HTTP behavior in `tests/openai_target.rs` via wiremock (happy path + non-200 + malformed body). No real network in tests, ever.

## Roadmap hooks (PRD §6)

Record/replay cache and rate-limit-aware scheduling land in this crate (engine + a future `cache.rs` keyed on canonicalized request hashes). Keep `run_case` pure enough to slot a cache between target and scorers.
