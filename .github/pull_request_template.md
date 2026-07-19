<!--
Thanks for the pull request. Keep it to one logical change where you can.
CONTRIBUTING.md has the workspace layout and the architectural rules.
-->

## What this changes

<!-- One or two sentences. What is different after this merges? -->

## Why

<!-- The part a reviewer cannot infer from the diff: what breaks without this,
     or what was wrong before. Link an issue if there is one. -->

## Checks

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo nextest run --workspace` (or `cargo test --workspace`)
- [ ] `cargo run -p evalcore -- run examples/quickstart/evals.yaml`

## Scope of the change

- [ ] Changes user-visible output, config, or the exit code
- [ ] Adds or changes a config surface (schema updated in `evalcore-config`, with a negative parse test)
- [ ] Updates `CHANGELOG.md`
- [ ] Updates the docs under `site/src/content/docs/`

<!--
If you touched report formats, regenerate snapshots and commit them:
  INSTA_UPDATE=always cargo test -p evalcore-report

If you changed anything that feeds a cache key, say so explicitly. It
invalidates every cassette users have committed.
-->
