# evalcore-core

Domain types (`TestCase`, `TargetOutput`, `Score`, `CaseResult`, `RunSummary`), the **`Target` and `Scorer` traits**, dataset loading, and the run engine. Depends only on `evalcore-config`.

## Layout

- `types.rs` — domain types + the `Scorer` trait (trait here, implementations in `evalcore-scorers`, so there's no dependency cycle).
- `dataset.rs` — JSONL loading. Errors must cite `file:line`.
- `target.rs` — `Target` trait, `ShellTarget`, `OpenAiCompatTarget`, and the `build_target` factory (env-var secrets resolve here, fail-fast).
- `engine.rs` — `run_suite`: concurrent execution via `buffered(n)`, which **preserves dataset order** in results. That ordering is load-bearing (stable reports, future baseline diffs) — don't switch to `buffer_unordered` without re-sorting.

## Rules

- A target error produces a failed `CaseResult` with `error` set and no scores — never a panic, never a skipped case.
- A scorer `Err` is converted by the engine into a failing `Score` with `reason: "scorer error: …"` — one bad scorer must not abort the run.
- HTTP targets: one `reqwest::Client` per target (pooling); errors include status + first ~200 chars of body; measure `latency_ms` around the call only.
- Adding a target type? Follow the `new-target` skill. Prefer protocol-shaped targets over vendor SDKs.
- Tests: unit tests inline; HTTP behavior in `tests/openai_target.rs` via wiremock (happy path + non-200 + malformed body). No real network in tests, ever.

## Roadmap hooks (PRD §6)

Record/replay cache and rate-limit-aware scheduling land in this crate (engine + a future `cache.rs` keyed on canonicalized request hashes). Keep `run_case` pure enough to slot a cache between target and scorers.
