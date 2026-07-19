---
title: Run history and serve
description: "EvalCore records every run to a local SQLite file and serves a read-only web viewer over it: browse past runs, watch the pass-rate trend, and diff any two runs (model A vs B, yesterday vs today). Local-first, so nothing leaves your machine."
---

Every `evalcore run` already tells you whether *this* run passed. But the
questions that come up a day later are historical: *is the pass rate trending
down?* *what did last night's run actually say?* *how does model A's run compare
to model B's, or today's run to yesterday's?* Answering those means keeping the
runs around and being able to look back at them.

Run history does exactly that. Every run appends a row to your local store, and
`evalcore serve` opens a small read-only web viewer over those rows: a listing
with a trend sparkline, each run's full HTML report, and a diff between any two
runs. It is entirely local. There is no server to sign up for and no data leaves
your machine.

## Recording history

History is **on by default**. Every `evalcore run` appends one row per executed
run to the store with that run's full result: the same cases, scores, gates, and
classification the report shows. You don't have to do anything:

```sh
evalcore run evals.yaml --target gpt
evalcore run evals.yaml --target claude
```

Two runs, two history rows. A [matrix run](/evalcore/guides/comparing-models/)
records **one row per arm**, each keyed by that arm's target name, so a single
`--matrix gpt,claude` invocation lands two rows, exactly as if you had run the
two targets separately:

```sh
evalcore run evals.yaml --matrix gpt,claude   # records a row for gpt and a row for claude
```

The row is written **after** gates and classification are attached and **before**
the report is rendered, so what history stores is precisely what you saw on the
terminal. If writing the row fails for any reason (a read-only disk, say), you
get a warning on stderr and nothing else changes. **History never fails a run.**
The eval verdict and the exit code your CI gates on never depend on history I/O.

### Turning it off

To skip the history row for a single invocation, pass `--no-history`:

```sh
evalcore run evals.yaml --no-history
```

To turn it off for a suite, set `run.history` in the config:

```yaml
run:
  history: false   # default is true
```

Either way the run is byte-identical otherwise: same report, same exit code.
History is metadata for the viewer, never part of the eval itself.

## Where history lives

History rows go into the same `.evalcore/cache.db` SQLite file as the
[record/replay cache](/evalcore/guides/record-replay/) and baselines. That is one
local store per project, in a new `run_history` table that sits beside the
existing ones (adding it doesn't touch the cache). Nothing is written anywhere
else.

Because it is just a file in your repo, committing `.evalcore/cache.db` shares
history with your teammates the same way committing it shares the replay
cassette: they pull the repo and `evalcore serve` shows them the same runs you
see. Leave it uncommitted (gitignored) and history stays personal to your
machine. Either choice works, since history is additive.

## Browsing with `evalcore serve`

`evalcore serve` starts a local, read-only web viewer over the store:

```sh
evalcore serve
```

It prints the URL and runs until you press Ctrl-C:

```
serving http://127.0.0.1:7878
```

Open that URL in a browser. By default it reads `.evalcore/cache.db` and binds
port `7878`; both are configurable:

```sh
evalcore serve --store path/to/cache.db --port 9000
```

### The trust story

The viewer is local-first and read-only by design. It binds `127.0.0.1` only,
localhost, never a wildcard address, and that is the *entire* security model:
there is no auth because there is no remote access. Every route is a `GET` (any
other method is a `405`), and there are zero mutation endpoints, so the viewer
only ever reads the store. There is no telemetry, no external request, and no
JavaScript framework; the pages are self-contained HTML. **Nothing leaves your
machine.**

This is the same principle stated in the [FAQ](/evalcore/faq/): EvalCore is
local-first and CI-native, with no server, no signup, and results in a SQLite
file next to your repo. A hosted tier, if one ever exists, composes *with* that;
it will never gate the local features. Run history and the viewer work fully
offline, forever, with no account.

## The three pages

### `/`: the run listing

The landing page is a table of every recorded run, newest first: the run id,
when it was recorded, the config path, the target (a matrix arm's name for matrix
runs), passed / failed / total, and cost when the run reported any. Above the
table, an inline pass-rate sparkline plots the trend over the most recent runs,
a compact glyph for "are we getting better or worse?"

A real listing looks like this, with four runs: two single-target, then a
`--matrix echo,upper` that added two more.

| Run | Recorded | Config | Target | Passed | Failed | Total | Cost | Diff |
|---|---|---|---|---|---|---|---|---|
| #4 | 2026-07-19 01:19:46 | evals.yaml | upper | 2 | 0 | 2 | | vs #2 |
| #3 | 2026-07-19 01:19:46 | evals.yaml | echo | 1 | 1 | 2 | | vs #1 |
| #2 | 2026-07-19 01:19:46 | evals.yaml | upper | 2 | 0 | 2 | | — |
| #1 | 2026-07-19 01:19:46 | evals.yaml | echo | 1 | 1 | 2 | | — |

Each run id links to its detail page. The Diff column links each run to the
nearest older run of the *same target*, so "did this target regress since last
time?" is one click. The first run of a target has nothing to diff against, so
it shows `—`.

### `/run/{id}`: one run's report

Clicking a run id opens that run's full detail, and it is byte-for-byte the same
document `--html` writes: the header counts, the gates panel, one expandable row
per case with the output, per-scorer scores, trials detail, and agent trajectory.
The viewer reuses the [HTML report](/evalcore/guides/html-reports/) renderer
verbatim, so a page you serve here is identical to a report you'd attach to a PR.
(An unknown id is a plain 404.)

### `/diff?a=<id>&b=<id>`: compare any two runs

The diff page lays any two stored runs side by side, using the same
[matrix comparison](/evalcore/guides/comparing-models/) view a live matrix run
prints: a per-case PASS/FAIL cell for each run and a winner column. Because it
works over *stored* runs, the two sides don't have to come from one invocation:

- Model A vs model B. Diff `gpt`'s run against `claude`'s run, even though you
  ran them separately on different days.
- Yesterday vs today. Diff two runs of the *same* target to see exactly which
  cases moved.

The "diff vs previous" links on the listing are just shortcuts into this page;
you can also point it at any two ids by hand (`/diff?a=1&b=3`). Cases are matched
by id, and missing cases are handled the way the matrix comparison already does.

## See also

- [Comparing models](/evalcore/guides/comparing-models/): the matrix comparison
  the diff page reuses, live in a single invocation.
- [HTML reports](/evalcore/guides/html-reports/): the exact document `/run/{id}`
  serves.
- [Record / replay](/evalcore/guides/record-replay/): the `.evalcore/cache.db`
  store history shares, and why committing it helps a team.
- [CLI reference](/evalcore/reference/cli/#evalcore-serve): the `serve`
  subcommand flags and the `--no-history` flag.
- [Configuration reference](/evalcore/reference/configuration/#run-block): the
  `run.history` field.
