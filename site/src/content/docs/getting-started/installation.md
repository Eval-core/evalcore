---
title: Installation
description: Install EvalCore from crates.io, grab a prebuilt binary, or run it as a GitHub Action.
---

EvalCore is a single binary. Install it from crates.io, download a prebuilt
release binary, or run it as a GitHub Action — no runtime, no services.

## From crates.io

```sh
cargo install evalcore
```

This builds and installs the `evalcore` binary from
[crates.io](https://crates.io/crates/evalcore). It is the simplest path when you
already have a Rust toolchain.

## Prebuilt binaries

Every release attaches prebuilt binaries to
[GitHub Releases](https://github.com/eval-core/evalcore/releases), so you can
skip the Rust toolchain entirely:

- **Linux x64**
- **macOS x64** (Intel)
- **macOS arm64** (Apple Silicon)

Download the archive for your platform from the latest release, extract the
`evalcore` binary, and put it on your `PATH`:

```sh
# Example: download and unpack a release archive, then move the binary
tar -xzf evalcore-*.tar.gz
mv evalcore /usr/local/bin/
evalcore --version
```

## GitHub Action

In GitHub Actions, one step installs the release binary (falling back to
`cargo` if needed), runs a suite, writes the report to the job step summary, and
exits with the suite's gate code — so the job passes or fails with your evals:

```yaml
- uses: eval-core/evalcore@v0.5.0
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
```

- `config` points at your `evals.yaml`.
- `args` are passed through to `evalcore run` — here, replay from the committed
  cassette (offline, keyless, `$0`) and gate against the `main` baseline.

Pin the action to a released tag (`@v0.5.0`) so CI is reproducible. See
[Running in CI](/evalcore/guides/running-in-ci/) for the full CI story.

## Verify the install

```sh
evalcore --version
evalcore validate examples/quickstart/evals.yaml
```

`evalcore validate` parses and checks a config without running anything. Next,
head to the [Quickstart](/evalcore/getting-started/quickstart/).
