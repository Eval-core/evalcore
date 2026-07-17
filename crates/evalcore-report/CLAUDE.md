# evalcore-report

Report rendering over `RunSummary`. Depends only on `evalcore-core`.

## Rules

- Every reporter is a **pure function** `&RunSummary -> String`: no I/O, no clock reads, no environment access. Identical summaries must render byte-identical reports — that's what makes snapshot tests and future baseline diffs possible. (File writing belongs to the CLI.)
- Anything user-controlled that lands in XML goes through `xml_escape`; add an escaping assertion when touching JUnit output.
- Snapshot tests (insta) run on fixtures with **fixed latencies** — never snapshot real timing. To regenerate after an intentional format change: `INSTA_UPDATE=always cargo test -p evalcore-report`, then re-run normally and commit the `.snap` files under `src/snapshots/`.
- New reporter checklist: pure function here, `Reporter` enum arm in the CLI, snapshot test with the shared fixture, row in the README if user-facing.
