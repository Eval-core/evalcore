# EvalCore

**Snapshot testing for AI behavior.** A single-binary, config-first eval runner for LLM apps and agents — deterministic in CI, extensible from any language, with agent-trajectory evaluation over OpenTelemetry traces on the roadmap.

> Status: early scaffold (pre-0.1). See [PRD.md](PRD.md) for the full product plan.

## Install

```sh
cargo install evalcore                 # from crates.io
# or grab a prebuilt binary (Linux x64, macOS x64/arm64):
#   https://github.com/eval-core/evalcore/releases
```

In GitHub Actions, one step runs a suite and gates the job (report lands in the step summary):

```yaml
- uses: eval-core/evalcore@v0.3.0
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
```

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

## Retries, cost tracking, budgets

Transient failures (429, 5xx, network) retry automatically with exponential backoff, honoring `Retry-After`. Token usage is captured per case; declare your prices and EvalCore reports cost and enforces a budget:

```yaml
targets:
  openai:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    max_retries: 3            # default 2
    cost:                     # your provider's prices per 1M tokens
      input_per_1m: 0.40
      output_per_1m: 1.60
run:
  budget_usd: 5.0             # stop dispatching new cases past this spend
```

Cases skipped by the budget are reported as failures with a reason — the run completes and exits 1 rather than aborting. The terminal summary shows totals: `12 passed, 0 failed, 12 total · 48210 tokens · $0.0341`.

## Baselines: gate on regressions, not perfection

Real eval suites are rarely 100% green — what you actually want to block in CI is *getting worse*. Save an accepted state, then gate against it:

```sh
evalcore run evals.yaml --save-baseline main     # record the accepted state
evalcore run evals.yaml --baseline main          # exit 0 iff NO regressions
```

With `--baseline`, the exit contract changes: failures already present in the baseline are tolerated; a case that regresses (passed → failing) or a new failing case exits 1, with a diff:

```
baseline "main": 11/12 passed -> current: 10/12 passed
REGRESSED refund-2
     judge: answer no longer cites the policy
baseline gate: FAIL (1 regressed, 0 new failing)
```

Combine both flags for a rolling baseline (`--baseline main --save-baseline main`): compare first, then re-record. Baselines live in the same `.evalcore` store as the cache — commit it and CI gates offline.

## Agent trajectories: evaluate what the agent *did*

Agents aren't judged by their final answer alone. EvalCore ingests **recorded traces** — its own [native trajectory format](docs/trajectory-spec.md) or an OTel/OpenInference JSON export your framework already emits — and asserts on the run itself. No SDK, no integration, any language:

```yaml
targets:
  support-agent:
    type: trace                      # ingest, don't invoke
datasets:
  - file: cases.jsonl                # {"id": "refund-flow", "trace": "traces/run1.json"}
scorers:
  - type: trajectory
    rules:
      - must_call: search_kb
        with:
          query: { contains: "refund" }
      - must_not_call: issue_refund
        before: verify_identity      # never refund before verifying identity
      - max_steps: 8
```

```
PASS refund-flow-native (0ms)
PASS refund-flow-otel (4400ms)      # latency & tokens read from the trace itself

2 passed, 0 failed, 2 total · 268 tokens
```

Try it: `evalcore run examples/agent-trace/evals.yaml`. The rule semantics are specified in [docs/trajectory-spec.md](docs/trajectory-spec.md).

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
