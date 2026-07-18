# Changelog

All notable changes to EvalCore. Format loosely follows
[Keep a Changelog](https://keepachangelog.com); versions follow semver
(pre-1.0: minor bumps may break APIs and config).

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
