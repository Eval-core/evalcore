---
name: new-target
description: Add a new target type to EvalCore (a new `type:` under `targets:` in evals.yaml), e.g. a provider adapter or transport. Follow this checklist so schema, adapter, factory, and wiremock tests stay in sync.
---

# Add a new target

A target is the thing being evaluated: it receives a `TestCase` and returns a `TargetOutput`. All targets are async and must be usable concurrently.

Checklist — all steps, in order:

1. **Config surface** (`crates/evalcore-config/src/lib.rs`):
   - Add a variant to `TargetConfig` with a kebab-case tag.
   - Secrets are NEVER inline in YAML — take an `api_key_env: <ENV_VAR_NAME>` field and read the env var in the factory.
   - Add a parse test.
2. **Implementation** (`crates/evalcore-core/src/target.rs`, or a new file if it needs its own module):
   - A struct implementing `Target` (`async_trait`).
   - Measure `latency_ms` around the actual call with `std::time::Instant`.
   - Propagate HTTP failures with status + truncated body in the error; never swallow them into empty output.
   - Reuse one `reqwest::Client` per target instance (connection pooling).
   - Implement `cache_identity()` for any remote/paid target so record/replay works: return every field that changes what would be sent (model, url, params) and never a secret. Leave it `None` only for local-code targets (shell).
3. **Factory**: wire the variant into `build_target` in `target.rs`; fail fast there on missing env vars or invalid URLs.
4. **Tests** (`crates/evalcore-core/tests/`): wiremock-based — happy path, non-200 response, malformed body. No real network, ever.
5. **Docs**: add the YAML snippet to `crates/evalcore-core/AGENTS.md` and README.md if user-facing.
6. Run the `verify` skill.

Design rule: prefer protocol-shaped targets (OpenAI-compatible HTTP, shell command) over per-vendor SDKs. A new vendor that speaks the OpenAI wire format needs zero code — document that before adding an adapter.
