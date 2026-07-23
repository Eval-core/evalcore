# Repository map

The single index for EvalCore. Every markdown doc in the public repo is listed here with a
one-line purpose, so you can find anything in one hop instead of searching. Agents and humans
both start here.

## Start here

| You are | Read first |
|---|---|
| An agent working in this repo | [`CLAUDE.md`](CLAUDE.md): operating rules, then this map |
| A contributor | [`CONTRIBUTING.md`](CONTRIBUTING.md) + [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| A user of the tool | [evalcore.cc](https://evalcore.cc): the docs site (source in `site/`) |
| Styling any surface | [`design/README.md`](design/README.md): the design system |

**Canonical sources** (edit these first; everything else points at them): architecture →
`docs/ARCHITECTURE.md` · design → `design/README.md` · agent rules → `CLAUDE.md` · end-user docs
→ evalcore.cc (`site/`). The private knowledge base lives outside this repo: see the last section.

## Root and governance

| File | Purpose |
|---|---|
| [`README.md`](README.md) | Public front door: what EvalCore is, install, offline example, feature matrix, GitHub Action, license. |
| [`CLAUDE.md`](CLAUDE.md) | Agent operating manual: product framing, privacy rules, build commands, crate map, architecture rules, style. |
| [`AGENTS.md`](AGENTS.md) | Stub for non-Claude tools; defers to `CLAUDE.md`. |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Contributor guide: setup, the four CI checks, workspace map, architecture rules, testing, commit/PR conventions. |
| [`CHANGELOG.md`](CHANGELOG.md) | Keep-a-Changelog release history documenting every user-visible change, feature, and security fix. |
| [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) | Contributor Covenant v2.1 with maintainer enforcement contacts. |
| [`SECURITY.md`](SECURITY.md) | Security policy: supported versions, private reporting, threat scope, the "evals.yaml is executable" warning. |
| [`.github/pull_request_template.md`](.github/pull_request_template.md) | PR checklist: the four CI checks plus change-scope checkboxes. |

## Agent and tooling config

| File | Purpose |
|---|---|
| [`.claude/agents/rust-reviewer.md`](.claude/agents/rust-reviewer.md) | Claude subagent for post-change Rust review (correctness, architecture rules, idioms). |
| [`.claude/agents/test-engineer.md`](.claude/agents/test-engineer.md) | Claude subagent for writing/extending tests per workspace conventions. |
| [`.claude/git-workflow.md`](.claude/git-workflow.md) | Repo-specific git profile (remotes, branching, commit trailers) read by the github-workflow skill. |
| [`.claude/skills/new-scorer/SKILL.md`](.claude/skills/new-scorer/SKILL.md) | Checklist for adding a scorer, keeping schema/impl/factory/tests/docs in sync. |
| [`.claude/skills/new-target/SKILL.md`](.claude/skills/new-target/SKILL.md) | Checklist for adding a target type, keeping schema/adapter/factory/tests in sync. |
| [`.claude/skills/verify/SKILL.md`](.claude/skills/verify/SKILL.md) | End-to-end verification (fmt, clippy, tests, CLI smoke, exit-code contract). |
| `.agents/skills/*/SKILL.md` | Codex-flavored copies of the three skills above (`new-scorer`, `new-target`, `verify`). |
| `.codex/agents/*.toml` | Codex copies of the two agent definitions (`rust-reviewer`, `test-engineer`). |

## Architecture and contributor docs (`docs/`)

| File | Purpose |
|---|---|
| [`docs/README.md`](docs/README.md) | Contributor doc guide for the `docs/` folder; defers to this map for the full catalog. |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Canonical architecture: seven-crate map, dependency direction, run flow, the five core rules. |
| [`docs/trajectory-spec.md`](docs/trajectory-spec.md) | Versioned v0 spec for the agent-trajectory JSON format and the trajectory scorer's matcher rules. |
| [`docs/cassette-lab-plan.md`](docs/cassette-lab-plan.md) | The cassette experiment: record/replay for whole agent runs, run in a separate lab repo, with research questions, graduation bar, kill triggers. |

## Crates

Each crate ships a `README.md` (crates.io landing) and a `CLAUDE.md` (local rules for agents).

| Crate | `CLAUDE.md` (local rules) |
|---|---|
| [`evalcore-config`](crates/evalcore-config/CLAUDE.md) | evals.yaml schema: YAML-first design, tagged-enum conventions, validation boundaries, secrets-by-env-ref. |
| [`evalcore-core`](crates/evalcore-core/CLAUDE.md) | Domain types, Target/Scorer traits, dataset loading, run engine, cache_identity and retry/timeout invariants. |
| [`evalcore-scorers`](crates/evalcore-scorers/CLAUDE.md) | Built-in scorer catalog plus rules for adding scorers, async trait, determinism, failure-reason conventions. |
| [`evalcore-report`](crates/evalcore-report/CLAUDE.md) | Reporters as pure functions of RunSummary: no I/O/clock, styling as pure overlay, snapshot workflow. |
| [`evalcore-store`](crates/evalcore-store/CLAUDE.md) | Cache/store: SHA-256 canonical-JSON keys, the preserve_order ban, cache modes, no-secrets, additive-schema-only. |
| [`evalcore-serve`](crates/evalcore-serve/CLAUDE.md) | Local read-only web viewer: localhost-only, GET-only routes, reuse report renderers, escape DB strings. |
| [`evalcore`](crates/evalcore/CLAUDE.md) | The CLI binary as wiring-only: exit-code contract, path resolution, terminal/env/clock detection confined to ui.rs. |

Crate READMEs: [`config`](crates/evalcore-config/README.md) · [`core`](crates/evalcore-core/README.md) · [`scorers`](crates/evalcore-scorers/README.md) · [`report`](crates/evalcore-report/README.md) · [`store`](crates/evalcore-store/README.md) · [`serve`](crates/evalcore-serve/README.md) · [`evalcore` (CLI)](crates/evalcore/README.md).

## Design system (`design/`)

[`design/README.md`](design/README.md) is the entry point and declares `design/` the canonical
source of truth. Philosophy topics:

| File | Topic |
|---|---|
| [`01-principles.md`](design/philosophy/01-principles.md) | The stance behind every surface (instrument not brochure, light-first, accent-is-judgment). |
| [`02-brand-identity.md`](design/philosophy/02-brand-identity.md) | The mark (three score bars + gate line), geometry, lockup, asset files, usage rules. |
| [`03-color.md`](design/philosophy/03-color.md) | Palette: iris accent, semantic verdict colors, code-blue syntax, neutrals, always-dark code blocks. |
| [`04-typography.md`](design/philosophy/04-typography.md) | Typefaces (Manrope + JetBrains Mono), the scale, weights, the mono-for-literals-only rule. |
| [`05-space-and-layout.md`](design/philosophy/05-space-and-layout.md) | Spacing scale, measures, radii/borders, whitespace-sectioning, elevation, glass treatment. |
| [`06-components.md`](design/philosophy/06-components.md) | Shared component vocabulary: glass windows, buttons, tabs, chips, nav states, asides, diagrams. |
| [`07-motion.md`](design/philosophy/07-motion.md) | Motion tokens and rules: motion carries information, honors reduced-motion, never shifts layout. |
| [`08-voice-and-writing.md`](design/philosophy/08-voice-and-writing.md) | How EvalCore writes: checkable claims, real numbers, sentence rules, headline pattern, attribution. |
| [`09-ecosystem-sync.md`](design/philosophy/09-ecosystem-sync.md) | Cross-surface sync standard, full asset inventory, and the dated design decision log. |

## Examples

| File | Purpose |
|---|---|
| [`examples/quickstart/README.md`](examples/quickstart/README.md) | The offline support-bot example that doubles as the CLI test fixture. |
| [`examples/claims-triage/README.md`](examples/claims-triage/README.md) | Offline claims-triage: a misclassified fraud case demonstrating accuracy/macro-F1 gates. |
| [`examples/support-rag/README.md`](examples/support-rag/README.md) | Offline support-RAG: grounding + safety-guard scorers plus a commented production judge path. |
| [`examples/openai/README.md`](examples/openai/README.md) | Live OpenAI-compatible model calls with usage, cost, and record/replay. |
| [`examples/agent-trace/README.md`](examples/agent-trace/README.md) | Scoring a recorded OTel/OpenInference agent trajectory. |

## Shims

| File | Purpose |
|---|---|
| [`shims/README.md`](shims/README.md) | The Ragas/DeepEval subprocess-scorer shims: when to use them, install, wiring, the `--check` self-test. |

## Site docs (end-user, canonical at [evalcore.cc](https://evalcore.cc))

Source lives in `site/src/content/docs/`. This is the canonical end-user documentation; do not
duplicate guide content into the repo. [`site/README.md`](site/README.md) covers building the site.

**Getting started**
- [`index.mdx`](site/src/content/docs/index.mdx): docs-site landing/splash.
- [`getting-started/installation.mdx`](site/src/content/docs/getting-started/installation.mdx): install via binary, crates.io, or GitHub Action.
- [`getting-started/quickstart.md`](site/src/content/docs/getting-started/quickstart.md): five-minute no-network walkthrough of the shipped example.
- [`getting-started/core-concepts.mdx`](site/src/content/docs/getting-started/core-concepts.mdx): the mental model: run pipeline, targets/datasets/scorers/gates, exit-code contract.
- [`getting-started/what-teams-use-it-for.md`](site/src/content/docs/getting-started/what-teams-use-it-for.md): five high-stakes scenarios mapped to guides.
- [`faq.md`](site/src/content/docs/faq.md): philosophy + practical FAQ. · [`404.md`](site/src/content/docs/404.md): not-found splash.

**Guides**
- [`agents-and-traces.mdx`](site/src/content/docs/guides/agents-and-traces.mdx): evaluate agents from OTel/OpenInference or native traces.
- [`classification.md`](site/src/content/docs/guides/classification.md): label scoring: accuracy, macro-F1, per-class precision/recall, gates.
- [`comparing-models.mdx`](site/src/content/docs/guides/comparing-models.mdx): matrix runs: multiple targets, per-case winners, per-arm cost/trials.
- [`cost-and-budgets.md`](site/src/content/docs/guides/cost-and-budgets.md): declare token rates, read cost, cap spend with `run.budget_usd`.
- [`custom-scorers.md`](site/src/content/docs/guides/custom-scorers.md): the subprocess scorer protocol with runnable Python/Node examples.
- [`evaluating-rest-apis.md`](site/src/content/docs/guides/evaluating-rest-apis.md): the http target: substitution, response_path, auth, retries, keyless replay.
- [`gates-and-baselines.md`](site/src/content/docs/guides/gates-and-baselines.md): save/compare baselines, the regressed/new/fixed diff, pass_rate and mean_score gates.
- [`html-reports.mdx`](site/src/content/docs/guides/html-reports.mdx): the `--html` flag and the Action's html artifact.
- [`llm-as-judge.md`](site/src/content/docs/guides/llm-as-judge.md): grade open-ended answers against a rubric via any OpenAI-compatible endpoint.
- [`rag-evaluation.md`](site/src/content/docs/guides/rag-evaluation.md): evaluate a RAG app: attach context, grade groundedness, wire in the shims.
- [`record-replay.md`](site/src/content/docs/guides/record-replay.md): the full cassette lifecycle: four cache modes, keys, what re-records.
- [`run-history-and-serve.md`](site/src/content/docs/guides/run-history-and-serve.md): run history and the local `evalcore serve` viewer.
- [`running-in-ci.md`](site/src/content/docs/guides/running-in-ci.md): the end-to-end CI story with GitHub/GitLab/Jenkins snippets.
- [`semantic-similarity.md`](site/src/content/docs/guides/semantic-similarity.md): the `similarity` scorer over OpenAI-compatible embeddings.
- [`trials-and-statistics.mdx`](site/src/content/docs/guides/trials-and-statistics.mdx): `run.trials`: running each case N times, folding, flakiness stats.

**Reference**
- [`reference/configuration.md`](site/src/content/docs/reference/configuration.md): the complete `evals.yaml` schema.
- [`reference/cli.md`](site/src/content/docs/reference/cli.md): the `evalcore` binary: subcommands, every flag, exit codes.
- [`reference/cache-and-determinism.md`](site/src/content/docs/reference/cache-and-determinism.md): the record/replay cache and determinism guarantees.
- [`reference/subprocess-protocol.md`](site/src/content/docs/reference/subprocess-protocol.md): the subprocess scorer contract (protocol v0).
- [`reference/trajectory-format.md`](site/src/content/docs/reference/trajectory-format.md): the canonical agent-trajectory format spec (v0).

## Private knowledge base (not in this repo)

Product-internal context (positioning, roadmap, competitive research) lives in a **gitignored
`wiki/` directory**: its own git repo, seeded from the internal PRD. When present, its index is
`wiki/index.md`. Nothing from `wiki/` or the PRD is ever copied into this repo's tracked files.
