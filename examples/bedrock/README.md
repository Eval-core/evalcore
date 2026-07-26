# Claude on AWS: SigV4-signed targets

Three ways to reach Claude on AWS, all through the same `anthropic` target type. Pick one with
`--target`.

## The two axes

`auth` and `dialect` are independent, which is why neither gets its own target type:

| Endpoint | `auth` | `dialect` | Shape |
|---|---|---|---|
| api.anthropic.com, API-key gateways | `api-key` (default) | `messages` (default) | `POST {url}/messages` |
| Bedrock Mantle, Claude Platform on AWS | `sigv4` | `messages` (default) | `POST {url}/messages` |
| Legacy `bedrock-runtime` InvokeModel | `sigv4` | `bedrock-invoke` | `POST {url}/model/{id}/invoke` |

The `bedrock-invoke` dialect moves the model into the path and replaces the body's `model` field
with `anthropic_version: bedrock-2023-05-31`. Everything else — retries, timeouts, the
record/replay cache, cost accounting — is identical across all three.

## Credentials

Credentials are **names of environment variables**, exactly like `api_key_env`:

```yaml
aws:
  region: us-east-1
  access_key_env: AWS_ACCESS_KEY_ID       # default
  secret_key_env: AWS_SECRET_ACCESS_KEY   # default
  session_token_env: AWS_SESSION_TOKEN    # default; optional, for temporary credentials
  service: bedrock                        # default
```

EvalCore signs requests but deliberately does **not** implement the AWS credential chain
(profile files, IMDS, assumed roles). Resolving those reads the network and the clock at
target-build time, which is in tension with the determinism rule and with the "secrets are named
environment variables" model. Export the variables yourself, or have your CI do it.

Signing itself reads the clock once per request, for the `X-Amz-Date` header. That timestamp is
never user-visible output and never enters a cache identity, so determinism is untouched — the
signing function is pure given the timestamp, and is tested that way.

## Run it

```sh
evalcore run examples/bedrock/evals.yaml --target bedrock-invoke --cache live
evalcore run examples/bedrock/evals.yaml --target bedrock-mantle --cache live
evalcore run examples/bedrock/evals.yaml --cache replay --target bedrock-invoke   # offline, $0
```

Replay-only runs build without any AWS credentials at all, so CI can replay a committed cassette
with nothing configured.

## Known gap: Claude Platform on AWS

Claude Platform on AWS (`https://aws-external-anthropic.{region}.api.aws/v1`) is Messages-shaped
and SigV4-signed, so it fits the `auth: sigv4` + `dialect: messages` combination with
`service: aws-external-anthropic`. It additionally requires a workspace identifier that this
target does not yet send, so it is **not** supported today. Bedrock Mantle and legacy
`bedrock-runtime` both work.
