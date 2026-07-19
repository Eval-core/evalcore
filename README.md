<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/evalcore-lockup-dark.png">
    <img src="assets/evalcore-lockup.png" alt="EvalCore" width="380">
  </picture>
</p>

**Know when your AI gets worse — before your users do.** Change a prompt, swap a model, update a dependency — did anything break? EvalCore records how your AI behaves and checks every change against it: free, offline, no flaky tests. One small tool, any language.

Under the hood it's **the eval engine for AI systems**: statistical trials, side-by-side model comparison, agent-trajectory evaluation over OpenTelemetry traces, a local run viewer, and record/replay caching that makes CI runs deterministic, offline, and $0 — snapshot testing for AI behavior.

**[Documentation](https://eval-core.github.io/evalcore/)** · [Quickstart](https://eval-core.github.io/evalcore/getting-started/quickstart/) · [crates.io](https://crates.io/crates/evalcore) · [Releases](https://github.com/eval-core/evalcore/releases)

> Status: pre-1.0 — config and APIs may still shift between minor versions. See [CHANGELOG.md](CHANGELOG.md).

## Install

```sh
cargo install evalcore                 # from crates.io
# or grab a prebuilt binary (Linux x64, macOS x64/arm64):
#   https://github.com/eval-core/evalcore/releases
```

In GitHub Actions, one step runs a suite and gates the job (report lands in the step summary):

```yaml
- uses: eval-core/evalcore@v0.7.0
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
    html-artifact: evalcore-report   # upload a shareable HTML report (default; "" disables)
```

The `html-artifact` input uploads a self-contained HTML report as a build artifact a reviewer can click straight from the PR — the pass/fail summary, gate outcomes, and every case's output, per-scorer scores, and agent trajectory, expandable inline. It uploads even when the suite fails (that's when it matters most), and embeds the baseline diff when `--baseline` is set. Locally or in any pipeline, `evalcore run … --html report.html` writes the same document alongside (never replacing) the primary `--reporter` output.

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

Scorers range from deterministic checks (`contains`, `exact`, `regex`, and `json-schema` for structured output), through an any-language escape hatch (`subprocess`: JSON on stdin → `{"score": ...}` on stdout), to LLM-backed grading — LLM-as-judge and `similarity` (semantic closeness by embedding cosine):

```yaml
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    rubric: "Is the answer grounded in the provided context?"
    api_key_env: OPENAI_API_KEY
    threshold: 0.7
```

Judge calls go through the record/replay cache too — replayed judge verdicts are deterministic, which is what makes LLM-graded suites usable as CI gates. Embedding calls (`similarity`) cache and replay just like the judge.

```jsonl
{"id": "refund-1", "input": "How do I get a refund for my order?"}
{"id": "rag-1", "input": "How long do refunds take?", "context": "Refunds are processed within 30 days."}
{"id": "rag-2", "input": "What do I need for a refund?", "context": ["Refunds require an order number.", "Keep your original receipt."]}
```

For RAG evaluation, a case may carry retrieved `context` — a single string or an array of strings. **Scorers see the context but targets never do:** a RAG app runs its own retrieval (put anything the target needs in `input`), so context stays on the scoring side. The judge grades the answer against the context (write rubrics like "grounded in the provided context?"), and subprocess scorers receive it as a `context` array on stdin. Because the context is part of the judge's prompt, changing it re-records the judge verdict, just like changing the rubric.

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

## Trials: measure, don't sample

An LLM is stochastic — one run samples its behavior once, so a green suite might just be a lucky roll. `run.trials` runs every case N times and aggregates, so you can gate on how *often* a case passes:

```yaml
run:
  trials: 3            # shorthand; or { count: 5, require: majority }  —  all | majority | any
```

A trial passes when every scorer passes; the case verdict follows `require` (default `all`; `majority` is strictly more than half). The per-scorer case score is the mean across trials (what `mean_score` gates and baselines see), latency is the trial mean, and every trial's cost counts toward `budget_usd`. Determinism holds: trial 0 keeps the pre-trials cache key so existing cassettes replay, while trials 1..N — and their judge/similarity calls — re-key per trial. The terminal report tags multi-trial cases, `PASS greeting (6ms) [3/3 trials]`; single-trial output is unchanged.

## Evaluate your deployed app's own REST endpoint

The `http` target points EvalCore at any HTTP/JSON API — typically your own RAG service or agent behind `POST /chat` — and caches it exactly like an LLM call, so the same commit-the-cassette, replay-in-CI story applies to your app's real responses:

```yaml
targets:
  my-rag:
    type: http
    url: https://api.myapp.com/chat   # {{input}} allowed here too (percent-encoded)
    method: POST                       # default POST; GET/PUT/PATCH also supported
    headers:                           # static headers — NEVER secrets (values are cached)
      x-tenant: acme
    api_key_env: MYAPP_API_KEY         # optional; sent as `authorization: Bearer <key>`
    body:                              # JSON template; {{input}} fills string values
      question: "{{input}}"
      session: eval
    response_path: /answer             # RFC 6901 JSON Pointer; omit to use the raw body
```

`{{input}}` is substituted from each case: percent-encoded into `url`, verbatim into every string value of `body`. On a 2xx, `response_path` pulls the answer out of the JSON response (omit it to score the raw body text); non-2xx and transient failures are classified and retried just like the LLM target. **Keep credentials in `api_key_env`, never in `headers:`** — header values are hashed into the cache identity and stored in the committed `.evalcore/cache.db`, whereas the API key never enters the cache. The key is sent as `authorization: Bearer <key>` by default; for an `x-api-key` style header set both `auth_header: x-api-key` and `auth_prefix: ""`. The cache identity keys on the request shape (url/method/headers/body/response_path), never on the key, so `--cache replay` runs offline with no secret configured.

## Retries, timeouts, cost tracking, budgets

Transient failures (429, 5xx, network) retry automatically with exponential backoff, honoring `Retry-After`. Each attempt is also bounded by `timeout_seconds` (default 120, applied per attempt so every retry gets a fresh budget): when it elapses the attempt is aborted and treated as a transient failure — retried like any 429/5xx — so a hung endpoint can no longer pin a concurrency slot and wedge a run. Token usage is captured per case; declare your prices and EvalCore reports cost and enforces a budget:

```yaml
targets:
  openai:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    max_retries: 3            # default 2
    timeout_seconds: 60      # default 120; per attempt
    cost:                     # your provider's prices per 1M tokens
      input_per_1m: 0.40
      output_per_1m: 1.60
    system: "You are a support agent. Be concise."
    params:                   # passed through verbatim — any provider knob
      temperature: 0
      max_tokens: 512
run:
  budget_usd: 5.0             # stop dispatching new cases past this spend
```

Cases skipped by the budget are reported as failures with a reason — the run completes and exits 1 rather than aborting. The terminal summary shows totals: `12 passed, 0 failed, 12 total · 48210 tokens · $0.0341`.

How the math works: `cost = (input_tokens × input_per_1m + output_tokens × output_per_1m) / 1M`, using the token counts the provider reported (or, for trace runs, the usage found in the spans). EvalCore deliberately ships **no pricing table** — prices change and differ per provider, deployment, and tier, and a stale table produces silently wrong dollars. Your rates live in config where code review can see them. Replayed runs report the *recorded* usage, so cost stays visible even when actual spend is $0. Known gap: LLM-judge calls are not yet included in totals or budgets.

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

## Suite gates: floors, not per-case checks

Baselines and per-case scorers ask "did any single case fail?" Enterprises also want a floor over the *whole* run — "at least 95% of cases pass", "the judge's mean score is at least 0.8". Declare aggregate gates under `run`:

```yaml
run:
  concurrency: 4
  gates:
    - type: pass_rate
      min: 0.95                # fraction of cases passing all scorers, in [0,1]
    - type: mean_score
      scorer: judge            # optional: restrict to that scorer's score; omitted = all scores
      min: 0.8                 # any finite number (subprocess scorers may use arbitrary scales)
```

Floors compare with a `1e-9` tolerance to absorb floating-point rounding, so a run that exactly meets its floor passes. Gates are *additive absolute floors*: the run exits `1` if the existing contract fails (any case failed, or with `--baseline` a regression) **or** any gate falls below its floor — so with `--baseline`, an accepted failure stays tolerated per-case, yet still sinks a `pass_rate` gate it drops below. Target-error cases count in `pass_rate`'s denominator but contribute no scores to `mean_score`, so pair a `mean_score` gate with a `pass_rate` gate to catch error storms. Gate outcomes print after the summary (`GATE PASS pass_rate >= 0.95 (actual 1.00)`) and ride along in the JSON report; JUnit is unchanged — the exit code carries the gate result for CI. For label-prediction suites, `run.classification` adds `accuracy` and `macro_f1` gates (each a `min` in `[0,1]`) that gate on the metrics over cases carrying an `expected` label, printing `classification: accuracy 0.67 · macro-F1 0.67 (3 labeled, 1 unlabeled)`.

## Matrix runs: compare models side by side

Comparing two models — or two prompts, or two deployed endpoints — used to mean running the suite twice and diffing reports by eye. A matrix runs the whole suite once per target in a single invocation and prints a side-by-side comparison. The two things you compare are just two targets (different `model`s, or the same model with different `system` prompts):

```yaml
run:
  matrix: [gpt, claude]   # >= 2 distinct, defined targets; or --matrix gpt,claude on the CLI
```

```
== comparison
case        echo    upper
refund-1    PASS    PASS     tie
refund-2    FAIL    PASS     upper
wins: echo 0 · upper 1 · ties 1
```

Arms run sequentially in list order; each arm prices with its own target's `cost` rates and gets the full `budget_usd` (the per-run cost-rates gap, closed). The per-case winner is the arm with the strictly highest mean score (`1e-9` tie tolerance); an all-tie or score-less case is a tie. The run exits `0` iff **every** arm passes all its cases and gates. Combining `--matrix` with `--target`, `--baseline`, or `--save-baseline` is a hard error — baselines are per-run in v1. One cassette store serves every arm (each arm has its own cache identity), so `--cache replay` reruns the whole matrix offline.

## Run history and a local viewer

Every run is recorded to your local store, and `evalcore serve` opens a read-only web viewer over that history — so "what did last night's run say?", the pass-rate trend, and "model A's run vs model B's" are all one click away.

```sh
evalcore run evals.yaml --matrix gpt,claude   # records a history row per arm (on by default; --no-history opts out)
evalcore serve                                # serving http://127.0.0.1:7878
```

You get a listing (newest first, with a pass-rate sparkline), each run's full HTML report at `/run/{id}`, and a `/diff?a=&b=` that compares **any** two stored runs — yesterday vs today, or one target vs another. It is **local-first and read-only by design**: binds `127.0.0.1` only, GET-only, no auth (no remote access), no telemetry — nothing leaves your machine. History lives in the same `.evalcore/cache.db` as the cache, so committing it shares runs with teammates.

## Agent trajectories: evaluate what the agent *did*

Agents aren't judged by their final answer alone — but the answer still matters. EvalCore ingests **recorded traces** — its own [native trajectory format](docs/trajectory-spec.md) or an OTel/OpenInference JSON export your framework already emits — and grades **the answer and the path in one suite**. No SDK, no integration, any language:

```yaml
targets:
  support-agent:
    type: trace                      # ingest, don't invoke
datasets:
  - file: cases.jsonl                # {"id": "refund-flow", "trace": "traces/run1.json"}
scorers:
  - type: contains                   # grade the ANSWER: the trace's final
    value: "30 days"                 # output (native final_output / OTel root
                                     # span), not the trajectory JSON. Use a
                                     # judge here for graded rubric scoring.
  - type: trajectory                 # grade the PATH: what the agent did
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

2 passed, 0 failed, 2 total · 268 tokens · $0.0002
```

Try it: `evalcore run examples/agent-trace/evals.yaml`. The rule semantics are specified in [docs/trajectory-spec.md](docs/trajectory-spec.md).

## Design principles

1. **Protocols over SDKs** — targets speak HTTP or shell, custom scorers speak JSON over stdin/stdout (any language), judges are any OpenAI-compatible endpoint. Rust is the engine, never a requirement.
2. **Deterministic in CI** — record/replay caching of every LLM call (shipped, see above).
3. **Traces as the unit of agent evaluation** — assert on tool calls, ordering, and budgets from OTel traces (shipped, see above).
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
