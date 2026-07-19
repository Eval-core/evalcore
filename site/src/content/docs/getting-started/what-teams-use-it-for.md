---
title: What teams use it for
description: Five real scenarios EvalCore is built for — a regulated support copilot, a model migration, an expensive judge suite made free, claims-triage classification, and an agent that touches money.
---

EvalCore is one small binary, but the same three moves — describe a suite as
data, record it once, replay it in CI forever — cover very different high-stakes
jobs. Here are five, each with the smallest fragment that makes the point and a
link to the full guide.

## The support copilot that must not promise refunds

A regulated bank or telecom ships a support copilot. Every answer has to be
grounded in the cited policy and must never make an out-of-policy promise
("guaranteed instant refund"). A `contains` check enforces grounding, a
`subprocess` guard blocks the forbidden promise, and the HTML report from each
PR run becomes the compliance audit artifact — what was said, which policy it
cited, and whether it stayed inside the lines.

```yaml
scorers:
  - type: contains          # grounded: the answer cites its policy
    value: "policy"
  - type: subprocess        # guard: no out-of-policy promise
    cmd: python3 guard.py
```

Full example: [`examples/support-rag/`](https://github.com/eval-core/evalcore/tree/main/examples/support-rag).
Guide: [RAG evaluation](/evalcore/guides/rag-evaluation/).

## Is the 10x-cheaper model good enough?

Swapping to a cheaper model is a six-figure decision usually made on vibes. Run
both as a matrix and decide on evidence: one invocation runs the whole suite
against each model and prints a side-by-side comparison with per-case winners
and each target's own cost.

```sh
evalcore run evals.yaml --matrix expensive,cheap
```

Guide: [Comparing models](/evalcore/guides/comparing-models/).

## The judge suite too expensive to run per-PR

An LLM-graded suite is the most honest signal you have and the one you run least,
because every PR would cost real money and give slightly different answers.
Record the judge verdicts once and commit the cassette; from then on the PR path
replays them — **$0 and byte-for-byte deterministic** — so the suite moves from
a weekly job to a blocking check on every merge.

```sh
evalcore run evals.yaml --cache live     # record once, commit .evalcore/cache.db
evalcore run evals.yaml --cache replay   # every PR: offline, free, deterministic
```

Guides: [Record / replay](/evalcore/guides/record-replay/) · [Running in CI](/evalcore/guides/running-in-ci/).

## Claims triage that catches a silent fraud-recall drop

An insurer routes each claim to a queue. The dangerous regression is quiet: the
model gets a little worse at spotting fraud, overall accuracy barely moves, and
staged claims start getting paid. `accuracy` and `macro_f1` gates fail the build
when the metric that matters slips, and every report carries per-class precision
and recall so the drop is visible.

```yaml
run:
  classification: true
  gates:
    - { type: accuracy, min: 0.8 }
    - { type: macro_f1, min: 0.7 }
```

Full example: [`examples/claims-triage/`](https://github.com/eval-core/evalcore/tree/main/examples/claims-triage).
Guide: [Classification](/evalcore/guides/classification/).

## The agent that touches money

An agent that can issue refunds has a policy: never refund before verifying
identity, and never loop forever. A prompt "please verify first" is a hope;
`trajectory` rules over the agent's OTel trace make it a CI assertion — checked
on what the agent actually did, in any language, with no SDK.

```yaml
scorers:
  - type: trajectory
    rules:
      - must_not_call: issue_refund
        before: verify_identity
      - max_steps: 8
```

Guide: [Agents and traces](/evalcore/guides/agents-and-traces/).

## What ties them together

Every one of these runs **local-first** — the suite, the cassette, and the run
history live in a SQLite file next to your repo. Nothing leaves the building,
which is the difference between an eval tool that clears procurement and a SaaS
that does not. Targets speak HTTP or shell and scorers speak JSON over
stdin/stdout, so your app can be written in **any language**. And because every
LLM call is recorded and replayed, the evidence is **deterministic**: the same
inputs produce the same report every time, which is exactly what an audit — or a
six-figure model decision — needs it to be.
