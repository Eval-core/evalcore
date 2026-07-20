---
title: CLI reference
description: The evalcore command. Validate and run, every flag, the exit-code contract, output destinations, and env-var handling.
---

The `evalcore` binary has three subcommands: `validate` (parse-check a config),
`run` (execute a suite), and `serve` (a local read-only viewer over run history).
`validate` and `run` take the config path as their first positional argument.

```sh
evalcore validate examples/quickstart/evals.yaml
evalcore run examples/quickstart/evals.yaml --reporter junit --output results.xml
```

Relative paths inside a config (dataset files, trace paths) resolve against the
**config file's directory**, never the current working directory, so a suite
runs identically from any directory (CI, editors, make).

## `evalcore validate <config>`

Parses and validates the config without executing anything. No target or scorer
is ever run.

| Argument | Description |
|---|---|
| `<config>` | Path to the `evals.yaml` file. |

On success it prints a one-line summary and exits `0`:

```text
OK: 2 target(s), 1 dataset(s), 5 scorer(s)
```

On any parse or validation error it exits non-zero with the error message. See
the [Configuration reference](/reference/configuration/) for every validation rule.

## `evalcore run <config>`

Runs a suite: builds the selected target, loads datasets, runs every case
through every scorer, renders a report, and returns an exit code.

| Argument | Description |
|---|---|
| `<config>` | Path to the `evals.yaml` file. |

### Flags

| Flag | Value | Default | Description |
|---|---|---|---|
| `--target` | name | n/a | Target to run. May be omitted only when exactly one target is defined; with several, omitting it is an error naming the available targets. |
| `--matrix` | comma list of names | n/a | Run the whole suite against several targets and print a side-by-side comparison. At least two distinct, defined names. Overrides `run.matrix`. Mutually exclusive with `--target`, `--baseline`, `--save-baseline`. Since v0.7.0. See [Matrix runs](#matrix-runs). |
| `--reporter` | `terminal` \| `json` \| `junit` | `terminal` | Report format. See [Reporters](#reporters). |
| `--output` | path | n/a | Write the report to a file instead of stdout. |
| `--html` | path | n/a | Also write a self-contained HTML report to this path, in addition to the primary `--reporter` output. Since v0.5.0. |
| `--cache` | `auto` \| `replay` \| `live` \| `off` | `auto` | Record/replay cache mode for cacheable targets. See [Cache modes](#cache-modes). |
| `--baseline` | label | n/a | Gate on regressions against a stored baseline instead of absolute pass/fail. |
| `--save-baseline` | label | n/a | Save this run's results as a named baseline. |
| `--no-history` | flag | off | Do not append a run-history row for this run (overrides `run.history: true`). The exit code and report bytes are unaffected; history is metadata for [`evalcore serve`](#evalcore-serve). Since v0.7.0. |
| `--color` | `auto` \| `always` \| `never` | `auto` | Semantic color in the terminal report. `auto` colors only an interactive terminal; `always` forces it; `never` disables it. Honors `NO_COLOR`, `TERM=dumb`, and `CLICOLOR_FORCE`. Machine reporters (`json`/`junit`) and `--output` files are never colored. Since v0.7.5. |
| `--progress` | `auto` \| `never` | `auto` | Live spinner and `k/N` case counter on stderr. `auto` shows it only on an interactive terminal; `never` disables it. Since v0.7.5. |
| `-q, --quiet` | flag | off | Print failing cases and the summary only, suppressing per-case `PASS` lines. Since v0.7.5. |

### Target selection

`--target <name>` picks a target from the config's `targets` map. A name that
isn't defined is an error listing the available targets. With exactly one target
the flag may be omitted; with more than one, omitting it fails with `multiple
targets defined; pass --target <name> (available: …)`.

### Matrix runs

Since v0.7.0. `--matrix <name,name,…>` runs the whole suite once
per named target, sequentially in list order, and prints each arm's report
followed by a `== comparison` table. It overrides `run.matrix` in the config;
either surface needs at least two distinct, defined names. `run.budget_usd`
applies per arm, and each arm prices with its own target's `cost` rates. The exit
code is `0` iff **every** arm passes all its cases and gates, else `1`.

A matrix is mutually exclusive with target selection and baselines. Each
combination is a hard error rather than a silent choice:

```text
$ evalcore run evals.yaml --matrix gpt,claude --target gpt
Error: cannot combine --target with a matrix: a matrix already runs the suite against several targets. Drop --target, or drop the matrix.

$ evalcore run evals.yaml --matrix gpt,claude --baseline main
Error: baselines are per-run; run targets separately with --target to baseline them
```

`--save-baseline` is rejected identically. See the [Comparing models
guide](/guides/comparing-models/) for the comparison table and winner
semantics.

### Reporters

`--reporter` selects the primary report format. All three are pure functions of
the run summary, so identical runs render byte-identical reports (see
[Reporter formats](#reporter-formats)).

| Value | Output |
|---|---|
| `terminal` | Human-readable `PASS`/`FAIL` lines, a summary line with totals (and tokens/cost when available), then one `GATE` line per configured gate. |
| `json` | The full `RunSummary` as pretty JSON (includes the `gates` array when gates are configured). |
| `junit` | JUnit XML (`<testsuites>`) for CI systems. One `<testcase>` per case; failures carry a `<failure message="…">`. |

### Output destination

- Without `--output`, the report is printed to **stdout**.
- With `--output <file>`, the report is written to the file and a one-line
  summary goes to **stderr**: `<p> passed, <f> failed, <t> total — report
  written to <path>`.
- `--html <path>` always writes an additional HTML file; it never replaces the
  primary reporter's output. It composes with every reporter and embeds the
  baseline diff when `--baseline` is used.

### Cache modes

`--cache` controls how the record/replay cache participates for cacheable
targets (LLM APIs, `http` targets, judge scorers, and `similarity` embedding
calls). Uncacheable targets
(`shell`, `trace`) bypass the cache in every mode. See [Cache and
determinism](/reference/cache-and-determinism/) for the full model.

| Mode | Behavior |
|---|---|
| `auto` (default) | Replay hits; on a miss, call live and record the result. |
| `replay` | Cache only. A miss is a case failure, never a live call. Use in CI for deterministic, zero-cost reruns. |
| `live` | Always call live and overwrite the recording. |
| `off` | Bypass the cache entirely. |

### Baselines

`--save-baseline <label>` stores this run's per-case results under a label.
`--baseline <label>` loads the newest baseline with that label and compares,
flipping the exit contract from "all passed" to "no regressions". Used together,
they give rolling baselines: the run is compared against the stored baseline and
then this run is saved (after comparison). A `--baseline` label with no stored
baseline is an error: `no baseline "<label>" found — record one with
--save-baseline <label>`. Baselines are stored in the same `.evalcore/cache.db`
file as the cache; the store is opened even for `shell` targets when a baseline
flag is present. See [Cache and determinism](/reference/cache-and-determinism/#baselines).

Baseline results print after the primary report as a diff section
(`baseline "<label>": p/t passed -> current: p/t passed`, then `REGRESSED`,
`NEW FAIL`, `FIXED`, `REMOVED` lines). For the `terminal` reporter writing to
stdout the diff goes to **stdout**; for machine reporters (or any `--output`
run) it goes to **stderr**, keeping the machine reporter's stdout pure.

## Exit-code contract

`evalcore run` exits `0` when the run passed and `1` otherwise. Gate CI on it.

- **Default** (no `--baseline`): exit `0` iff every case passed.
- **With `--baseline`**: the gate becomes "no regressions". Exit `0` iff no
  case regressed and no previously-passing case newly fails. Failures already
  accepted into the baseline are tolerated.
- **Suite gates are additive.** Regardless of the above, if any configured
  `run.gates` floor is not met, the run exits `1`. With `--baseline`, an
  accepted baseline failure stays tolerated per-case yet can still sink a
  `pass_rate` gate it drops the run below. See
  [Gates](/reference/configuration/#gates).

`validate` exits `0` on a valid config and non-zero on any error.

## Environment variables and secrets

Secrets are never inline in YAML: a target or judge references an environment
variable by name (`api_key_env`), resolved at build time.

- In modes that may call the network (`auto`, `live`, `off`), a referenced but
  unset variable is a build error, and the run fails fast before any case runs:
  `environment variable <VAR> is not set`.
- In `--cache replay`, secrets are **optional**: a missing variable resolves to
  no key. Replay-only runs never call the live target, which is what lets CI
  replay a committed cache with no API keys configured at all.

## Reporter formats

The three reporters are pure `&RunSummary -> String` functions with fixed,
snapshot-tested layouts.

### terminal

```text
PASS refund-1 (12ms)
FAIL refund-2
     contains: expected output to contain "refund", got: "I can't help with that"

2 passed, 1 failed, 3 total · 210 tokens · $0.0020
GATE PASS pass_rate >= 0.95 (actual 1.00)

FAILED
```

Each passing case is `PASS <id> (<latency>ms)`; each failing case is `FAIL <id>`
followed by indented failure reasons. The summary line always shows
`<p> passed, <f> failed, <t> total`, with ` · <n> tokens` and ` · $<cost>`
appended only when the run reported usage/cost. Gate lines
(`GATE PASS|FAIL <gate> (actual <n>)`, with an indented reason when present)
follow, and are absent entirely when no gates are configured. A closing
`PASSED` or `FAILED` verdict line ends the report, matching the run's exit code.
On an interactive terminal the report also carries restrained semantic color and
a live progress spinner; piped, redirected, in CI, or under `NO_COLOR`/`TERM=dumb`
the output is byte-for-byte the previous plain text. See `--color`, `--progress`,
and `-q/--quiet` above.

### json

`serde_json::to_string_pretty` of the full `RunSummary`: `results` (one object
per case with `output`, `error`, `scores`, `cost_usd`) and `gates`. The `gates`
array is omitted entirely when no gates are configured, and absent optional
fields are omitted, so a gate-free run's JSON is byte-identical to before gates
existed.

### junit

```xml
<?xml version="1.0" encoding="UTF-8"?>
<testsuites tests="3" failures="1">
  <testsuite name="evalcore" tests="3" failures="1">
    <testcase name="refund-1" time="0.012"/>
    <testcase name="refund-2" time="0.040">
      <failure message="contains: expected output to contain &quot;refund&quot;"/>
    </testcase>
  </testsuite>
</testsuites>
```

Times are latency in seconds (three decimals). Failure messages join a case's
reasons with `; `. Every user-controlled value is XML-escaped. JUnit output does
not include gate outcomes; the exit code carries the gate result.

## `evalcore serve`

Since v0.7.0. Starts a **local, read-only** web viewer over the
[run history](/guides/run-history-and-serve/) stored in a `.evalcore/cache.db`
file. Unlike `validate` and `run`, it takes no config argument, because it reads the
store rather than a config.

```sh
evalcore serve                              # reads .evalcore/cache.db, binds 127.0.0.1:7878
evalcore serve --store path/db --port 9000  # explicit store and port
```

| Flag | Value | Default | Description |
|---|---|---|---|
| `--store` | path | `.evalcore/cache.db` | SQLite store to read run history from. |
| `--port` | u16 | `7878` | Port to bind on `127.0.0.1`. |

On start it prints the URL and runs until interrupted (Ctrl-C):

```text
serving http://127.0.0.1:7878
```

**The bind address is fixed at `127.0.0.1`, and localhost is the entire security
model.** There is no bind-address knob, no auth (there is no remote access to
authenticate), and no telemetry. Every route is a `GET`, and any other method
returns `405`. The viewer only ever reads the store, so nothing it does can
mutate history or leave the machine. Pages are self-contained HTML (inline CSS,
no external requests, no JS) and every stored string is escaped.

Three routes:

| Route | Shows |
|---|---|
| `GET /` | The run listing, newest first: id, time, config, target, passed/failed/total, cost, a pass-rate sparkline, and a "diff vs previous same-target run" link. |
| `GET /run/{id}` | That run's full detail, byte-for-byte the `--html` report. Unknown id → `404`; a non-integer id → `400`. |
| `GET /diff?a=<id>&b=<id>` | Any two stored runs compared with the [matrix comparison](/guides/comparing-models/) view. Missing/non-integer ids → `400`; an unknown id → `404`. |

See the [Run history and serve guide](/guides/run-history-and-serve/) for the
workflow.

## See also

- [Configuration reference](/reference/configuration/): the `evals.yaml`
  fields these flags read and override.
- [Running in CI](/guides/running-in-ci/): `--cache replay`, `--baseline`,
  and `--reporter junit` wired into a pipeline.
- [Gates and baselines](/guides/gates-and-baselines/): what `--baseline`
  and `--save-baseline` do to the exit code.
- [Cache and determinism](/reference/cache-and-determinism/): what each
  `--cache` mode does to the store on disk.
- [Run history and serve](/guides/run-history-and-serve/): the workflow
  around `evalcore serve` and stored runs.
