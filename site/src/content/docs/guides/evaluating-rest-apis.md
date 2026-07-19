---
title: Evaluating REST APIs
description: "Point EvalCore at your own deployed app's HTTP/JSON endpoint: {{input}} substitution, response_path JSON Pointers, auth patterns, the headers-are-cached caveat, timeouts and retries, and keyless replay."
---

The `http` target points EvalCore at any HTTP/JSON API, typically your own
deployed RAG service or agent behind `POST /chat`, and caches it exactly like
an LLM call. The same commit-the-cassette, replay-in-CI story applies to your
app's real responses: record once, then gate every PR offline and free.

## The end-to-end example

Say your app answers questions at `POST /chat`, taking `{"question": "..."}` and
returning `{"answer": "..."}`. Here is the whole suite:

```yaml
targets:
  my-rag:
    type: http
    url: https://api.myapp.com/chat
    method: POST                       # default POST; GET/PUT/PATCH also supported
    headers:                           # static, NON-secret headers (values are cached!)
      x-tenant: acme
    api_key_env: MYAPP_API_KEY         # optional; -> authorization: Bearer <key>
    body:                              # JSON template; {{input}} fills string values
      question: "{{input}}"
      session: eval
    response_path: /answer             # RFC 6901 JSON Pointer; omit for the raw body

datasets:
  - file: cases.jsonl

scorers:
  - type: contains
    value: "30 days"
```

```jsonl
{"id": "refund", "input": "How long do refunds take?"}
```

Run it once with `--cache auto` to record, then `--cache replay` in CI. A live
localhost run of exactly this shape produces:

```
PASS refund (7ms)

1 passed, 0 failed, 1 total
```

Re-run offline with the server stopped and `--cache replay`. It replays the
recorded answer with no network and no key:

```
PASS refund (7ms)

1 passed, 0 failed, 1 total
```

## `{{input}}` substitution

Each case's `input` is substituted into the request in two places, with two
different encodings:

- Into `url`, **percent-encoded** (every non-alphanumeric byte), so it is
  safe in a query string or path segment:

  ```yaml
  url: "https://api.myapp.com/search?q={{input}}"
  ```

- Into every string value of `body`, **verbatim** (keys are never touched):

  ```yaml
  body:
    question: "{{input}}"          # -> the case input, unescaped, as a JSON string
    top_k: 5                       # non-string values pass through untouched
  ```

At least one of `url` or `body` must contain `{{input}}`. Otherwise every case
would send an identical request, which config validation rejects.

## Pulling the answer out: `response_path`

On a 2xx response, `response_path` is an RFC 6901 JSON Pointer into the JSON
body. It must start with `/`:

```yaml
response_path: /answer            # {"answer": "..."} -> the answer string
response_path: /data/0/text       # nested: {"data": [{"text": "..."}]}
```

Omit `response_path` to score the raw response body text, which is useful for
plain `text/plain` endpoints. A pointer that resolves to JSON `null` yields the literal
string `"null"`; only a pointer that resolves to *nothing* (an absent path) is an
error for that case.

## A GET example

Not every endpoint takes a body. For a search-style GET, put `{{input}}` in the
URL and drop `body` entirely (a GET may not carry one):

```yaml
targets:
  search:
    type: http
    url: "https://api.myapp.com/search?q={{input}}&limit=1"
    method: GET
    response_path: /results/0/snippet
```

## Auth patterns

Credentials come from an environment variable named by `api_key_env`, never
inline in YAML. The default sends the key as a bearer token:

```yaml
api_key_env: MYAPP_API_KEY         # -> authorization: Bearer <key>
```

For an `x-api-key`-style header, set both `auth_header` and an empty
`auth_prefix` (so no `Bearer ` is prepended):

```yaml
api_key_env: MYAPP_API_KEY
auth_header: x-api-key
auth_prefix: ""                    # send the raw key, no prefix
```

`auth_header` and `auth_prefix` require `api_key_env`; setting them without a key
is rejected. A static `headers:` entry that collides (case-insensitively) with the
auth header is also rejected, since it would send two conflicting header lines.

## The headers caveat: never put secrets in `headers:`

This is the one rule to internalize. **Header values enter the cache identity.**
They are hashed into the key and stored in the committed `.evalcore/cache.db`.
Anything you put in `headers:` is persisted in your repo's cassette.

- Non-secret routing headers (`x-tenant`, `x-region`, an API version) belong
  in `headers:`.
- Secrets (API keys, tokens) belong in `api_key_env`, which is excluded
  from the cache identity and never persisted.

```yaml
headers:
  x-tenant: acme                   # fine — not a secret
  # authorization: "Bearer sk-..." # NEVER — this would be committed in the cassette
api_key_env: MYAPP_API_KEY         # the right home for the credential
```

## Timeouts and retries

The `http` target shares the same deterministic retry and timeout policy as the
`openai-compatible` target:

```yaml
max_retries: 3                     # default 2; retries 429 / 5xx / transport errors
timeout_seconds: 30                # default 120; per attempt (each retry is fresh)
```

Transient failures (429, 5xx, network, or a timeout) retry with exponential
backoff honoring `Retry-After`. A timeout aborts the attempt and is treated like
any transient failure, so a hung endpoint can't wedge a run by pinning a
concurrency slot. Neither `max_retries` nor `timeout_seconds` is part of the
cache identity, so tuning them keeps existing cassettes valid.

## Cacheability and keyless replay

The cache identity is the request shape only:
`{url, method, headers, body, response_path}`. Secrets, `api_key_env`,
`auth_header`, `auth_prefix`, `max_retries`, and `timeout_seconds` are all
excluded. That is what lets `--cache replay` run offline with no key
configured. The identity doesn't depend on the credential, so CI replays the
committed cassette without secrets.

A replay cache miss (a request never recorded, usually because you changed the
`url`, `body`, `headers`, `response_path`, or a case input) fails that case with
a reason rather than calling out:

```
FAIL new
     target error: cache miss for case "new" in replay mode — record it first with --cache auto (or live)
```

:::caution[Known gap]
`http` targets do no token or cost accounting. Generic APIs have no standard
usage shape, so there is no `cost:` block and no `$` line for them. If you need
cost tracking, it lives on `openai-compatible` and `trace` targets; see
[Cost and budgets](/evalcore/guides/cost-and-budgets/).
:::

For the full field list, see the
[configuration reference](/evalcore/reference/configuration/); for cache
mechanics, [Record / replay](/evalcore/guides/record-replay/).

## See also

- [Configuration reference](/evalcore/reference/configuration/#http): the full
  `http` target field list, including the JSON Pointer response mapping.
- [Custom scorers](/evalcore/guides/custom-scorers/): grade an API's response
  with your own logic over stdin and stdout.
- [Record / replay](/evalcore/guides/record-replay/): how `http` responses are
  cached and replayed offline at $0.
- [Agents and traces](/evalcore/guides/agents-and-traces/): when your app emits
  a trace instead of a single response.
