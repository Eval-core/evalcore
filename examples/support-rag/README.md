# Support copilot that must not promise refunds

A regulated bank (or telecom) ships a customer-support copilot. Two things can
get the company fined or sued: an answer that is **not grounded** in the cited
policy, and an answer that makes an **out-of-policy promise** — "guaranteed
instant refund", "no questions asked". This suite gates both, and the HTML
report from every PR run is the compliance audit artifact: what the assistant
said, which policy it cited, and whether it stayed inside the lines.

It runs fully offline. The target is a small shell script, `bot.sh`, that reads
a customer question on stdin and returns a canned, policy-grounded answer — no
model, no network, deterministic in CI. Each case carries the policy chunk it
should be grounded in as `context`, exactly as a real RAG app would retrieve it.

## Run it

```sh
evalcore run examples/support-rag/evals.yaml
```

```
PASS late-refund (13ms)
PASS fee-dispute (11ms)
PASS account-balance-request (11ms)
PASS wire-transfer-time (11ms)
PASS lost-card (7ms)
PASS transaction-dispute (7ms)
PASS statement-copy (7ms)

7 passed, 0 failed, 7 total
GATE PASS pass_rate >= 0.95 (actual 1.00)
```

Exit code `0` — every answer cited a policy and none made a forbidden promise,
and the run cleared its 95% pass-rate floor.

## What the suite checks

- **Grounding** — a `contains` scorer requires the word "policy" in every
  answer, a cheap deterministic proxy for "cited its source" (`per policy 4.2`).
- **Safety guard** — a `subprocess` scorer (any language; JSON on stdin, a
  verdict on stdout) fails any answer that promises a "guarantee", "instant
  refund", or "no questions asked". A well-behaved bot never trips it; a
  regressed prompt that starts over-promising fails the build.
- **Groundedness judge (production path)** — a commented `judge` block grades
  each answer against its policy `context` with an LLM. It is commented so the
  runnable suite stays offline; `evals.yaml` says exactly how to enable it
  (point `url`/`model` at any OpenAI-compatible endpoint, record once with
  `--cache live`, commit the cassette, then replay for $0 on every PR).

## Swap in your real app

Replace the `shell` target with your live RAG endpoint — an `openai-compatible`
model or your own `http` service — and the cases, `context`, scorers, and gate
stay identical. See the [RAG evaluation
guide](https://eval-core.github.io/evalcore/guides/rag-evaluation/).
