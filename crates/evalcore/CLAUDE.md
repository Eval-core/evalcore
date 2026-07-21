# evalcore (CLI binary)

> Parent: [root CLAUDE.md](../../CLAUDE.md) · architecture: [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) · map: [MAP.md](../../MAP.md)

The user-facing binary. This crate is **wiring only**: clap parsing, config→factory→engine→reporter composition, file writing, exit codes. Logic lives in the library crates — if you're writing an algorithm here, it belongs elsewhere.

## Contracts (breaking these breaks users' CI)

- Exit codes: `0` = every case passed; `1` = case failures or any error. Tested in `tests/cli.rs`.
- Relative paths in a config file resolve against the **config file's directory**, never the CWD.
- Reports go to stdout by default; with `--output <file>` the report goes to the file and a one-line summary goes to stderr.
- `validate` never executes targets or scorers.
- **All terminal/env/clock detection lives in `ui.rs`** — the reporters stay pure. `ui::resolve` reads `--color`/`--progress`/`--quiet`, `NO_COLOR`, `TERM=dumb`, `CLICOLOR_FORCE`, and per-stream `IsTerminal`, and hands the reporter a plain-data `Style`. A Terminal report is styled only when it owns an interactive stdout; **files and machine reporters (json/junit) always get plain bytes** (no ANSI, no verdict). The prominent `PASSED`/`FAILED` verdict is CLI-emitted (it reflects the real exit code, folding cases+gates+baseline) — to stdout beside a Terminal report, else to stderr so machine stdout stays pure. Progress is a stderr-only, TTY-only counter driven by `RunOptions::on_progress`; it never writes stdout and is absent in captured/CI runs.

## Tests

`tests/cli.rs` spawns the real binary via `assert_cmd`. The quickstart example (`examples/quickstart/`) doubles as the test fixture — editing it can break these tests, deliberately. Assert on stable output fragments (counts, case ids), never on latencies.
