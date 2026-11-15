# Changelog

All notable changes to EvalCore. Format loosely follows
[Keep a Changelog](https://keepachangelog.com); versions follow semver
(pre-1.0: minor bumps may break APIs and config).

## [Unreleased]

### Added
- **`anthropic` target** — the Anthropic Messages wire format as a second
  protocol, not a second SDK: `POST {url}/messages`, `x-api-key` auth with a
  pinned `anthropic-version` header, `system` as a top-level body field, and
  usage read from `input_tokens`/`output_tokens`. `url` defaults to
  `https://api.anthropic.com/v1`, so any Messages-shaped gateway that accepts an
  API key is a `url:` away; the AWS endpoints are SigV4-signed and need
  `auth: sigv4` (below). `max_tokens` is a first-class
  field (default 4096) because the API rejects a request without it, and it is
  part of the cache key — a lower cap truncates the answer. Responses are
  parsed as an array of typed content blocks, so a leading `thinking` block no
  longer breaks extraction, and a policy decline (HTTP 200 with
  `stop_reason: "refusal"`) becomes a failed case naming the refusal rather
  than a parse error. Retries, timeouts, secret resolution, and record/replay
  are the same shared implementation every HTTP target uses.
- **`provider:` on the `judge` scorer** — `openai` (default) or `anthropic`,
  so an LLM judge can run on Claude. The default keeps every existing suite
  parsing and cache-keying identically; the `type` discriminator puts the two
  wire formats in disjoint keyspaces, so no recorded cassette was invalidated.
- **`gemini` target** — the third wire format:
  `POST {url}/models/{model}:generateContent`, with the model in the URL path,
  `contents[].parts[]` in place of `messages[]`, a `systemInstruction` Content
  value, and generation tunables nested under `generationConfig` (so `params`
  nests too — the structure passes through rather than being translated). The
  API key rides in the `x-goog-api-key` **header**, never the `?key=` query
  parameter the API also accepts: request URLs are quoted in error messages and
  reports, so a query-string secret would leak into them. `thought` parts are
  skipped, and both safety outcomes — a blocked prompt and a filtered candidate,
  each an HTTP 200 — become failed cases with readable reasons. Google's
  OpenAI-compatible endpoint still needs no code at all; prefer it unless you
  need a native-only feature.
- **`auth: sigv4` on `anthropic` targets** — AWS Signature Version 4 signing,
  for Claude on AWS. Credentials are **names of environment variables**
  (`region`, `access_key_env`, `secret_key_env`, `session_token_env`,
  `service`), the same secret model as `api_key_env`; EvalCore deliberately does
  not implement the AWS credential chain, whose profile/IMDS lookups would put a
  network round-trip and a clock read behind every target build. Signing is
  hand-rolled over `sha2` + `hmac` rather than pulling `aws-config`, and is a
  pure function of its inputs plus an explicit timestamp — the clock is read at
  exactly one call site, and the timestamp never reaches a cache identity.
  Missing credentials fail at build time naming the variable; replay-only runs
  build without any.
- **`dialect:` on `anthropic` targets** — `messages` (default) or
  `bedrock-invoke`, which moves the model into the path and swaps the body's
  `model` for `anthropic_version`. Independent of `auth`: SigV4 signs a plain
  Messages request against Bedrock Mantle exactly as it signs a legacy
  `bedrock-runtime` InvokeModel one. The dialect is part of the cache key, since
  it changes both path and body.
- **`provider: gemini`** on the `judge` scorer, alongside `openai` and
  `anthropic`.
- **`examples/anthropic/`, `examples/gemini/`, `examples/bedrock/`** — live
  Claude and Gemini calls, and the three AWS shapes side by side.
- **Output diff on baseline regressions** — a `REGRESSED` case whose baseline
  and current runs both recorded output now shows how the output moved: an
  order-aware word similarity and the changed runs, e.g.
  `output vs baseline: 89% similar: -2.1 +2.2`, in the terminal and HTML
  baseline sections (`CaseRegression::output_change`). Pure and deterministic;
  gate semantics are unchanged. Word diff via `llm-diff`.

### Changed
- **Three wire formats now share one implementation.** `ChatWireFormat` and
  `ChatTarget<F>` in `evalcore-core::chat` hold the retry loop, client
  construction, transient/permanent classification, body-read timeout, and
  latency measurement that were byte-identical in all three copies; each format
  supplies only its endpoint, body, auth header, response parsing, and cache
  identity. `OpenAiCompatTarget`, `AnthropicTarget`, and `GeminiTarget` are type
  aliases, so every call site and the public API are unchanged — and every
  cache-identity shape-pinning test still passes byte-for-byte, so no cassette
  moved.

### Fixed
- The `anthropic` target's documented AWS story was wrong: Bedrock Mantle and
  Claude Platform on AWS are SigV4-signed, not `x-api-key` endpoints, so the
  "set a `url` and you're done" guidance could not have worked. Both are now
  reachable via `auth: sigv4`. Claude Platform on AWS additionally requires a
  workspace identifier this target does not yet send, and is documented as
  unsupported rather than implied to work.

## [0.7.5] — 2026-07-19

### Changed
- **Premium terminal output.** On an interactive terminal, `evalcore run` now
  renders restrained semantic color (green pass, red fail, yellow flaky, dim
  secondary figures), a prominent closing `PASSED`/`FAILED` verdict, and a
  live spinner + `k/N` case counter on stderr. The status words are always
  present, so nothing depends on color. Piped, redirected, in CI, or under
  `NO_COLOR`/`TERM=dumb`, the output is exactly the previous plain text —
  byte-for-byte, so snapshots, grep, and pull-request pastes are unaffected.
  New flags tune it: `--color auto|always|never`, `--progress auto|never`, and
  `-q/--quiet` (failures only). Machine reporters (`json`/`junit`) and
  `--output` files are never colored, and the verdict rides stderr there so
  their stdout stays pure. `--color` honors `NO_COLOR`, `TERM=dumb`, and
  `CLICOLOR_FORCE`.

### Security
- **Terminal-output sanitization.** All user-derived text in the terminal
  reporters (case ids, scorer reasons, target errors, gate/class labels) is
  neutralized against ANSI/control-sequence injection — ESC, C0/C1/DEL,
  CR/LF/TAB, and Unicode bidirectional (Trojan-Source) controls render as
  visible escapes — so a hostile model output or dataset can't move the
  cursor, recolor the report, or reorder a line. Benign text is unchanged;
  JSON/JUnit/HTML keep their existing escaping.

## [0.7.0] — 2026-07-18

### Added
- **Trials — statistical evals** (`run.trials`): run every case N times and
  aggregate. `trials: 3` shorthand or `trials: {count: 5, require: majority}`
  (`all` | `majority` | `any`; default `all`). A trial passes when every scorer
  passes; the case verdict follows `require`; the per-scorer case score is the
  mean across trials (what gates and baselines see) and case latency is the
  trial mean, while cost sums every trial and counts toward `budget_usd`.
  Replays stay deterministic: trial 0 keeps the exact pre-trials cache key, so
  every existing cassette still replays; trials 1..N re-key with a `trial`
  index — and judge calls re-key per trial the same way. The terminal report
  tags multi-trial cases with `[k/N trials]` (single-trial output is
  byte-identical to before); JSON and HTML carry the full per-trial detail.
- **`json-schema` scorer**: `{type: json-schema, schema: path.json}` validates
  the output as JSON against a draft 2020-12 schema (path relative to the
  config file). Failing reasons name up to three violations by JSON pointer,
  deterministically ordered. Remote `$ref` resolution is compiled out —
  validation never touches the network.
- **`similarity` scorer**: `{type: similarity, url, model, api_key_env,
  threshold}` grades semantic closeness between the output and the case's
  `expected` via any OpenAI-compatible `/embeddings` endpoint — score is the
  raw cosine similarity, passing at `threshold` (default 0.8). Embedding calls
  ride the record/replay cache like judge calls (with an identity that can
  never collide with a chat target), so replayed similarity scores are free,
  offline, and deterministic.
- **Classification aggregates** (`run.classification: true`): treat each
  labeled case's `expected` as the true class and its output as the prediction,
  then report accuracy and macro-averaged F1 over the labeled cases, plus a
  per-class precision/recall/F1/support table. A case is labeled iff it has
  `expected`; the label is the trimmed `expected`, the prediction the trimmed
  output, matched **case-sensitively** (normalize in your target or a scorer —
  v1 trims and nothing more). The class set is the observed expected labels
  only, so a hallucinated prediction is a false negative for its true class,
  never a class of its own; every `0/0` guard yields `0.0`. A target-error case
  with `expected` counts as labeled-and-wrong, so an error storm sinks accuracy.
  New `accuracy {min}` and `macro_f1 {min}` gates read these aggregates (and
  turn them on implicitly) — a run with **zero labeled cases** scores `0.0` and
  fails such a gate loudly with a `no labeled cases` reason rather than passing
  vacuously. The terminal prints one line after the gates block (`classification:
  accuracy 0.67 · macro-F1 0.67 (3 labeled, 1 unlabeled)`); the per-class table
  rides the JSON and HTML reports. Absent (the default), `RunSummary` serializes
  byte-identically to before.
- **Trials statistics presentation**: reporters now surface flakiness from the
  per-trial detail (no engine change). The terminal summary line gains a
  ` · N flaky` suffix — a case is flaky when its trials split (`0 < passed <
  count`), e.g. `1 passed, 1 failed, 2 total · 0 flaky`. The JSON report adds a
  per-case `trial_stats` object (`pass_fraction`, per-scorer `score_mean`, and
  per-scorer `score_range` as `[min, max]`, keys sorted), and the HTML per-case
  Trials section shows `k/N passed (pass fraction 0.67)`. All three are emitted
  only when a case carries trials detail, so a single-trial run's terminal, JSON,
  and HTML output stays byte-identical.
- **Matrix runs — side-by-side model/prompt comparison** (`run.matrix` /
  `--matrix`): run the whole suite once per named target and print a comparison,
  so "model A vs B" or "prompt v1 vs v2" (two configured targets) is one
  invocation. `run.matrix: [gpt, claude]` in the config, or `--matrix gpt,claude`
  on the CLI (a comma list that overrides `run.matrix`); at least two distinct
  names, each defined in `targets`. Arms run **sequentially, in the list order
  you wrote** (not map order) — deterministic and predictable against rate
  limits; within an arm, concurrency, trials, gates, and classification behave
  exactly as a single run does today. **Each arm prices with its own target's
  `cost:` rates**, closing the one-rate-per-run gap; `run.budget_usd` applies
  **per arm** (each arm gets the full budget). **Exit code is 0 iff every arm
  satisfies today's whole contract** (all cases pass *and* every gate holds),
  else 1. The terminal report prints each arm's existing block under a
  `== target: <name>` header, then a `== comparison` table with a per-case
  winner/tie column and a `wins: …` footer; the **per-case winner is the arm
  with the strictly highest mean case score** (mean of that case's scorer
  values), with a `1e-9` tie tolerance — cases where the top arms tie, or where
  no arm produced a score, are ties, for any number of arms. JSON (`--reporter
  json`), HTML (`--html`), and JUnit reporters all gain matrix shapes; a
  `--matrix` run over two shell targets renders:

  ```
  == comparison
  case        echo    upper
  refund-1    PASS    PASS     tie
  refund-2    FAIL    PASS     upper
  wins: echo 0 · upper 1 · ties 1
  ```

  Combining `--matrix` with `--target`, `--baseline`, or `--save-baseline` is a
  hard error (a matrix already runs several targets; baselines are per-run in v1
  — run targets separately with `--target` to baseline them). A cassette store
  is shared across arms (each arm has its own cache identity; judge/embedding
  calls grade each arm's distinct output, so `--cache replay` works per arm).
  **Non-matrix runs are byte-identical everywhere** — a config with no `run.matrix`
  and no `--matrix` takes the unchanged single-target path.

- **Run history — local persistence of past runs** (`run.history`): every
  `evalcore run` appends one row per executed run to the `.evalcore/cache.db`
  store (a matrix records **one row per arm**, keyed by the arm's target name),
  capturing the full `RunSummary` with gates and classification attached — those
  are what the viewer shows. On by default; `run.history: false` in the config or
  `--no-history` on the CLI opts out. The row is written after gates and
  classification are attached and before reporting, and a write failure is a
  warning on stderr, **never a run failure** — the eval verdict never depends on
  history I/O. Additive schema (a new `run_history` table beside the cache and
  baseline tables, so pre-existing stores open unchanged); the exit code, report
  bytes, and cache keys are byte-identical whether history is on or off.
- **`evalcore serve` — a local, read-only run-history viewer**
  (`evalcore serve [--store <path>] [--port <u16>]`): browse past runs in a
  browser. It binds `127.0.0.1` only and prints `serving http://127.0.0.1:7878` —
  **localhost is the entire security model**: read-only, GET-only (any other
  method is a 405), no auth because there is no remote access, no telemetry,
  nothing leaves the machine. Three pages: `/` lists every run newest-first (id,
  time, config, target, passed/failed/total, cost) with an inline pass-rate
  sparkline and a "diff vs the previous same-target run" link; `/run/{id}` is
  byte-for-byte that run's `--html` report; `/diff?a=<id>&b=<id>` compares **any
  two stored runs** with the matrix comparison renderer (model A's run vs model
  B's, or yesterday vs today). Store defaults to `.evalcore/cache.db`, port to
  `7878`; every page is self-contained (inline CSS, no external requests, no JS)
  and escapes every stored string. New `evalcore-serve` crate — nothing depends
  on it but the binary.

### Changed
- **Duplicate case ids are now rejected at dataset load**, naming both lines:
  `duplicate case id "x" at <file>:<line> (first used at line <n>)`. Case ids key
  baseline matching and matrix comparison rows, so a repeated id would silently
  collapse two cases into one row; failing fast at load turns a silent data loss
  into a clear error. Ids that default to `case-<line number>` (cases with no
  explicit `id`) are still unique by construction.

### Known gaps
- With `run.trials` > 1, the terminal/HTML token totals reflect one trial per
  case (cost totals are correct — they sum every trial). Tracked for a
  follow-up alongside judge-call cost attribution.

## [0.6.0] — 2026-07-18

### Added
- **Per-case `context` for RAG evaluation**: a dataset case may carry retrieved
  `context` — a single string or an array of strings (an empty array normalizes
  to none). Context lives on the scoring side only: targets never receive it (a
  RAG app does its own retrieval, and cache keys still hash just identity +
  `input`), while scorers do. The `judge` scorer injects the context as a
  clearly delimited, numbered section placed before the answer — so rubrics like
  "grounded in the provided context?" have something to grade against — and,
  because the context is part of the judge prompt, changing it re-records the
  verdict just like a rubric change; contextless judge prompts stay
  byte-identical, so existing cassettes keep replaying. The `subprocess` scorer
  gains a `context` array in its stdin payload, present only when the case
  carries context. The HTML report renders a numbered, escaped Context block in
  each case's expandable details; the JSON report carries it too, while the
  terminal and JUnit reporters are unchanged.
- **Ragas/DeepEval shims** (`shims/` in the repo): ready-made subprocess
  scorers wrapping Ragas and DeepEval faithfulness/context-recall metrics —
  stdlib-only at import, lazy library imports, and an offline `--check`
  self-test. These metrics call an LLM themselves, so they belong on a nightly
  tier; the cached native `judge` remains the PR path.

## [0.5.0] — 2026-07-17

### Added
- **Self-contained HTML report** (`--html <path>`): writes a single, shareable
  HTML document — the artifact a reviewer clicks in a PR — alongside (never
  replacing) the primary `--reporter` output. Header counts, tokens, and cost;
  a gates panel; one expandable row per case revealing the output, per-scorer
  scores, and the agent trajectory (tool calls, inputs/outputs); and the
  baseline diff when `--baseline` is set. Entirely inline (no external requests,
  no JavaScript — `<details>` drives expansion), works air-gapped from
  `file://`, light/dark themed, deterministic byte-for-byte, and every
  user-derived value is HTML-escaped. The GitHub Action gains an `html-artifact`
  input (default `evalcore-report`) that uploads the report as a build artifact
  — even on failure — and notes it in the job step summary; set it to `""` to
  disable.
- **Suite-level aggregate gates**: `run.gates` declares absolute floors over a
  whole run as CI acceptance criteria — `pass_rate` (fraction of cases passing
  every scorer, in `[0,1]`) and `mean_score` (mean of scorer values, optionally
  restricted to one `scorer`). Gates are additive to the existing contract: a
  run exits `1` if any case fails (or, with `--baseline`, regresses) *or* any
  gate falls below its floor — so an accepted baseline failure stays tolerated
  per-case yet still sinks a `pass_rate` gate it drops below. Target-error cases
  count in `pass_rate`'s denominator but add no scores to `mean_score` (pair the
  two to catch error storms). Outcomes print after the summary (`GATE PASS
  pass_rate >= 0.95 (actual 1.00)`) and ride along in the JSON report; JUnit is
  unchanged (the exit code carries the gate result).
- **Final-answer extraction from agent traces**: a `trace` target now grades
  the agent's actual answer, not the trajectory JSON. The native format gains an
  optional top-level `final_output` string, and OTel exports extract the final
  answer from the root span (OpenInference `output.value`, else OTel GenAI
  `gen_ai.completion`). When present, it becomes the target's text output — so
  `judge`, `contains`, `regex`, and `exact` grade the answer — while the
  `trajectory` scorer always asserts on the steps: the answer and the path
  graded on the same case. Traces without a final answer keep emitting the
  trajectory JSON as text, so existing suites are unaffected. `TargetOutput`
  gains an optional structured `trajectory` channel; LLM cassettes (where it is
  always absent) keep their exact recorded bytes.
- **`http` target type**: evaluate an arbitrary HTTP/JSON endpoint — typically
  your own deployed app's REST API (`POST /chat {"question": ...}`) — through
  the record/replay cache, exactly like an LLM target. `{{input}}` is
  percent-encoded into `url` and substituted verbatim into every string value
  of a JSON `body` template; `response_path` (RFC 6901 JSON Pointer) pulls the
  answer out of a 2xx JSON response, or the raw body text is used when omitted.
  Supports GET/POST/PUT/PATCH, static `headers`, and env-var auth
  (`api_key_env` + `auth_header`/`auth_prefix`, defaulting to
  `authorization: Bearer <key>`). Transient failures (429/5xx/transport) retry
  with the same deterministic backoff as `openai-compatible`. The cache
  identity keys on the request shape only, never on secrets, so `--cache
  replay` runs offline with no key configured. No cost/token accounting for
  `http` targets in v1 (generic APIs have no standard usage shape).
- **Per-attempt request timeout** (`timeout_seconds`, default 120) on
  `openai-compatible` and `http` targets: bounds the total time of each attempt
  (connect + reading the response body) so a hung endpoint can no longer pin a
  concurrency slot and wedge a run. Each retry gets a fresh budget; a timeout is
  a transient failure, retried like a 429/5xx. Excluded from the cache identity,
  so cassettes recorded before this change keep their keys. Judge calls inherit
  the 120s default (no judge-level knob in v1).

## [0.4.0] — 2026-07-17

### Added
- **System prompts and provider params** on `openai-compatible` targets:
  `system:` prepends a system message; `params:` passes arbitrary
  request-body fields through verbatim (`temperature`, `max_tokens`, …).
  `model`, `messages`, and `stream` are reserved and rejected at validation.
- **Cost rates on `trace` targets**: token usage extracted from trace spans
  is priced (`cost:` block), so trace runs report `$` totals and respect
  `run.budget_usd`.

### Changed
- `system`/`params` join the record/replay cache identity — changing either
  re-records instead of replaying stale answers. Unset fields are omitted
  from the identity, so cassettes recorded before 0.4.0 keep their keys.

### Known gaps
- LLM-judge calls are not yet included in cost totals or budgets.

## [0.3.0] — 2026-07-17

### Added
- **Agent trajectory evaluation**: `type: trace` targets ingest recorded
  traces — EvalCore's native trajectory JSON or OTel JSON exports (OTel
  GenAI + OpenInference conventions) — extracting steps, token usage, and
  latency from spans. `type: trajectory` scorer with `must_call` (argument
  matchers, `after:`), `must_not_call` (`before:`), and `max_steps` rules.
  Spec: `docs/trajectory-spec.md`.
- **Prebuilt release binaries** (Linux x64, macOS x64/arm64) attached to
  GitHub Releases by a tag-triggered workflow.
- **GitHub Action** (`uses: eval-core/evalcore@<tag>`): installs the release
  binary (cargo fallback), runs a suite, writes the report to the job step
  summary, exits with the gate's code.

### Fixed
- Shell targets no longer fail with EPIPE when the command exits without
  reading stdin (Linux race).

## [0.2.0] — 2026-07-17

### Added
- **LLM-as-judge scorer** (`type: judge`): rubric-based grading via any
  OpenAI-compatible endpoint, with code-fence-tolerant verdict parsing and a
  configurable pass threshold.
- **Record/replay cache** (SQLite): every cacheable LLM call is content-
  addressed by canonical request hash. Modes `auto` / `replay` / `live` /
  `off`; `--cache replay` runs offline, deterministically, and **without API
  keys**. Judge calls are cached too.
- **Retries**: transient failures (429/5xx/transport) retry with
  deterministic exponential backoff honoring `Retry-After`.
- **Token usage + cost accounting**: provider-reported usage per case;
  user-declared `cost:` rates; `run.budget_usd` stops dispatching new cases
  when exhausted (skipped cases fail with a reason).
- **Baseline regression gating**: `--save-baseline <label>` records the
  accepted state; `--baseline <label>` flips the exit contract to "no
  regressions" (accepted failures tolerated; regressed and new-failing
  cases exit 1). Both flags together give rolling baselines.

## [0.1.0] — 2026-07-17

Initial release: config-first eval runner. `evals.yaml` suites; shell and
OpenAI-compatible targets; JSONL datasets; `contains`/`exact`/`regex`
scorers plus the any-language subprocess protocol; terminal/JSON/JUnit
reporters; concurrent engine with dataset-order results; exit-code contract
(0 = all passed, 1 = anything else).
