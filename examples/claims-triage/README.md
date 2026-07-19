# Claims triage that catches a silent fraud-recall drop

An insurer (or health plan) auto-routes every incoming claim to one of four
queues: `auto`, `property`, `injury`, or `fraud-review`. The failure that costs
real money is subtle: the model quietly gets a little worse at spotting fraud, a
few staged claims slip into the `auto` queue and get paid, and nothing looks
broken because overall accuracy barely moves. This suite gates on the metrics
that *do* move — **accuracy** and **macro-F1** — and puts **per-class precision
and recall** in every report, so a drop in fraud recall is visible and gateable.

It runs fully offline. The target is a small shell script, `triage.sh`, that
keyword-routes a claim description to a label — deliberately misclassifying one
case (a staged claim whose "collision" wording pulls it into `auto` before the
fraud signal is ever checked) so the report is interesting while the gates still
pass.

## Run it

```sh
evalcore run examples/claims-triage/evals.yaml
```

```
PASS auto-rear-end (76ms)
PASS auto-deer (76ms)
PASS auto-parking (72ms)
PASS property-pipe (77ms)
PASS property-fire (31ms)
PASS injury-slip (31ms)
PASS injury-whiplash (32ms)
PASS fraud-duplicate (26ms)
PASS fraud-staged-collision (11ms)

9 passed, 0 failed, 9 total
GATE PASS accuracy >= 0.8 (actual 0.89)
GATE PASS macro_f1 >= 0.7 (actual 0.88)
classification: accuracy 0.89 · macro-F1 0.88 (9 labeled, 0 unlabeled)
```

Exit code `0`. Every case emits a valid label (the per-case `regex` scorer), so
the exit code is driven by the **gates**, not by whether any single prediction
was right — grading correctness is the gates' job.

## The math (one deliberate miss)

`fraud-staged-collision` is truly `fraud-review` but is routed to `auto`. Over
the 9 labeled cases:

| label        | precision | recall | F1   | support |
|--------------|-----------|--------|------|---------|
| auto         | 0.75      | 1.00   | 0.86 | 3       |
| property     | 1.00      | 1.00   | 1.00 | 2       |
| injury       | 1.00      | 1.00   | 1.00 | 2       |
| fraud-review | 1.00      | 0.50   | 0.67 | 2       |

- **Accuracy** = 8 correct / 9 = **0.89** — clears the `0.8` floor.
- **Macro-F1** = mean(0.86, 1.00, 1.00, 0.67) = **0.88** — clears the `0.7` floor.

The one miss drags `fraud-review` recall to `0.50` and `auto` precision to
`0.75` — exactly the fingerprint of fraud leaking into a normal queue. Both
gates pass today; push the fraud misses higher and macro-F1 sinks below `0.7`
and the build goes red.

## Swap in your real classifier

Replace the `shell` target with your live classifier — an `openai-compatible`
model or your own `http` endpoint — and the cases, labels, and gates stay
identical. See the [classification
guide](https://eval-core.github.io/evalcore/guides/classification/).
