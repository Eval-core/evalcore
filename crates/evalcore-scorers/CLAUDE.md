# evalcore-scorers

Built-in scorer implementations. The `Scorer` trait lives in `evalcore-core`; this crate implements it and exposes `build_scorers(&[ScorerConfig])`. **One scorer per file.**

## Current scorers

| type | YAML | Semantics |
|---|---|---|
| `contains` | `{type: contains, value: "refund", case_sensitive: true}` | substring check (`case_sensitive` defaults to true) |
| `exact` | `{type: exact}` or `{type: exact, value: "yes"}` | equals inline `value`, else the case's `expected` |
| `regex` | `{type: regex, pattern: "^[A-Z]"}` | pattern match; compiled at build time (fail-fast) |
| `subprocess` | `{type: subprocess, cmd: "python3 my_scorer.py"}` | any-language protocol: case JSON on stdin → `{"score", "passed"?, "reason"?}` on stdout |
| `judge` | `{type: judge, url: ..., model: ..., rubric: "...", threshold: 0.7}` | LLM-as-judge via any OpenAI-compatible endpoint; calls go through the record/replay cache |
| `trajectory` | `{type: trajectory, rules: [{must_call: search_kb, with: {...}}, ...]}` | agent-trace assertions; rule semantics are spec (docs/trajectory-spec.md), pair with `type: trace` targets |
| `judge` | `{type: judge, url: ..., model: ..., rubric: "...", threshold: 0.7}` | LLM-as-judge via any OpenAI-compatible endpoint; verdict JSON `{"score", "reason"?}`; `passed = score >= threshold` (default 0.5) |

## Rules

- The `Scorer` trait is **async** (defined in `evalcore-core`). Deterministic scorers just return immediately; judge/subprocess do real awaited work.
- `JudgeScorer` takes an injected `Box<dyn Target>` — it never builds its own HTTP client. `build_scorers` takes a `build_judge_target` closure so the CLI can wrap judges in the record/replay cache while tests pass plain `build_target`. Keep it that way: it's what makes judge verdicts deterministic under `--cache replay`.
- Adding a scorer? Follow the `new-scorer` skill — config variant, one new file here, factory wiring, ≥3 tests (pass, fail-with-reason, malformed input), doc row above.
- Deterministic 0/1 scorers emit `value` exactly `0.0` or `1.0`; graded scorers use the full range and `passed = score >= 0.5` unless overridden.
- Failure `reason` must state what was expected AND what was seen; truncate output via `snippet()` (~200 chars).
- Never panic on malformed input: `Err` (engine turns it into a failing score) or a failing `Score` with a reason.
- Fallible/expensive construction (regex compile, path resolution) happens in `build_scorers`, never per-case.
- Subprocess protocol is versioned API surface — changing field names/semantics is a breaking change; update the doc comment in `subprocess.rs`, this table, and README together.
- Tests for subprocess-style scorers must use commands that read stdin (`cat >/dev/null; …`) to avoid EPIPE.
