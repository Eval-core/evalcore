---
title: Classification
description: Score a label-prediction suite with accuracy, macro-averaged F1, and per-class precision/recall over your labeled cases, plus gates you can put in CI.
---

Not every eval is open-ended generation. Intent routing, sentiment,
spam/not-spam, and support-ticket triage all ask the model to pick one label
from a fixed set, and the question you want answered is "how often does it pick
the right one, and where does it confuse one class for another?"
`run.classification` computes the standard classification metrics (accuracy,
macro-averaged F1, and per-class precision/recall) over the cases that carry an
expected label.

## The dataset

A case is a labeled classification case when it has an `expected` field. The
label is that `expected` value; the prediction is the target's output. Both are
plain strings, so the class is whatever text you put there:

```jsonl
{"id": "ticket-1", "input": "I was charged twice this month", "expected": "billing"}
{"id": "ticket-2", "input": "the app crashes on launch", "expected": "technical"}
{"id": "ticket-3", "input": "how do I export my data?", "expected": "technical"}
{"id": "ticket-4", "input": "just wanted to say thanks!"}
```

The `input` is the prompt sent to your target; the `expected` is the true class.
A case with no `expected` (like `ticket-4`) is *unlabeled*. It still runs
and is still scored by your scorers, but it contributes to no classification
metric. Unlabeled cases are counted and reported separately so you can see how
much of the dataset the metrics actually cover.

## Turn it on

Classification is off by default. Opt in under `run`:

```yaml
targets:
  classifier:
    type: shell
    cmd: "cat"          # your real target: an openai-compatible or http endpoint
datasets:
  - file: cases.jsonl
scorers:
  - type: exact         # optional: also gate each labeled case on an exact match
run:
  classification: true
```

The classification aggregates are computed independently of your `scorers`. The
scorers decide each case's pass/fail, while `classification` reads `expected`
versus output directly. You do not need any particular scorer for the metrics to
appear (though `exact` pairs naturally with a label suite, passing a labeled case
exactly when its prediction is correct).

## Gate on the metrics

Two gates read the classification aggregates, so a suite can fail CI when the
model's accuracy or F1 drops below a floor. Declaring either gate turns the
aggregates on implicitly, so you don't also need `classification: true`:

```yaml
run:
  gates:
    - type: accuracy
      min: 0.9           # fraction of labeled cases predicted correctly
    - type: macro_f1
      min: 0.8           # macro-averaged F1 over the observed label set
```

Both floors take a `min` in `[0, 1]` and compare with the same `1e-9` tolerance
as every other gate, so a run that exactly meets its floor passes. They are
additive to the existing exit-code contract (see [Gates and
baselines](/guides/gates-and-baselines/)): the run exits non-zero if any
case fails **or** either metric falls below its floor.

## What the numbers mean

The label set is the set of `expected` labels observed in the dataset: the
true classes, and only those. A prediction the model invents that matches no
expected label is not a class of its own; it simply fails to match its case's
true class (lowering that class's recall) and enters no other class's tally.
Every metric is over the labeled cases:

- **Accuracy** is labeled cases predicted correctly, over all labeled cases. One
  number for the whole suite.
- **Precision** of class *c* is, of the cases predicted *c*, the fraction that
  were really *c* (`correct(c) / predicted-as-c`).
- **Recall** of class *c* is, of the cases that were really *c*, the fraction
  predicted *c* (`correct(c) / support(c)`, where support is how many labeled
  cases carry that class).
- **F1** of class *c* is the harmonic mean of its precision and recall.
- **Macro-F1** is the plain, unweighted mean of the per-class F1 scores. Every
  class counts the same regardless of how many cases it has, so a rare class the
  model ignores drags macro-F1 down as hard as a common one.

Any `0/0` in these ratios is defined as `0.0`, so a class no case predicted has
precision `0.0`, not an error or a blank. A target-error case that carries an
`expected` counts as labeled and wrong: it produced no output, so it matches no
class, and an error storm sinks accuracy exactly as it should.

For a run over the dataset above with a two-label version (`billing` /
`technical`) where the model gets two of three right, EvalCore reports:

```
classification: accuracy 0.67 · macro-F1 0.67 (3 labeled, 1 unlabeled)
```

with this per-class breakdown in the JSON and HTML reports:

```json
"classification": {
  "labeled_cases": 3,
  "unlabeled_cases": 1,
  "accuracy": 0.6666666666666666,
  "macro_f1": 0.6666666666666666,
  "per_class": [
    { "label": "billing",   "precision": 1.0, "recall": 0.5, "f1": 0.6666666666666666, "support": 2 },
    { "label": "technical", "precision": 0.5, "recall": 1.0, "f1": 0.6666666666666666, "support": 1 }
  ]
}
```

`per_class` is sorted by label, so the report is deterministic.

## Reading the terminal report

Here is a real run of a four-case suite (`cat` echoes the input as the
prediction, so the labels are exercised end to end) with `classification: true`
and both gates set to a `0.6` floor:

```
PASS ticket-1 (6ms)
FAIL ticket-2
     exact: expected "billing", got "technical"
PASS ticket-3 (6ms)
FAIL ticket-4
     exact: case "ticket-4" has no `expected` field and the scorer has no inline `value`

2 passed, 2 failed, 4 total
GATE PASS accuracy >= 0.6 (actual 0.67)
GATE PASS macro_f1 >= 0.6 (actual 0.67)
classification: accuracy 0.67 · macro-F1 0.67 (3 labeled, 1 unlabeled)
```

The per-case `PASS`/`FAIL` lines and the `GATE`/`classification` lines answer
different questions. The `exact` scorer fails `ticket-2` (a genuinely wrong
prediction) and `ticket-4` (an unlabeled case has nothing for `exact` to compare
against). That is the per-case contract. The classification line, by contrast,
counts `ticket-4` as unlabeled and reports accuracy `0.67` over the three labeled
cases. The metrics line always follows the gates block, and appears only when the
run computed classification (via `classification: true` or an `accuracy`/
`macro_f1` gate), so a suite that uses neither is byte-identical to before.

### Zero labeled cases fails loudly

If you configure an `accuracy` or `macro_f1` gate but no case carries an
`expected`, there is nothing to measure. Rather than pass vacuously, the metric
is `0.0` and the gate fails with an explicit reason:

```
2 passed, 0 failed, 2 total
GATE FAIL accuracy >= 0.6 (actual 0.00)
     no labeled cases
classification: accuracy 0.00 · macro-F1 0.00 (0 labeled, 2 unlabeled)
```

This catches the common mistake of gating on classification against a dataset
that never got its labels. The gate turns red instead of green.

## Labels are trimmed, then matched case-sensitively

A label matches its prediction when the two strings are **equal after trimming
surrounding whitespace**, and that is the *only* normalization v1 applies. So
`" billing "` and `billing` are the same class, but `Billing`, `billing.`, and
`billing (charged twice)` are three different classes from `billing`. Matching
is case-sensitive; there is no lowercasing, no punctuation stripping, no synonym
folding.

Real models rarely emit a bare label on their own. Constrain and normalize the
prediction before it reaches the metric, in whichever layer you already control:

- In the target. Prompt the model to answer with exactly one label and
  nothing else (`Reply with only one of: billing, technical, other.`), optionally
  with `params: { temperature: 0 }` and a tight `max_tokens`. For an [`http`
  target](/guides/evaluating-rest-apis/) wrapping your own classifier,
  return the normalized label from the endpoint and point `response_path` at it.
- In a scorer. A [`subprocess` scorer](/reference/subprocess-protocol/)
  can lowercase, strip, or map the output however you like and decide the
  per-case pass/fail. Note that the classification aggregates read the raw
  (trimmed) output, not a scorer's transformed view, so normalization that must
  reach accuracy/F1 belongs in the target's output itself.

:::caution[Known gap]
Under [`run.trials`](/guides/trials-and-statistics/) a case runs several
times, but classification is a single-prediction-per-case metric. The prediction
it scores is the case-level surfaced output, meaning the first successful trial,
not a vote across trials. Accuracy and F1 therefore describe one representative
prediction per case, while the trials machinery still measures how *often* the
case passes your scorers. Aggregating a label across trials (majority-vote
prediction) is intentionally out of scope for v1.
:::

## See also

- [Gates and baselines](/guides/gates-and-baselines/): how the
  `accuracy` and `macro_f1` gates fold into the exit code.
- [Configuration reference](/reference/configuration/#run-block): the
  `run.classification` flag and the classification gate schemas.
- [Trials and statistics](/guides/trials-and-statistics/): the
  multi-trial machinery the prediction limitation above refers to.
