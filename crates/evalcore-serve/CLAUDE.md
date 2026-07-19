# evalcore-serve

The `evalcore serve` viewer: a **local, read-only** web UI over the run-history
in an `.evalcore/cache.db` store. Depends on `evalcore-core`,
`evalcore-report`, and `evalcore-store` — **never** `evalcore-config` (it reads
the store, not a config). Nothing depends on this crate except the CLI binary.

## Rules

- **Localhost-only is the whole security model.** The binder hard-codes
  `127.0.0.1`; there is no auth because there is no remote access. Do not add a
  bind address knob.
- **Read-only.** Every route is `GET`; there are zero mutation endpoints. A
  non-GET method returns 405 (axum's `MethodRouter` does this for us — keep it).
- **Reuse the report renderers, don't fork them.** `/run/:id` is
  `evalcore_report::html`; `/diff` builds a `MatrixSummary` and calls
  `evalcore_report::html_matrix`. Only the listing page is rendered here.
- **Escape every DB-derived string** with `evalcore_report::html_escape` — the
  same rule the report uses. A hostile target name must render inert.
- **Failures are data.** A corrupt summary row is an error entry in the
  listing, never a panic or a whole-page 500.
- Pages are self-contained (inline CSS, no external requests, no JS). The
  pass-rate sparkline is server-computed pure SVG.
- Tests drive the router with `tower::ServiceExt::oneshot` — no real port
  binding, no network.
