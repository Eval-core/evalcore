---
name: verify
description: Verify EvalCore end-to-end — format check, clippy, full test suite, and a real CLI run against the quickstart example. Use before declaring any change done, before commits, and after dependency or toolchain changes.
---

# Verify EvalCore

Run every step; do not stop at the first green. Report which steps passed/failed with the failing output.

1. **Format**: `cargo fmt --all --check`
2. **Lint**: `cargo clippy --workspace --all-targets -- -D warnings`
3. **Tests**: `cargo nextest run --workspace` (fallback if nextest missing: `cargo test --workspace`)
4. **End-to-end smoke** (the real binary, no network needed):
   ```sh
   cargo run -q -p evalcore -- validate examples/quickstart/evals.yaml
   cargo run -q -p evalcore -- run examples/quickstart/evals.yaml
   ```
   Expected: validate prints the config summary; run prints per-case PASS lines and a summary with 0 failed, exit code 0.
5. **Exit-code contract** (only when engine/CLI code changed): run against a config whose scorer cannot pass (e.g. `contains: value: "xyzzy-not-present"` in a temp dir) and confirm exit code is exactly 1.

Notes:
- Never verify against real LLM APIs; HTTP paths are covered by wiremock tests.
- If snapshots fail after an intentional output change: `INSTA_UPDATE=always cargo test -p evalcore-report`, then re-run normally and include the snapshot diff in your report.
