---
title: Installation
description: Install EvalCore from crates.io, download a prebuilt binary, or run it as a GitHub Action — plus verifying, upgrading, and a CI installation matrix.
---

EvalCore is a single, dependency-free binary. There is nothing to run as a
service and no runtime to install alongside it — you install one executable and
gate CI on its exit code. Pick the path that matches where you are running it.

## From crates.io

```sh
cargo install evalcore
```

This builds and installs the `evalcore` binary from
[crates.io](https://crates.io/crates/evalcore) into `~/.cargo/bin`. It is the
simplest path when you already have a Rust toolchain, and the only path on
platforms without a prebuilt binary (see the platform table below).

## Prebuilt binaries

Every release attaches prebuilt binaries to
[GitHub Releases](https://github.com/eval-core/evalcore/releases), so you can
skip the Rust toolchain entirely. Download the archive for your platform,
extract the `evalcore` binary, and put it on your `PATH`:

```sh
# Example: download and unpack a release archive, then move the binary
tar -xzf evalcore-v0.5.0-x86_64-unknown-linux-gnu.tar.gz
mv evalcore /usr/local/bin/
evalcore --version
```

### Platform support

| Platform | Prebuilt binary | Release target triple |
|---|---|---|
| Linux x64 | Yes | `x86_64-unknown-linux-gnu` |
| macOS arm64 (Apple Silicon) | Yes | `aarch64-apple-darwin` |
| macOS x64 (Intel) | Yes | `x86_64-apple-darwin` |
| Windows | **Not yet supported** | install with `cargo install evalcore` (needs a Rust toolchain) |
| Linux arm64 / musl | Not yet | install with `cargo install evalcore` |

Windows and other platforms have no prebuilt binary today, but the crate builds
there — `cargo install evalcore` works anywhere a Rust toolchain does.

## Verify the install

```sh
evalcore --version
evalcore validate examples/quickstart/evals.yaml
```

`evalcore validate` parses and checks a config **without running anything** — no
targets are invoked, no scorers run, no network. It is the fastest way to
confirm both that the binary works and that a suite is well-formed:

```
OK: 1 target(s), 1 dataset(s), 1 scorer(s)
```

Now head to the [Quickstart](/evalcore/getting-started/quickstart/).

## Upgrading

How you upgrade depends on how you installed:

- **crates.io:** `cargo install evalcore --force` rebuilds the latest published
  version over the old one.
- **Prebuilt binary:** download the newer release archive and replace the
  binary on your `PATH` (re-run the extract/move step above).
- **GitHub Action:** bump the pinned tag (`eval-core/evalcore@v0.5.0`), or set
  the `version:` input to a specific release tag or `latest`.

EvalCore is pre-1.0: minor versions may change config or CLI surface. Check the
[CHANGELOG](https://github.com/eval-core/evalcore/blob/main/CHANGELOG.md) before
upgrading, and re-run `evalcore validate` on your suites afterward.

## The GitHub Action

In GitHub Actions, one step installs EvalCore (prebuilt binary, `cargo`
fallback), runs a suite, writes the report to the job step summary, and exits
with the suite's gate code — so the job passes or fails with your evals:

```yaml
- uses: eval-core/evalcore@v0.5.0
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
```

- `config` points at your `evals.yaml`.
- `args` are passed straight through to `evalcore run` — here, replay from the
  committed cassette (offline, keyless, `$0`) and gate against the `main`
  baseline.
- `version` (optional) pins the release to install; defaults to `latest`.

Pin the action to a released tag (`@v0.5.0`) so CI is reproducible. The
[Running in CI](/evalcore/guides/running-in-ci/) guide covers the full workflow.

## Choosing an install method in CI

| Where you run | Recommended install | Why |
|---|---|---|
| GitHub Actions | The `eval-core/evalcore@v0.5.0` Action | Installs the binary, adds a step summary, and carries the exit code for you. |
| GitLab CI / Jenkins / Buildkite | Download the release binary in a script | No Rust toolchain on the runner; fast, pinned. |
| Any runner with Rust | `cargo install evalcore --locked` | Works everywhere, including Windows, at the cost of a build. |
| Air-gapped / self-hosted | Vendor the binary into your image | Fully offline; no download at job time. |

A minimal non-GitHub install step looks like this:

```sh
# GitLab / Jenkins / Buildkite: fetch a pinned release binary
VERSION=v0.5.0
TARGET=x86_64-unknown-linux-gnu
curl -fsSL "https://github.com/eval-core/evalcore/releases/download/$VERSION/evalcore-$VERSION-$TARGET.tar.gz" \
  | tar -xz -C /usr/local/bin
evalcore --version
```

See [Running in CI](/evalcore/guides/running-in-ci/) for complete GitLab and
Jenkins pipelines.
