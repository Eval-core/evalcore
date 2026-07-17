# EvalCore

**Snapshot testing for AI behavior.** A single-binary, config-first eval runner for LLM apps and agents — deterministic in CI, extensible from any language, with agent-trajectory evaluation over OpenTelemetry traces on the roadmap.

> Status: early scaffold (pre-0.1). See [PRD.md](PRD.md) for the full product plan.

## Quickstart

```sh
cargo run -p evalcore -- run examples/quickstart/evals.yaml
```

An eval suite is a YAML file plus a JSONL dataset:

```yaml
# evals.yaml
targets:
  echo:
    type: shell
    cmd: "cat"
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "refund"
```

Scorers range from deterministic checks (`contains`, `exact`, `regex`), through an any-language escape hatch (`subprocess`: JSON on stdin → `{"score": ...}` on stdout), to LLM-as-judge:

```yaml
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    rubric: "Is the answer grounded in the provided context?"
    api_key_env: OPENAI_API_KEY
    threshold: 0.7
```

Judge calls go through the record/replay cache too — replayed judge verdicts are deterministic, which is what makes LLM-graded suites usable as CI gates.

```jsonl
{"id": "refund-1", "input": "How do I get a refund for my order?"}
```

`evalcore run` exits `0` when every case passes and `1` otherwise, so it drops straight into CI.

## Record/replay caching

Every call to an LLM target is recorded in `.evalcore/cache.db` (SQLite), keyed by a content hash of the request. Reruns replay from the cache: **free, offline, deterministic**.

```sh
evalcore run evals.yaml                  # auto (default): replay hits, record misses
evalcore run evals.yaml --cache replay   # CI mode: cache only, a miss fails the case
evalcore run evals.yaml --cache live     # re-record everything
evalcore run evals.yaml --cache off      # bypass
```

Treat the cache file like VCR cassettes: commit it, and CI runs `--cache replay` with zero LLM spend and zero flakiness. Changing the model, URL, or a case's input changes the key, so stale hits don't lie to you. Shell targets are never cached — they run your local code, whose behavior can change without the config changing.

## Design principles

1. **Protocols over SDKs** — targets speak HTTP or shell, custom scorers speak JSON over stdin/stdout (any language), judges are any OpenAI-compatible endpoint. Rust is the engine, never a requirement.
2. **Deterministic in CI** — record/replay caching of every LLM call (shipped, see above).
3. **Traces as the unit of agent evaluation** — assert on tool calls, ordering, and budgets from OTel traces (roadmap, v0.2).
4. **Local-first** — results in SQLite next to your repo; no server, no signup.

## Development

```sh
cargo build                                          # build everything
cargo nextest run                                    # tests (or: cargo test)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Workspace layout is documented in [CLAUDE.md](CLAUDE.md).

## License

Apache-2.0
