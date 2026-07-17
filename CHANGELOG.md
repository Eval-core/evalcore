# Changelog

All notable changes to EvalCore. Format loosely follows
[Keep a Changelog](https://keepachangelog.com); versions follow semver
(pre-1.0: minor bumps may break APIs and config).

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
