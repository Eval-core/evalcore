# Architecture

EvalCore is a Rust workspace of seven crates arranged in strict dependency order. The
short version: configuration is pure data, the core owns the traits and the engine,
everything else implements or renders, and the binary only wires things together.

## Crate map

| Crate | Role |
|---|---|
| `evalcore-config` | The `evals.yaml` schema: parsing and validation. Pure data, no behavior. |
| `evalcore-core` | Domain types, the `Target` and `Scorer` traits, dataset loading, the run engine. |
| `evalcore-scorers` | Built-in scorers (contains, exact, regex, json-schema, similarity, judge, trajectory, subprocess). One per file. |
| `evalcore-report` | Reporters as pure `&RunSummary -> String` functions: terminal, JSON, JUnit, HTML. |
| `evalcore-store` | SQLite storage: the record/replay cache, baselines, run history. |
| `evalcore-serve` | Local read-only web viewer over the store: run list, report, diff. |
| `evalcore` | The CLI binary. Wiring only, no logic. |

Dependency direction:

```
evalcore-config <- evalcore-core <- { evalcore-scorers, evalcore-report,
                                     evalcore-store, evalcore-serve } <- evalcore (bin)
```

Traits live in `evalcore-core`; implementations live downstream. The direction never
inverts, and `evalcore-serve` is a leaf like the binary: nothing depends on it, which
keeps UI concerns off the critical path.

## How a run flows

1. `evalcore-config` parses and validates `evals.yaml` into plain data.
2. `evalcore-core` loads datasets (JSONL, errors cite file and line), builds targets,
   and runs cases concurrently while preserving dataset order in the results.
3. Each case goes to its target (through the record/replay cache when the target is
   cacheable), then every scorer grades the output.
4. `evalcore-report` renders the summary; `evalcore-store` persists cassettes,
   baselines, and run history; the process exits 0 if everything passed and 1 otherwise.

## The rules that shape every change

1. **Protocols over SDKs.** Extension points are language-agnostic: targets speak HTTP
   or shell, custom scorers speak JSON over stdin/stdout, judges are OpenAI-compatible
   endpoints, agent traces arrive as OTel/OpenInference exports. A design that forces
   users to write Rust is wrong; Rust is the engine, never the interface.
2. **YAML first.** Every user-facing feature starts as config surface in
   `evalcore-config`. The YAML is designed before the types.
3. **Determinism.** Identical inputs give identical outputs everywhere: results stay in
   dataset order, reporters are pure functions, and nothing user-visible reads the clock
   except latency measurement. Cache keys hash canonical request JSON;
   `crates/evalcore-store` documents the invariants.
4. **Failures are data.** A target error is a failed case with a reason and a scorer
   error is a failing score with a reason. Runs never panic, and one bad case never
   aborts the suite.
5. **Exit-code contract.** `evalcore run` exits 0 (all passed) or 1 (anything else),
   and CI gates on it. Gates and baselines extend this contract without breaking it.

## Extension points

- **New target or scorer:** each is one file implementing one trait, registered in a
  factory. The `new-target` and `new-scorer` checklists under `.claude/skills/` keep
  schema, implementation, tests, and docs in sync.
- **Custom scoring in any language:** the subprocess protocol takes JSON on stdin and
  returns `{"score": ..., "reason": "..."}` on stdout. `shims/` shows Ragas and
  DeepEval running behind it.
- **Trace evaluation:** apps export OTel/OpenInference traces; EvalCore ingests the
  files and asserts on the trajectory. The format is specified in
  [trajectory-spec.md](trajectory-spec.md).

## Testing conventions

Unit tests sit inline; cross-crate behavior lives in `tests/`. HTTP tests use wiremock
only (no real network anywhere), CLI tests run the real binary via `assert_cmd`
asserting exit codes and stable output fragments, and report snapshots use insta on
fixtures with fixed latencies. `examples/quickstart/` doubles as the end-to-end fixture
and runs without network access.
