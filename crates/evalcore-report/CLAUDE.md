# evalcore-report

Report rendering over `RunSummary`. Depends only on `evalcore-core`.

## Rules

- Every reporter is a **pure function** of its inputs: no I/O, no clock reads, no environment access, and crucially **no terminal detection**. Identical inputs must render byte-identical reports — that's what makes snapshot tests and baseline diffs possible. (File writing, and deciding *whether* to color, belong to the CLI.)
- The terminal reporters (`terminal`, `baseline`, `terminal_matrix`) take a plain-data `&Style { color, unicode, quiet }`. Styling is a **pure overlay**: `Style::plain()` reproduces the pre-styling bytes exactly (a `styled_output_differs_only_by_ansi` test pins that stripping SGR from a styled render equals the plain render). Color is emitted with `anstyle` (style structs only — no detection, no globals); the CLI computes the `Style` from `--color`/TTY/`NO_COLOR`. Never read the terminal here.
- **Never let color be the only status signal**: the PASS/FAIL/GATE words stay present in every path. All user-derived text (case ids, reasons, target/gate names, class labels) is run through the private `sanitize` before it reaches the terminal, neutralizing ANSI/control-sequence injection; benign text is returned unchanged so ordinary output is byte-identical. JSON/JUnit/HTML keep their own escaping and are byte-frozen.
- Anything user-controlled that lands in XML goes through `xml_escape`; add an escaping assertion when touching JUnit output.
- Snapshot tests (insta) run on fixtures with **fixed latencies** — never snapshot real timing. To regenerate after an intentional format change: `INSTA_UPDATE=always cargo test -p evalcore-report`, then re-run normally and commit the `.snap` files under `src/snapshots/`.
- New reporter checklist: pure function here, `Reporter` enum arm in the CLI, snapshot test with the shared fixture, row in the README if user-facing.
