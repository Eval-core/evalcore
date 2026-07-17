---
title: HTML reports
description: The --html flag and the Action's html-artifact input — what's in the self-contained report, why it always uploads on failure, using it in PR review, and its air-gapped, file:// friendliness.
---

Exit codes gate the build; an HTML report is what a human opens to see *why*. The
`--html` flag writes a single, self-contained document — the shareable "here's
the eval report" artifact a reviewer clicks straight from a PR.

```sh
evalcore run evals.yaml --cache replay --html report.html
```

It is written **in addition to** the primary `--reporter` output, never instead
of it — so you can emit terminal output to the console *and* a rich HTML file in
the same run. It composes with every reporter and every flag.

## What's in the report

The document mirrors what the terminal reporter shows, expanded for the browser:

- **Header** — an overall pass/fail badge and the summary counts (passed /
  failed / total), plus total tokens and cost when the target reports usage.
- **Gates panel** — one row per suite gate with its status, floor, actual value,
  and any reason. Omitted entirely when no gates are configured.
- **Cases** — one **expandable row per case**, in dataset order. Each row shows
  status, case id, latency, and cost; expanding it reveals:
  - the **output** text,
  - a **scores** table (each scorer's value, pass/fail, and reason),
  - for a target error, the error as the case's reason,
  - for `trace` cases, the agent **trajectory** — each tool call with its input
    and output, expandable inline.
- **Baseline diff** — when you pass `--baseline`, the same regressed /
  new-failing / fixed / removed breakdown the terminal diff prints, embedded in
  the report.

## Self-contained and deterministic

The report is deliberately minimal and portable:

- **One file, no dependencies.** All CSS is inlined; there are **no external
  requests** — no CDN, no fonts, no images — and **no JavaScript**. Expansion is
  driven entirely by native HTML `<details>` elements.
- **Air-gapped and `file://`-friendly.** Because nothing is fetched, it opens
  correctly straight from disk (`file:///path/report.html`) with no server and
  no network — ideal for locked-down or offline environments.
- **Light and dark.** It themes to the reader's `prefers-color-scheme`
  automatically.
- **Deterministic byte-for-byte.** Like every reporter, it's a pure function of
  the run — the same run renders identical bytes, so it diffs and snapshot-tests
  cleanly.
- **Safe.** Every user-derived value (case ids, outputs, reasons, tool names,
  JSON payloads) is HTML-escaped, so even a hostile model output renders as inert
  text.

## The GitHub Action produces it automatically

The Action's `html-artifact` input (default `"evalcore-report"`) passes `--html`
for you and uploads the file as a CI artifact:

```yaml
- uses: eval-core/evalcore@v0.5.0
  with:
    config: evals/evals.yaml
    args: --cache replay --baseline main
    html-artifact: evalcore-report   # artifact name; "" disables it
```

Two behaviors worth knowing:

- **It uploads even when the suite fails.** The report is *most* valuable on a
  red build, so the upload step runs regardless of the run's outcome and never
  changes the run's exit code. The job step summary notes that the report was
  uploaded.
- **Set it to `""` to disable.** With an empty value, no `--html` is passed and
  nothing is uploaded — the command is byte-identical to a run without the flag.

## Using it in PR review

The workflow for a reviewer:

1. A PR's eval job runs (offline, replayed) and — if it regressed something —
   goes red.
2. The reviewer opens the run's **artifacts** and downloads `evalcore-report`.
3. They open the HTML file and expand the failing case to read its output, the
   scorer that failed and why, and — for agents — the exact trajectory the run
   took.
4. With `--baseline` set, the embedded diff shows precisely which cases
   regressed relative to the accepted state.

No log-scraping, no re-running with more verbosity — the answer to "why did this
fail?" is one click and one expand away.

For the CI wiring around this, see [Running in CI](/evalcore/guides/running-in-ci/).
