---
title: Configuration reference
description: The complete evals.yaml schema. Every target, scorer, and run option, with fields, defaults, and the validation rules that reject bad configs.
---

The `evals.yaml` file is EvalCore's primary interface. This page documents every
key it accepts, exactly as the schema defines it. Unknown top-level keys and
unknown fields on most structs are rejected (`deny_unknown_fields`), so a typo
fails at parse time rather than being silently ignored.

:::note[Jump to a type]
**Targets:** [`shell`](#shell) · [`openai-compatible`](#openai-compatible) · [`anthropic`](#anthropic) · [`gemini`](#gemini) · [`http`](#http) · [`trace`](#trace)

**Scorers:** [`contains`](#contains) · [`exact`](#exact) · [`regex`](#regex) · [`subprocess`](#subprocess) · [`trajectory`](#trajectory) · [`judge`](#judge) · [`json-schema`](#json-schema) · [`similarity`](#similarity)

**Run block:** [Trials](#trials) · [Classification](#classification) · [Gates](#gates) · [Matrix](#matrix)
:::

## Top-level structure

```yaml
targets:   # map of name -> target; at least one required
datasets:  # list of dataset files; at least one required
scorers:   # list of scorers; at least one required
run:       # optional run options (concurrency, budget, gates)
```

| Key | Type | Required | Description |
|---|---|---|---|
| `targets` | map of name to [target](#targets) | yes | Named things to evaluate. `evalcore run --target <name>` selects one; with exactly one target the flag may be omitted. |
| `datasets` | list of [dataset](#datasets) | yes | JSONL files of cases, merged in list order. |
| `scorers` | list of [scorer](#scorers) | yes | Applied to every case's output. |
| `run` | [run block](#run-block) | no | Concurrency, budget, gates, and trials. Defaults apply when omitted. |

Validation rejects an empty `targets`, `datasets`, or `scorers` section with
`at least one target/dataset/scorer is required`.

## Targets

`targets` is a map from a name you choose to a target definition. Every target
carries a `type` discriminator (`shell`, `openai-compatible`, `anthropic`,
`gemini`, `http`, or `trace`). Names appear in `--target` and in error
messages.

### `shell`

Runs a shell command once per case: the case `input` is piped to the command's
stdin, and its stdout becomes the output. Local code, never cached.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"shell"` | yes | n/a | Selects this target. |
| `cmd` | string | yes | n/a | Command run via `sh -c`. Case input arrives on stdin; stdout is the output text. |

```yaml
targets:
  echo:
    type: shell
    cmd: "cat"
```

A non-zero exit becomes a failed case whose error carries the command's trimmed
stderr. A command that exits without reading stdin is tolerated (a broken-pipe
write is not treated as an error). Shell targets are never cached, because their
behavior can change without the config string changing. Their `cache_identity`
is `None` and they pass through every `--cache` mode unchanged.

### `openai-compatible`

POSTs to `{url}/chat/completions` in the OpenAI wire format. Any endpoint that
speaks that format works with no additional code.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"openai-compatible"` | yes | n/a | Selects this target. |
| `url` | string | yes | n/a | Base URL, e.g. `https://api.openai.com/v1`. `/chat/completions` is appended (a trailing slash is trimmed first). |
| `model` | string | yes | n/a | Model name sent in the request body. |
| `api_key_env` | string | no | none | Name of the environment variable holding the API key. Secrets never appear inline in YAML. The key is sent as `Authorization: Bearer <key>`. |
| `max_retries` | integer | no | `2` | Retries on transient failures (429 / 5xx / transport), with exponential backoff honoring `Retry-After`. |
| `timeout_seconds` | integer | no | `120` | Per-attempt total time budget (connect + reading the response body). Each retry gets a fresh budget. Must be at least 1. Since v0.5.0. |
| `cost` | [cost block](#cost-rates) | no | none | Token prices; enables per-case cost reporting and `run.budget_usd`. |
| `system` | string | no | none | System prompt prepended as a `system` message before each case's input. Since v0.4.0. |
| `params` | JSON object | no | none | Extra request-body fields passed through verbatim (`temperature`, `max_tokens`, `top_p`, …). Since v0.4.0. |

```yaml
targets:
  gpt:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-4.1-mini
    api_key_env: OPENAI_API_KEY
    max_retries: 5
    timeout_seconds: 90
    cost:
      input_per_1m: 0.40
      output_per_1m: 1.60
    system: "You are a terse support agent."
    params:
      temperature: 0
      max_tokens: 256
```

**Validation rules:**

- `timeout_seconds: 0` is rejected with `target "<name>": timeout_seconds must
  be at least 1` (the message names the target).
- `params` may not contain the reserved keys `model`, `messages`, or `stream`.
  Any of them is rejected: `params may not set "<key>" (model/messages are
  managed by EvalCore; streaming responses are unsupported)`. EvalCore sets
  `model` and `messages` itself; streaming responses are unsupported.
- If a `cost` block is present, `input_per_1m` and `output_per_1m` must be
  non-negative, else `target "<name>" has negative cost rates`.

The response text is read from `choices[0].message.content`; a missing field or
a non-JSON 200 body is a permanent (non-retried) case failure. Token usage is
captured from `usage.prompt_tokens` / `usage.completion_tokens` when present.

### `anthropic`

Since v0.8.0. POSTs to `{url}/messages` in the Anthropic Messages wire format.
A second wire format, not a second SDK — you still write only YAML. Retries,
timeouts, secret handling, and the record/replay cache work exactly as they do
for `openai-compatible`.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"anthropic"` | yes | n/a | Selects this target. |
| `url` | string | no | `https://api.anthropic.com/v1` | Base URL. `/messages` is appended (a trailing slash is trimmed first). Override for gateways, proxies, or the AWS endpoints (see [`auth: sigv4`](#auth-sigv4)). |
| `model` | string | yes | n/a | Model name sent in the request body. |
| `api_key_env` | string | no | none | Name of the environment variable holding the API key. Secrets never appear inline in YAML. The key is sent as `x-api-key: <key>`, and every request also carries `anthropic-version: 2023-06-01`. |
| `max_retries` | integer | no | `2` | Retries on transient failures (429 / 5xx / transport), with exponential backoff honoring `Retry-After`. |
| `timeout_seconds` | integer | no | `120` | Per-attempt total time budget (connect + reading the response body). Each retry gets a fresh budget. Must be at least 1. |
| `max_tokens` | integer | no | `4096` | Maximum tokens to sample. A first-class field rather than a `params` passthrough because the Messages API rejects a request without it. Must be at least 1. |
| `cost` | [cost block](#cost-rates) | no | none | Token prices; enables per-case cost reporting and `run.budget_usd`. |
| `system` | string | no | none | System prompt, sent as the **top-level** `system` body field. The Messages API has no `system` message role. |
| `params` | JSON object | no | none | Extra request-body fields passed through verbatim (`temperature`, `top_p`, `thinking`, …). |
| `auth` | `"api-key"` or `"sigv4"` | no | `"api-key"` | How requests authenticate. See [`auth: sigv4`](#auth-sigv4). |
| `dialect` | `"messages"` or `"bedrock-invoke"` | no | `"messages"` | Which deployment of the format the endpoint speaks. See [`dialect`](#dialect). |
| `aws` | AWS block | no | none | Region and credential env-var names. Required with `auth: sigv4`, rejected otherwise. |

```yaml
targets:
  claude:
    type: anthropic
    model: claude-opus-5
    api_key_env: ANTHROPIC_API_KEY
    max_tokens: 512
    cost:
      input_per_1m: 5.0
      output_per_1m: 25.0
    system: "You are a terse support agent."
    params:
      temperature: 0
```

**Validation rules:**

- `timeout_seconds: 0` and `max_tokens: 0` are rejected, each naming the target:
  `target "<name>": max_tokens must be at least 1`.
- `params` may not contain the reserved keys `model`, `messages`, `system`,
  `max_tokens`, or `stream`. `system` and `max_tokens` are reserved here (but
  not on `openai-compatible`) because the Messages API carries both as
  top-level body fields that EvalCore sets itself.
- If a `cost` block is present, `input_per_1m` and `output_per_1m` must be
  non-negative.

The response `content` is an **array of typed blocks**. EvalCore filters to
blocks with `type: "text"` and concatenates them in order, so a response that
leads with a `thinking` block parses correctly. A body with no text block, or a
non-JSON 200 body, is a permanent (non-retried) case failure. Token usage is
captured from `usage.input_tokens` / `usage.output_tokens`.

A policy decline arrives as **HTTP 200** with `stop_reason: "refusal"` and empty
content. That becomes a failed case whose reason names the refusal — not a parse
error blaming the response body — and it is never retried.

**AWS and gateways.** Any endpoint that speaks the Messages format works here;
which fields you set depends on how it authenticates.

| Endpoint | `auth` | `dialect` | Notes |
|---|---|---|---|
| api.anthropic.com, API-key gateways and proxies | `api-key` | `messages` | Both defaults — set only `url` and `api_key_env`. |
| Bedrock Mantle (`https://bedrock-mantle.{region}.api.aws/anthropic`) | `sigv4` | `messages` | Model IDs take an `anthropic.` prefix. |
| Legacy `bedrock-runtime` InvokeModel | `sigv4` | `bedrock-invoke` | Model moves into the path; `anthropic_version` replaces `model` in the body. |

Claude Platform on AWS (`https://aws-external-anthropic.{region}.api.aws/v1`)
is Messages-shaped and SigV4-signed with `service: aws-external-anthropic`, but
additionally requires a workspace identifier that this target does not yet
send — it is not supported today.

### `auth: sigv4`

Since v0.8.0. Signs requests with AWS Signature Version 4 instead of sending an
API key. An axis orthogonal to [`dialect`](#dialect): SigV4 signs a plain
Messages request exactly as readily as an InvokeModel one.

Credentials are **names of environment variables**, the same secret model as
`api_key_env`. EvalCore signs requests but deliberately does not implement the
AWS credential chain (profile files, IMDS, assumed roles): resolving those reads
the network and the clock at target-build time. Export the variables yourself.

| `aws` field | Type | Required | Default | Description |
|---|---|---|---|---|
| `region` | string | yes | n/a | Region the signature is scoped to, e.g. `us-east-1`. Must be non-empty. |
| `access_key_env` | string | no | `AWS_ACCESS_KEY_ID` | Environment variable holding the access key id. |
| `secret_key_env` | string | no | `AWS_SECRET_ACCESS_KEY` | Environment variable holding the secret access key. |
| `session_token_env` | string | no | `AWS_SESSION_TOKEN` | Environment variable holding the session token, for temporary credentials. Absent or unset means long-lived credentials. |
| `service` | string | no | `bedrock` | AWS service the signature is scoped to. |

```yaml
targets:
  bedrock:
    type: anthropic
    auth: sigv4
    dialect: bedrock-invoke
    url: https://bedrock-runtime.us-east-1.amazonaws.com
    model: anthropic.claude-opus-4-5-20251101-v1:0
    max_tokens: 512
    aws:
      region: us-east-1
```

**Validation rules:** `auth: sigv4` requires an `aws` block with a non-empty
`region`, rejects `api_key_env` (two credentials configured), and requires an
explicit `url` — the default is Anthropic's own API, which does not accept a
SigV4 signature. `auth: api-key` rejects an `aws` block, and rejects
`dialect: bedrock-invoke` (legacy bedrock-runtime accepts no API key).

Missing credentials fail at target-build time naming the variable, before any
case runs. Under `--cache replay` they resolve to empty and nothing is signed,
so CI can replay a committed cassette with no AWS credentials configured.

Signing reads the clock once per request, for the `X-Amz-Date` header. That
timestamp is never user-visible output and never enters a cache identity, so
determinism is untouched.

### `dialect`

Since v0.8.0. Which deployment of the Messages format the endpoint speaks.

| Value | Endpoint | Request |
|---|---|---|
| `messages` (default) | api.anthropic.com, gateways, Bedrock Mantle, Claude Platform on AWS | `POST {url}/messages`, `model` in the body, `anthropic-version` header |
| `bedrock-invoke` | Legacy AWS `bedrock-runtime` | `POST {url}/model/{model}/invoke` (the model is percent-encoded — Bedrock IDs contain `:`), `anthropic_version: bedrock-2023-05-31` in the body, no version header |

The dialect is part of the cache identity, since it changes both the path and
the body. `dialect: bedrock-invoke` requires `auth: sigv4`.

### `gemini`

Since v0.8.0. POSTs to `{url}/models/{model}:generateContent` in the Gemini wire
format. **Google also ships an OpenAI-compatible endpoint that works with
[`openai-compatible`](#openai-compatible) and no new code** — prefer that unless
you need a native-only feature such as `safetySettings` or thinking parts.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"gemini"` | yes | n/a | Selects this target. |
| `url` | string | no | `https://generativelanguage.googleapis.com/v1beta` | Base URL. `/models/{model}:generateContent` is appended. |
| `model` | string | yes | n/a | Model name. It rides in the URL path, not the body. |
| `api_key_env` | string | no | none | Name of the environment variable holding the API key, sent as `x-goog-api-key`. |
| `max_retries` | integer | no | `2` | Retries on transient failures (429 / 5xx / transport). |
| `timeout_seconds` | integer | no | `120` | Per-attempt total time budget. Must be at least 1. |
| `cost` | [cost block](#cost-rates) | no | none | Token prices; enables per-case cost reporting and `run.budget_usd`. |
| `system` | string | no | none | System prompt, sent as the top-level `systemInstruction` field. |
| `params` | JSON object | no | none | Extra request-body fields passed through verbatim. Generation tunables live under `generationConfig` in this API, so they nest. |

```yaml
targets:
  gemini:
    type: gemini
    model: gemini-2.5-pro
    api_key_env: GEMINI_API_KEY
    system: "You are a terse support agent."
    params:
      generationConfig:
        temperature: 0
        maxOutputTokens: 256
```

**The API key is never placed in the URL.** Gemini also accepts a `?key=` query
parameter; EvalCore always uses the `x-goog-api-key` header instead, because
request URLs are quoted in error messages and reports and a query-string secret
would leak into them.

**Validation rules:** `params` may not contain `model`, `contents`, or
`systemInstruction`; `timeout_seconds: 0` is rejected.

The answer is read from `candidates[0].content.parts[]`, filtered to parts that
carry `text` and are **not** flagged `thought: true` — a thought part is the
model's reasoning, not its answer. Token usage comes from
`usageMetadata.promptTokenCount` / `candidatesTokenCount`.

Two kinds of safety outcome arrive as **HTTP 200** and become failed cases with
readable reasons rather than parse errors: a blocked prompt
(`promptFeedback.blockReason`, no candidates) and a filtered candidate (a
non-`STOP` `finishReason` with no text part).

### `http`

Since v0.5.0. Calls an arbitrary HTTP/JSON endpoint, typically your own
deployed app's REST API, so it can be evaluated through the record/replay
cache like any LLM target. `{{input}}` is percent-encoded (every
non-alphanumeric byte) when substituted into `url`, and substituted verbatim
into every string **value** of the JSON `body` (object keys are never touched).

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"http"` | yes | n/a | Selects this target. |
| `url` | string | yes | n/a | Request URL. Must start with `http://` or `https://`. `{{input}}` is percent-encoded when substituted. |
| `method` | string | no | `"POST"` | One of `GET`, `POST`, `PUT`, `PATCH` (case-insensitive). A `GET` may not carry a `body`. |
| `headers` | map of string to string | no | none | Static headers, sent verbatim. Keys are matched case-insensitively. Never put secrets here: header values are hashed into the cache identity and persisted in the committed cache. |
| `api_key_env` | string | no | none | Name of the environment variable holding the API key. Sent in the auth header; never enters the cache. |
| `auth_header` | string | no | `authorization` | Header the key is sent in. Only valid alongside `api_key_env`. |
| `auth_prefix` | string | no | `"Bearer "` | Prefix prepended to the key. For an `x-api-key` style header, set `auth_header: x-api-key` and `auth_prefix: ""`. Only valid alongside `api_key_env`. |
| `max_retries` | integer | no | `2` | Retries on transient failures (429 / 5xx / transport), same deterministic backoff as `openai-compatible`. |
| `timeout_seconds` | integer | no | `120` | Per-attempt total time budget. Each retry gets a fresh budget. Must be at least 1. |
| `body` | JSON value | no | none | Request body template; `{{input}}` inside any string value is replaced with the case input. Omit to send no body. |
| `response_path` | string | no | none | RFC 6901 JSON Pointer (e.g. `/answer`) into the JSON response. Omitted: the raw response body text is the output. |

```yaml
targets:
  my-rag:
    type: http
    url: https://api.myapp.com/chat
    method: POST
    headers:
      x-tenant: acme
    api_key_env: MYAPP_API_KEY
    auth_header: authorization
    auth_prefix: "Bearer "
    max_retries: 3
    timeout_seconds: 30
    body:
      question: "{{input}}"
      session: eval
    response_path: /answer
```

**Validation rules** (each names the target):

- `url` must be non-empty and start with `http://` or `https://`, else `url
  must be a non-empty http:// or https:// URL`.
- `method` must be one of `GET, POST, PUT, PATCH`, else `method "<m>" is not one
  of GET, POST, PUT, PATCH`.
- A `GET` request may not carry a `body`: `a GET request may not carry a body`.
- Neither `url` nor any `body` string may omit `{{input}}`; at least one must
  contain it, else `neither url nor body contains {{input}}; every case would
  send the same request`.
- `auth_header` / `auth_prefix` require `api_key_env`, else `auth_header/
  auth_prefix require api_key_env`.
- When `api_key_env` is set, a `headers:` entry whose name matches the auth
  header (case-insensitively; default `authorization`) is rejected: `header
  "<name>" collides with the auth header … remove it from headers`. Otherwise
  two conflicting header lines would be sent.
- `response_path`, if present, must start with `/` (RFC 6901), else
  `response_path must be an RFC 6901 JSON Pointer starting with '/'`.
- `timeout_seconds: 0` is rejected with `timeout_seconds must be at least 1`.

A pointer that resolves to a JSON string yields that string; any other resolved
JSON value is serialized compactly (so `null` yields the literal `"null"`). A
pointer with **no** value at that path is a case error. `http` targets report no
tokens and therefore no cost in v1, because generic APIs have no standard usage
shape.

### `trace`

Since v0.3.0. Ingests recorded agent traces instead of invoking anything. Each
case names a trace file (via the case's `trace` field); the target reads it,
normalizes it (native trajectory format or OTel/OpenInference JSON export), and
outputs the canonical trajectory. Pair with the [`trajectory`](#trajectory)
scorer. See [Trajectory format](/reference/trajectory-format/) for the trace formats.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"trace"` | yes | n/a | Selects this target. |
| `cost` | [cost block](#cost-rates) | no | none | Token prices applied to usage extracted from trace spans; enables cost reporting and `run.budget_usd` for trace runs. Since v0.4.0. |

```yaml
targets:
  agent:
    type: trace
    cost:
      input_per_1m: 0.40
      output_per_1m: 1.60
```

The target's text output is the trace's final answer when the trace carries one
(so `judge`, `contains`, `regex`, and `exact` grade the answer); otherwise it is
the serialized trajectory JSON. The structured trajectory is always attached for
the `trajectory` scorer. Latency and token usage come from the trace itself.
`trace` targets are never cached (traces are local files).

### Cost rates

The `cost` block (on `openai-compatible`, `anthropic`, `gemini`, and `trace`
targets) declares USD prices per **one million** tokens. EvalCore ships no pricing table. Prices
change and differ per deployment, so they are config.

| Field | Type | Required | Description |
|---|---|---|---|
| `input_per_1m` | number | yes | USD per 1M input tokens. Must be non-negative. |
| `output_per_1m` | number | yes | USD per 1M output tokens. Must be non-negative. |

A negative rate is rejected: `target "<name>" has negative cost rates`.

## Datasets

`datasets` is a list of files. Each entry is an object with a single `file`
key. Files are merged in list order, and results stay in dataset order.

| Field | Type | Required | Description |
|---|---|---|---|
| `file` | path | yes | JSONL file of cases, resolved relative to the config file's directory. |

```yaml
datasets:
  - file: cases.jsonl
  - file: regressions/edge-cases.jsonl
```

### JSONL case format

Each non-blank line is one JSON object. Blank lines are skipped. Fields:

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | no | Case identifier used in reports. Missing ids default to `case-<line number>`. |
| `input` | string | for invoked targets | The prompt/input sent to the target. Defaults to `""` for trace cases. |
| `expected` | any JSON | no | Optional expectation, interpreted per-scorer (e.g. `exact` compares against it when no inline `value` is set). |
| `trace` | path | for `trace` targets | Path to the recorded trace file, resolved relative to the dataset file. |
| `context` | string, or list of strings | no | Retrieved RAG context the answer is graded against. A single string normalizes to a one-element list; an empty array (`[]`) normalizes to *none* (treated as absent). Scorers see it (the `judge` scorer grades against it, `subprocess` scorers receive it); targets never do. Since v0.6.0. |

Every case needs an `input` **or** a `trace`; a case with neither is an error:
`case at <file>:<line> has neither 'input' nor 'trace'`. A malformed line
reports `invalid case at <file>:<line>`. A `context` that is not a string or an
array of strings (a number, an object, a mixed array) is likewise a dataset
error naming the case's `file:line`.

**Case ids must be unique.** Since v0.7.0, a repeated `id`, across
all merged datasets, is rejected at load, naming both lines: `duplicate case id
"<id>" at <file>:<line> (first used at line <n>)`. Ids key baseline matching and
[matrix comparison](#matrix) rows, so a duplicate would silently collapse two
cases into one row; failing at load turns that into a clear error. Auto-filled
`case-<line number>` ids (cases with no explicit `id`) are unique by
construction.

```jsonl
{"id": "refund-1", "input": "How long do refunds take?", "expected": "30 days"}
{"input": "anonymous case gets id case-2"}
{"id": "rag-1", "input": "How long do refunds take?", "context": "Refunds are processed within 30 days."}
{"id": "rag-2", "input": "What do I need?", "context": ["Bring your order number.", "Keep the receipt."]}
{"id": "agent-flow", "trace": "traces/run1.json"}
```

See the [RAG evaluation guide](/guides/rag-evaluation/) for how context
flows to the scorers.

## Scorers

`scorers` is a list; every scorer runs on every case's output. Each carries a
`type` discriminator. The `type` tag is the name that appears in `Score.scorer`
and that a `mean_score` gate's `scorer` field references. An unknown `type` is a
parse error.

### `contains`

Passes if the output contains `value` as a substring.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"contains"` | yes | n/a | Selects this scorer. |
| `value` | string | yes | n/a | Substring the output must contain. |
| `case_sensitive` | bool | no | `true` | When false, the match ignores case. |

```yaml
scorers:
  - type: contains
    value: refund
    case_sensitive: true
```

### `exact`

Passes if the output equals `value`, or the case's `expected` field when
`value` is omitted.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"exact"` | yes | n/a | Selects this scorer. |
| `value` | string | no | none | The exact string to compare against. When omitted, the case's `expected` is used. |

```yaml
scorers:
  - type: exact          # compares against each case's `expected`
  - type: exact
    value: "yes"         # compares against this literal
```

### `regex`

Passes if the output matches the regular expression. The pattern is compiled
once at build time (a bad pattern fails fast, before any case runs).

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"regex"` | yes | Selects this scorer. |
| `pattern` | string | yes | Regular expression matched against the output. |

```yaml
scorers:
  - type: regex
    pattern: "^[A-Z]"
```

### `subprocess`

The any-language escape hatch. The command receives `{"input", "output",
"expected"}` as JSON on stdin and must print `{"score", "passed"?, "reason"?}`
on stdout. See [Subprocess protocol](/reference/subprocess-protocol/) for the full
contract.

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"subprocess"` | yes | Selects this scorer. |
| `cmd` | string | yes | Command run via `sh -c`. Must read stdin. |

```yaml
scorers:
  - type: subprocess
    cmd: python3 scorers/faithfulness.py
```

### `trajectory`

Since v0.3.0. Asserts on an agent trajectory (tool calls, ordering, step
budget). Requires a `trace` target, whose output is the normalized trajectory.
The scorer passes iff **all** its rules hold. See [Trajectory
format](/reference/trajectory-format/) for full rule semantics.

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"trajectory"` | yes | Selects this scorer. |
| `rules` | list of [rule](#trajectory-rules) | yes | Assertions; all must hold to pass. |

#### Trajectory rules

Rules are untagged; the distinctive required key selects the variant.

**`must_call`** holds when at least one step calls the named tool and satisfies
every `with` constraint:

| Field | Type | Required | Description |
|---|---|---|---|
| `must_call` | string | yes | Tool name that must be called. |
| `with` | map of field to [matcher](#field-matcher) | no | Argument constraints; all listed fields must match. |
| `after` | string | no | Only count calls strictly after the first call of this tool. If that tool never runs, the rule fails. |

**`must_not_call`** requires that the named tool is never called:

| Field | Type | Required | Description |
|---|---|---|---|
| `must_not_call` | string | yes | Tool name that must never be called. |
| `before` | string | no | Only consider calls before the first call of this tool. If that tool never runs, every call of the forbidden tool violates the rule (conservative). |

**`max_steps`** requires the trajectory to contain at most this many tool calls:

| Field | Type | Required | Description |
|---|---|---|---|
| `max_steps` | integer | yes | Maximum number of tool-call steps. |

```yaml
scorers:
  - type: trajectory
    rules:
      - must_call: search_kb
        with:
          query: { contains: "refund" }
      - must_call: issue_refund
        after: verify_identity
      - must_not_call: issue_refund
        before: verify_identity
      - max_steps: 8
```

#### Field matcher

A matcher constrains one argument field of a tool call. Both constraints may be
given; all present constraints must hold. A missing field never matches.

| Field | Type | Required | Description |
|---|---|---|---|
| `contains` | string | no | Substring match on the field rendered as a string (non-string fields compared against their compact JSON rendering). |
| `equals` | any JSON | no | Exact JSON equality. |

### `judge`

Since v0.2.0. LLM-as-judge: grade the output against a rubric using an
OpenAI-compatible or Anthropic Messages endpoint. Judge calls go through the
record/replay cache, so replayed verdicts are deterministic.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"judge"` | yes | n/a | Selects this scorer. |
| `url` | string | yes | n/a | Base URL, e.g. `https://api.openai.com/v1`. |
| `model` | string | yes | n/a | Judge model name. |
| `provider` | `"openai"`, `"anthropic"`, or `"gemini"` | no | `"openai"` | Wire format the endpoint speaks. `openai` POSTs to `{url}/chat/completions`; `anthropic` POSTs to `{url}/messages` with `max_tokens: 4096` (no judge-level knob); `gemini` POSTs to `{url}/models/{model}:generateContent`. All three use API-key auth — a SigV4-signed judge would need AWS config the judge schema does not carry, so point it at a gateway URL instead. Since v0.8.0. |
| `rubric` | string | yes | n/a | What the judge should assess, e.g. "Is the answer grounded in the provided context?". |
| `api_key_env` | string | no | none | Name of the environment variable holding the judge's API key. |
| `threshold` | number | no | `0.5` | Minimum score (0.0 to 1.0) to pass. |
| `cost` | [cost block](#cost-rates) | no | none | Token prices for the judge's own calls, priced for the judge model. Judge usage then folds into per-case cost, `total_cost_usd`, and `run.budget_usd`. See [LLM-as-judge](/guides/llm-as-judge/#judge-tokens-and-cost-count-toward-totals-and-budgets). |

```yaml
scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: gpt-4o
    rubric: "Is the answer grounded in the provided context?"
    api_key_env: OPENAI_API_KEY
    threshold: 0.7
  - type: judge
    provider: anthropic
    url: https://api.anthropic.com/v1
    model: claude-opus-5
    rubric: "Does the answer state a concrete refund window in days?"
    api_key_env: ANTHROPIC_API_KEY
```

Omitting `provider` keeps a judge on the OpenAI wire format, so configs written
before the field existed parse — and cache-key — identically. Switching a judge
to `anthropic` changes its cache identity, so its verdicts re-record rather than
replaying the other provider's answer.

### `json-schema`

Since v0.7.0. Passes iff the output parses as JSON **and** validates
against a JSON Schema (draft 2020-12). Non-JSON output is a failing score with a
reason, never an error. The schema file is read and compiled once in the factory,
so a bad or unreadable schema fails the whole run before any case executes (the
error names the file).

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"json-schema"` | yes | Selects this scorer. |
| `schema` | path | yes | Path to the JSON Schema file, resolved relative to the config file. Must be non-empty. |

```yaml
scorers:
  - type: json-schema
    schema: schemas/reply.json
```

A failing score names up to three violations by their JSON pointer, in a
deterministic (sorted) order, e.g. `/age: -1 is less than the minimum of 0;
/name: 42 is not of type "string"`.

**Offline by design.** Remote `$ref` resolution is compiled out, so validation
never touches the network. An external `$ref` (`http(s)://…`) is unresolvable
and fails at construction time, not per case and never over the wire, so
validation stays deterministic and air-gapped.

**Validation rules:** an empty `schema` path is rejected with `json-schema
scorer: schema path must be non-empty`. File existence and schema compilation
are checked in the factory (a missing file, non-JSON schema, invalid schema, or
unresolvable remote `$ref` each fails the run, naming the file).

### `similarity`

Since v0.7.0. Embeds the case's `expected` answer and the output
through an OpenAI-compatible `/embeddings` endpoint and passes iff their **cosine
similarity** is at least `threshold`. Embedding calls go through the
record/replay cache, so replayed scores are deterministic, offline, and keyless.
See the [Semantic similarity guide](/guides/semantic-similarity/).

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `"similarity"` | yes | n/a | Selects this scorer. |
| `url` | string | yes | n/a | Base URL of the OpenAI-compatible embeddings API, e.g. `https://api.openai.com/v1`. `/embeddings` is appended. Must be non-empty. |
| `model` | string | yes | n/a | Embedding model name, e.g. `text-embedding-3-small`. |
| `api_key_env` | string | no | none | Name of the environment variable holding the API key. Secrets never appear inline in YAML; the key never enters the cache. |
| `threshold` | number | no | `0.8` | Minimum cosine similarity to pass. A finite value in `[-1, 1]`. |

```yaml
scorers:
  - type: similarity
    url: https://api.openai.com/v1
    model: text-embedding-3-small
    api_key_env: OPENAI_API_KEY
    threshold: 0.8
```

The reported `value` is the raw cosine similarity in `[-1, 1]` (it may be
negative and is not clamped); `passed` is `value >= threshold` with a `1e-9`
tolerance. A case with no `expected` is a failing score with a reason (the
scorer needs a reference to embed against), never an error.

**Validation rules:** an empty `url` is rejected with `similarity scorer: url
must be non-empty`. A `threshold` that is not finite or falls outside `[-1, 1]`
is rejected: `similarity scorer: threshold must be within [-1, 1], got <n>`.

## Run block

The `run` block is optional; every field has a default.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `concurrency` | integer | no | `4` | Maximum in-flight cases. Must be at least 1. |
| `budget_usd` | number | no | none | Abort scheduling new cases once accumulated cost reaches this (USD). Requires the target to declare `cost` rates. Must be positive. |
| `gates` | list of [gate](#gates) | no | `[]` | Suite-level aggregate acceptance criteria. Evaluated in list order. |
| `trials` | integer or [trials block](#trials) | no | `1` | Run each case N times and aggregate. Since v0.7.0. |
| `classification` | bool | no | `false` | Compute [classification aggregates](#classification) (accuracy, macro-F1, per-class metrics) over labeled cases. Since v0.7.0. |
| `matrix` | list of target names | no | none | Run the whole suite once per named target and print a side-by-side [comparison](#matrix). At least two distinct names, each defined in `targets`. Since v0.7.0. |
| `history` | bool | no | `true` | Append one run-history row per executed run (a matrix records one per arm) to `.evalcore/cache.db`, so [`evalcore serve`](/guides/run-history-and-serve/) can list and diff past runs. `--no-history` overrides to false. This is metadata only: the exit code, report bytes, and cache keys are identical either way, and a write failure is a warning, never a run failure. Since v0.7.0. |

```yaml
run:
  concurrency: 8
  budget_usd: 5.0
  trials: 3
  gates:
    - type: pass_rate
      min: 0.95
    - type: mean_score
      min: 0.5
    - type: mean_score
      scorer: judge
      min: 0.8
```

**Validation rules:**

- `concurrency: 0` is rejected with `run.concurrency must be at least 1`.
- `budget_usd` must be positive, else `run.budget_usd must be positive, got
  <n>`. Costing is done from token usage, so replayed runs count their recorded
  (virtual) cost too. When the budget is exhausted, remaining cases fail with a
  reason rather than aborting the run mid-flight.
- `trials` count must be at least 1, else `run.trials count must be at least 1`.
- `matrix`, if present, must list at least two distinct target names, each
  defined in `targets`. Otherwise: `run.matrix must list at least two targets,
  got <n>`, `run.matrix lists target "<name>" more than once`, or `run.matrix
  target "<name>" is not defined; available: <names>`.

### Trials

Since v0.7.0. Runs every case `count` times and folds the per-trial
verdicts into one case verdict. Accepts an **integer shorthand** (`trials: 3`,
meaning `require: all`) or the full `{ count, require }` map. Absent, or
`trials: 1`, means one trial with `require: all`, byte-identical to a run with
no trials configured. See the [Trials and statistics
guide](/guides/trials-and-statistics/) for aggregation and cache semantics.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `count` | integer | yes | n/a | Number of trials per case. Must be at least 1. |
| `require` | string | no | `all` | Fold policy from per-trial verdicts to the case verdict: `all` (every trial passes), `majority` (strictly more than half pass), or `any` (at least one passes). |

```yaml
run:
  trials: 3                # shorthand: 3 trials, require: all
# or the full form:
run:
  trials:
    count: 5
    require: majority
```

A trial passes when every scorer passes for that trial. The case-level score for
each scorer is the **mean** of that scorer's value across the trials (what
`mean_score` gates and baselines see), the case latency is the trial mean, and
the cost is the sum of every trial (counting toward `budget_usd`). An unknown
`require` value is a parse error. Trial 0 keeps the pre-trials cache key so
existing cassettes replay; trials 1..N re-key with the trial index, and judge
and similarity calls re-key per trial the same way.

### Classification

Since v0.7.0. `classification: true` computes label-prediction
metrics over the **labeled** cases (those with an `expected` field): accuracy,
macro-averaged F1, and a per-class precision/recall/F1/support table. Off by
default; also turned on implicitly by an [`accuracy` or `macro_f1`
gate](#gates), which needs the metrics. See the [Classification
guide](/guides/classification/) for a worked example.

```yaml
run:
  classification: true
```

- A case's **label** is its trimmed `expected`; its **prediction** is its trimmed
  output. Matching is exact and **case-sensitive**: v1 normalizes with `.trim()`
  and nothing more (normalize in your target or a scorer).
- The class set is the observed **expected** labels only. A prediction matching
  no expected label is a false negative for its true class and enters no other
  class's tally. Every `0/0` ratio is defined as `0.0`.
- Macro-F1 is the **unweighted** mean of the per-class F1 scores (every class
  counts equally, regardless of support).
- A **target-error** case with `expected` counts as labeled-and-wrong. It
  produced no output, so it matches no class, and an error storm sinks accuracy.
- Multi-trial runs (v1 limitation): the prediction is the case-level surfaced
  output (the first successful trial), not a vote across trials.

The terminal prints one line after the gates block when the run computed
classification: `classification: accuracy 0.67 · macro-F1 0.67 (3 labeled, 1
unlabeled)`. The per-class table rides the JSON and HTML reports. Absent (the
default), reporter output and the serialized `RunSummary` are byte-identical to a
run with no classification.

### Gates

Since v0.5.0. Gates express CI acceptance criteria over the whole run rather
than per case. They are checked after every case runs and are **additive** to
the existing contracts: a run exits non-zero if any case fails (or, with
`--baseline`, regresses) **or** any gate falls below its floor. Floors compare
with a `1e-9` absolute tolerance, so a run that exactly meets its floor is not
failed by floating-point rounding. Empty by default; evaluated in list order.

**`pass_rate`**, the fraction of cases passing every scorer:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"pass_rate"` | yes | Selects this gate. |
| `min` | number | yes | Minimum passing fraction, within `[0, 1]`. |

Target-error cases count in the denominator (failures are data), so an error
storm sinks this gate. An out-of-range `min` is rejected: `pass_rate min must be
within [0, 1], got <n>`.

**`mean_score`**, the mean of scorer `value`:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"mean_score"` | yes | Selects this gate. |
| `min` | number | yes | Minimum mean score. Any finite `f64` (subprocess scorers may use arbitrary scales). |
| `scorer` | string | no | Restrict the mean to one scorer type (a config `type:` tag, e.g. `judge`, `contains`). Omitted: average across all scorers. |

`min` must be finite, else `mean_score min must be finite, got <n>`. A `scorer`
naming no configured scorer is rejected as a typo: `mean_score scorer "<name>"
is not among the configured scorers`. Cases whose target errored produce no
scores, so they contribute nothing to the mean. Pair a `mean_score` gate with a
`pass_rate` gate to catch error storms that would otherwise leave a high mean
intact.

**`accuracy`**, the fraction of labeled cases predicted correctly:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"accuracy"` | yes | Selects this gate. |
| `min` | number | yes | Minimum accuracy, within `[0, 1]`. |

Since v0.7.0. Reads the run's [classification aggregates](#classification)
and turns them on implicitly. An out-of-range `min` is rejected: `accuracy min
must be within [0, 1], got <n>`. A run with **zero labeled cases** scores `0.0`
and fails with a `no labeled cases` reason rather than passing vacuously.

**`macro_f1`**, macro-averaged F1 over the observed (expected) label set:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | `"macro_f1"` | yes | Selects this gate. |
| `min` | number | yes | Minimum macro-F1, within `[0, 1]`. |

Since v0.7.0. Like `accuracy`, reads the classification aggregates,
turns them on implicitly, and fails loudly on zero labeled cases. An out-of-range
`min` is rejected: `macro_f1 min must be within [0, 1], got <n>`.

```yaml
run:
  gates:
    - type: accuracy
      min: 0.9
    - type: macro_f1
      min: 0.8
```

Gate outcomes print after the summary and ride along in the JSON and HTML
reports; JUnit output is unchanged (the exit code carries the gate result). See
the [CLI reference](/reference/cli/) for how gates fold into the exit code.

### Matrix

Since v0.7.0. `run.matrix: [name, name, …]` runs the whole suite
**once per named target**, in the listed order, and prints a side-by-side
comparison (model A vs B, or prompt v1 vs v2) from one invocation. Requires at
least two distinct names, each defined in `targets`. Absent (the default): a single-target
run, byte-identical to before. `--matrix name,name` on the CLI overrides this;
combining a matrix with `--target`, `--baseline`, or `--save-baseline` is an
error. See the [Comparing models guide](/guides/comparing-models/) for a
worked example.

```yaml
run:
  matrix: [gpt, claude]
```

- Arms run **sequentially, in list order** (not the alphabetical map order), so
  behavior is deterministic and predictable against rate limits. Within an arm,
  concurrency, gates, classification, and trials behave exactly as a single run
  does.
- Each arm prices with its own target's [`cost`](#cost-rates) rates, and
  `run.budget_usd` applies **per arm** (each arm gets the full budget).
- The exit code is `0` iff **every** arm passes all its cases *and* every gate
  holds, else `1`.
- The per-case **winner** in the comparison table is the arm with the strictly
  highest mean case score (mean of that case's scorer values), with a `1e-9` tie
  tolerance; cases where the top arms tie, or where no arm produced a score, are
  ties for any number of arms.
- One cassette store is shared across arms; each arm has its own cache identity,
  so `--cache replay` works per arm. Non-matrix runs take the unchanged
  single-target path.

## See also

- [Core concepts](/getting-started/core-concepts/): what each block is for,
  before you read its fields.
- [CLI reference](/reference/cli/): the flags that override these fields at
  run time.
- [Custom scorers](/guides/custom-scorers/): writing the command a
  [`subprocess`](#subprocess) scorer points at.
- [Gates and baselines](/guides/gates-and-baselines/): the pass/fail
  semantics behind the [`gates`](#gates) block.
- [Cache and determinism](/reference/cache-and-determinism/): which of these
  target fields end up in the cache key.
