# Quickstart: a mini bank support bot

The five-minute intro, and the fixture the CLI tests run against. A small support bot answers
bank-policy questions, and each answer is graded against the policy it must cite. It runs fully
offline: the target is a shell script (`bot.sh`) that returns canned, policy-grounded answers, so
there is no model and no network, yet it exercises the real scoring path.

## Run it

```sh
evalcore run examples/quickstart/evals.yaml
```

Exit code `0` means every answer cited a specific policy and the run cleared its 95% pass-rate
floor. This is the same suite the [Quickstart guide](https://evalcore.cc/getting-started/quickstart/)
walks through step by step.

## What the suite checks

- Grounding: a `contains` scorer requires the word "policy" in every answer, a cheap
  deterministic proxy for "cited its source".
- Specificity: a `regex` scorer (`policy [0-9.]+`) requires a concrete policy number, so the
  answer is auditable against a real rule.
- Groundedness judge (production path): a commented `judge` block grades each answer against
  its policy context with an LLM. It stays commented so the runnable suite is offline and
  deterministic; `evals.yaml` says exactly how to enable it.

## Swap in your real app

Replace the `shell` target with your live endpoint (an `openai-compatible` model or your own
`http` service) and the cases, context, scorers, and gate stay identical.
