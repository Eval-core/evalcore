# Live model calls: Gemini target

Evaluates Gemini through the `gemini` target — the native `generateContent` format. It's the
third wire format EvalCore speaks, and the one that breaks every assumption the first two
shared.

## Run it

Needs `GEMINI_API_KEY` in the environment.

```sh
evalcore run examples/gemini/evals.yaml --cache live   # records responses into .evalcore/cache.db
evalcore run examples/gemini/evals.yaml --cache replay # re-runs offline, for $0
```

## Prefer the OpenAI-compatible endpoint when you can

Google ships an OpenAI-compatible endpoint that works with `type: openai-compatible` and no new
code. Reach for `type: gemini` only when you need something native-only — `safetySettings`,
`systemInstruction`, or thinking parts. Two targets pointed at the same model over different
wire formats never share cassettes, so you can run both and compare.

## What's different

- The model rides in the **URL path**, after a colon: `/models/{model}:generateContent`.
- The request is `contents[].parts[]`, not `messages[]`, and the system prompt is a
  `systemInstruction` *Content* value rather than a string.
- Generation tunables nest under `generationConfig`, so `params` nests too. EvalCore passes the
  structure through verbatim instead of inventing a translation layer — the same rule that keeps
  every other provider param a passthrough.
- The API key goes in the `x-goog-api-key` **header**. The API also accepts a `?key=` query
  parameter; EvalCore never uses it, because the request URL is quoted in error messages and
  reports, and a query-string secret would leak straight into them.
- Responses are `candidates[0].content.parts[]`. Parts flagged `thought: true` are the model's
  reasoning, not its answer, so they're skipped.
- A safety block arrives as HTTP 200 — either `promptFeedback.blockReason` with no candidates, or
  a candidate with a non-`STOP` `finishReason`. Both become failed cases with readable reasons.
