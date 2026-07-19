---
name: new-scorer
description: Add a new scorer to EvalCore (a new `type:` under `scorers:` in evals.yaml). Follow this checklist so config schema, implementation, factory, tests, and docs stay in sync.
---

# Add a new scorer

A scorer is a function `(TestCase, TargetOutput) -> Score`. Scorers must be deterministic given their inputs (LLM-judge scorers achieve this later via the replay cache).

Checklist — all steps, in order:

1. **Config surface** (`crates/evalcore-config/src/lib.rs`):
   - Add a variant to `ScorerConfig` with kebab-case tag and serde defaults for optional fields.
   - Add a parse test for the new YAML shape, and a validation rule if the variant has constraints (e.g. regex must compile → validate in the factory, step 3).
2. **Implementation** (`crates/evalcore-scorers/src/<name>.rs` — one file per scorer):
   - A struct implementing `evalcore_core::Scorer`.
   - `name()` returns the config tag string.
   - Failure `reason` messages must say what was expected AND what was seen (truncate long output to ~200 chars).
   - Never panic on malformed input — return `Err` with context or a failing `Score` with a reason.
3. **Factory** (`crates/evalcore-scorers/src/lib.rs`): wire the new variant into `build_scorers`, doing any expensive/validating construction (compile regexes, resolve paths) here so errors surface before the run starts.
4. **Tests** (same file, `#[cfg(test)]`): minimum three — passing case, failing case (assert the reason text), and one malformed/edge input. Subprocess-style scorers must use commands that read stdin.
5. **Docs**: add the YAML snippet to the scorer table in `crates/evalcore-scorers/AGENTS.md` and, if user-facing, to README.md.
6. Run the `verify` skill.

Design rule: if the scorer needs user-provided logic, it must work via the subprocess protocol (JSON on stdin → `{"score": ..}` on stdout) — never require users to write Rust.
