# EvalCore

Snapshot testing for AI behavior: a single-binary, config-first eval runner for LLM apps and agents. Rust workspace, Apache-2.0, GitHub org `eval-core`. Full product context (positioning, roadmap, competitive landscape) lives in `PRD.md`.

## Commands

```sh
cargo build                                            # build all crates
cargo nextest run --workspace                          # tests (fallback: cargo test --workspace)
cargo clippy --workspace --all-targets -- -D warnings  # lint (CI-enforced)
cargo fmt --all                                        # format
cargo run -p evalcore -- run examples/quickstart/evals.yaml   # E2E smoke, no network
```

Before declaring any change done, run the `verify` skill.

## Workspace layout

| Crate | Purpose |
|---|---|
| `crates/evalcore-config` | `evals.yaml` schema, parsing, validation. Pure data. |
| `crates/evalcore-core` | Domain types, `Target`/`Scorer` traits, dataset loading, run engine. |
| `crates/evalcore-scorers` | Built-in scorers (contains, exact, regex, subprocess). One per file. |
| `crates/evalcore-report` | Pure `&RunSummary -> String` reporters (terminal, JSON, JUnit). |
| `crates/evalcore-store` | SQLite storage: the record/replay cache (`CachedTarget`, `--cache` modes). |
| `crates/evalcore` | The CLI binary. Wiring only — no logic. |

Each crate has its own CLAUDE.md with local rules. `examples/quickstart/` doubles as the CLI test fixture.

## Architecture rules (do not break)

1. **Protocols over SDKs.** Extension points are language-agnostic protocols: targets speak HTTP or shell; custom scorers speak JSON over stdin/stdout; judges are OpenAI-compatible endpoints; agent traces (v0.2) arrive as OTel/OpenInference. Any design that forces users to write Rust is wrong — Rust is the engine, never the interface.
2. **Dependency direction:** `evalcore-config` ← `evalcore-core` ← {`evalcore-scorers`, `evalcore-report`, `evalcore-store`} ← `evalcore` (bin). Traits live in `evalcore-core`; implementations live downstream. Never invert.
3. **YAML-first features.** Every user-facing feature starts as config surface in `evalcore-config` (design the YAML before the types).
4. **Determinism is the product.** Identical inputs → identical outputs everywhere: results stay in dataset order, reporters are pure functions, nothing user-visible reads the clock except latency measurement. The record/replay cache is built on this: cache keys hash canonical request JSON (see `evalcore-store/CLAUDE.md` for the invariants, including why serde_json's `preserve_order` feature is banned).
5. **Failures are data.** A target error is a failed case with a reason, a scorer error is a failing score with a reason — runs never panic and one bad case never aborts the suite.
6. **Exit-code contract:** `evalcore run` exits 0 (all passed) / 1 (anything else). Users gate CI on it.

## Testing conventions

- Unit tests inline (`#[cfg(test)] mod tests`); cross-crate/binary behavior in `tests/`.
- HTTP: wiremock only — no real network in any test. Cover happy path + non-200 + malformed body.
- CLI: `assert_cmd` against the real binary; assert exit codes and stable output fragments, never latencies.
- Snapshots: insta, only on fixtures with fixed latencies. Regenerate: `INSTA_UPDATE=always cargo test -p evalcore-report`.
- Subprocess test commands must read stdin (`cat >/dev/null; …`).

## Agents & skills

- Agents: `rust-reviewer` (post-change review), `test-engineer` (coverage work).
- Skills: `verify` (full check + E2E smoke), `new-scorer`, `new-target` (checklists that keep schema/impl/factory/tests/docs in sync).

## Style

- Errors: `thiserror` enums in library crates, `anyhow` + context at edges. Messages name the file/case/scorer involved.
- Inline format args (`format!("{err}")`). No `unwrap`/`expect` outside tests.
- Public items get doc comments; comments state constraints, not narration.
