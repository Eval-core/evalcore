# Contributing to EvalCore

Thanks for taking the time. This file covers how to get a working checkout, what
the review bar is, and the few architectural rules that are load-bearing enough
that a PR breaking them will be sent back.

## Getting set up

You need a stable Rust toolchain (1.75 or newer; `rust-toolchain.toml` pins the
channel) and, optionally, `cargo-nextest` for faster test runs.

```sh
git clone https://github.com/eval-core/evalcore
cd evalcore
cargo build
cargo nextest run --workspace     # or: cargo test --workspace
```

There is no other setup. No database to provision, no API keys, no services.
Every test in the suite runs offline. HTTP is faked with `wiremock`, and the
example suites use shell scripts as targets.

## The checks CI runs

Run these before opening a PR. They are the same four commands CI runs, so a
green local run means a green pipeline.

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo run -p evalcore -- run examples/quickstart/evals.yaml   # end-to-end smoke, no network
```

Clippy warnings are errors in CI. That is deliberate, and it keeps the diff
noise out of review.

## How the workspace fits together

Seven crates, with a dependency direction that only flows one way:

```
evalcore-config  ──►  evalcore-core  ──►  evalcore-scorers ─┐
                                     ──►  evalcore-report  ─┼──►  evalcore (bin)
                                     ──►  evalcore-store   ─┤
                                     ──►  evalcore-serve   ─┘
```

| Crate | What belongs in it |
|---|---|
| `evalcore-config` | The `evals.yaml` schema and its validation. Pure data, with no I/O, no network, and no engine logic. |
| `evalcore-core` | Domain types, the `Target` and `Scorer` traits, dataset loading, the run engine, gates, baselines, classification, trace normalization. |
| `evalcore-scorers` | Built-in scorers, one per file. Implementations only. The trait lives in core. |
| `evalcore-report` | Pure `&RunSummary -> String` renderers: terminal, JSON, JUnit, HTML. |
| `evalcore-store` | SQLite: the record/replay cache, baselines, run history. |
| `evalcore-serve` | The local read-only run viewer behind `evalcore serve`. |
| `evalcore` | The CLI binary. Wiring only. If you are writing an algorithm here, it belongs in a library crate. |

Each crate has its own `CLAUDE.md` with local rules worth reading before you
touch it.

## Rules that are not up for negotiation

These four come up in most reviews, so here they are up front.

**1. Protocols, not SDKs.** Every extension point is language-agnostic: targets
speak HTTP or shell, custom scorers speak JSON over stdin and stdout, judges are
any OpenAI-compatible endpoint, agent traces arrive as OTel or OpenInference
JSON. A design that requires a user to write Rust to extend EvalCore is the wrong
design. Rust is the engine, not the interface.

**2. Config comes first.** A user-facing feature starts as YAML in
`evalcore-config`. Write the config you want people to type, then derive the
types from it, not the other way round.

**3. Determinism is the product.** Identical inputs must produce identical
outputs, everywhere. Results stay in dataset order, reporters are pure functions,
and nothing user-visible reads the clock except latency measurement. The
record/replay cache depends on this: cache keys are a hash of the canonical
request JSON, so anything that perturbs those bytes silently invalidates every
cassette on disk. That is why serde_json's `preserve_order` feature is banned
workspace-wide. It would switch object keys from sorted to insertion order and
break every committed cache file.

**4. Failures are data.** A target error is a failed case carrying a reason. A
scorer error is a failing score carrying a reason. Runs never panic, and one bad
case never aborts a suite. If you find yourself reaching for `unwrap` outside a
test, that is the rule telling you something.

There is a fifth, narrower rule worth stating because breaking it breaks users'
CI: `evalcore run` exits `0` when the run passed and `1` otherwise. People gate
merges on that. Do not add a third exit code without a discussion first.

## Testing

- Unit tests go inline in the module, in `#[cfg(test)] mod tests`.
- Cross-crate and binary behavior goes in `tests/`.
- HTTP is tested with `wiremock` only. No test may touch the real network.
  Cover the happy path, a non-200, and a malformed body.
- CLI behavior is tested with `assert_cmd` against the real binary. Assert on
  exit codes and stable output fragments, never on latencies, which vary.
- Report formats are snapshot-tested with `insta`, on fixtures with fixed
  latencies. Regenerate with `INSTA_UPDATE=always cargo test -p evalcore-report`
  and commit the updated `.snap` files.
- A subprocess scorer used in a test must read stdin to completion
  (`cat >/dev/null; …`), or it may exit before the payload is written.

Every schema change ships with a parse test in the same PR, including one
negative test showing what gets rejected.

## Adding a scorer or a target

Both touch five places, and a PR that updates three of them will bounce. The
order that works:

1. The config surface in `evalcore-config` (schema plus validation).
2. The implementation: a new file in `evalcore-scorers`, or a `Target` impl in
   `evalcore-core`.
3. The factory that maps config to implementation.
4. Tests, including a negative one.
5. Docs: the configuration reference, and a guide page if the feature needs
   more than a table row to explain.

For a new target, also decide what `cache_identity()` returns. It must describe
everything that changes the response and must never contain a secret, because it
is hashed into a cache file that people commit to their repositories.

## Commits and pull requests

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org):

```
feat(scorers): add json-schema scorer
fix(engine): count budget-skipped cases as failures
docs(readme): document the http target auth options
```

Keep a PR to one logical change. Explain what breaks if the change is wrong,
because that is the part reviewers cannot infer from the diff. If a change
alters user-visible output or config, update `CHANGELOG.md` in the same PR.

## Reporting bugs and asking for features

Use the [issue tracker](https://github.com/eval-core/evalcore/issues). For a bug,
the minimal reproduction is the `evals.yaml` and the dataset line that triggers
it. For a security issue, do not open an issue. See [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the
Apache License 2.0, the same license as the project.
