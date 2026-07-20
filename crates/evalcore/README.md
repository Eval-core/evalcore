# evalcore

**[EvalCore](https://evalcore.cc)** is snapshot testing for AI behavior: a
single-binary, config-first eval runner for LLM apps and agents. This crate is the CLI.

```sh
cargo install evalcore
```

Point it at a suite and run:

```sh
evalcore run evals.yaml
```

The process exits 0 when every case passes and 1 otherwise, so CI can gate on it
directly. Record LLM responses once with `--cache auto`, commit the cassette, and
`--cache replay` reruns the suite offline, deterministically, with no API keys.

- [Quickstart](https://evalcore.cc/getting-started/quickstart/)
- [CLI reference](https://evalcore.cc/reference/cli/)
- [Repository](https://github.com/eval-core/evalcore)

Prebuilt binaries for Linux and macOS are on the
[releases page](https://github.com/eval-core/evalcore/releases), and a GitHub Action
(`uses: eval-core/evalcore@v0.7.5`) wraps the same binary for CI.
