# Security Policy

## Supported versions

EvalCore is pre-1.0. Security fixes land on the latest minor release; there are
no long-term support branches yet. Upgrade to the newest release before
reporting an issue, in case it is already fixed.

| Version | Supported |
|---|---|
| 0.7.x | Yes |
| < 0.7 | No, please upgrade |

## Reporting a vulnerability

**Do not open a public issue for a security vulnerability.**

Use GitHub's private vulnerability reporting:
[Report a vulnerability](https://github.com/eval-core/evalcore/security/advisories/new).
It creates a private thread visible only to the maintainers.

Include whatever you have: a description, the version, and ideally a minimal
`evals.yaml` that reproduces the problem. You can expect an acknowledgement
within a few days. If a report is confirmed, we will agree a disclosure timeline
with you and credit you in the advisory unless you prefer otherwise.

## What is in scope

Bugs where EvalCore does something a reasonable operator would not expect:

- Secrets leaking somewhere they should not be, including into the cache file,
  reports, or run history.
- `evalcore serve` becoming reachable from outside the local machine, or serving
  content that escapes HTML escaping.
- A crafted trace, dataset, response body, or JSON Schema causing memory unsafety
  or a crash in the runner.
- Cache poisoning: making a recorded response answer for a request that should
  have produced a different cache key.

## What is not a vulnerability

Two behaviors look alarming but are working as designed. Both are documented
here so a report does not spend your time.

**A config file executes commands.** `shell` targets and `subprocess` scorers run
the command you write in `evals.yaml`, by design, because that is how EvalCore
stays language-agnostic without an SDK. It follows that **an `evals.yaml` is executable
code**. Treat one from an untrusted source exactly as you would treat a
`Makefile` or a `package.json` with install scripts: read it before you run it,
and do not run suites from untrusted pull requests on a privileged runner.

**`evalcore serve` has no authentication.** It binds `127.0.0.1` and nothing
else, which is the entire security model. There is no remote access to
authenticate. Every route is read-only and `GET`-only. If you deliberately expose
it with a tunnel or a reverse proxy, authentication becomes your responsibility.

## Handling secrets

EvalCore is built so that credentials never reach disk, and the rules are worth
knowing because one of them is easy to get wrong.

- API keys are referenced by environment variable name (`api_key_env`), never
  written into YAML. They are resolved at run time and are excluded from every
  target's cache identity, so they never enter the cache file.
- **Do not put credentials in an `http` target's `headers:` block.** Header
  values are part of the cache identity, which means they are hashed into
  `.evalcore/cache.db`, a file people commit. Use `api_key_env` instead; set
  `auth_header` and `auth_prefix` if your API wants a header other than
  `authorization: Bearer …`. The config validator rejects a static header that
  collides with the auth header, but it cannot tell that an arbitrary header
  value is a secret.
- **The cache file contains request and response payloads.** Committing it is the
  recommended workflow, and it is what makes CI replay free and offline. But it
  means your prompts, your model's outputs, and any case inputs are stored in the
  repository in readable form. If your evaluation data is sensitive, keep the
  cache out of version control and record it in CI instead.
- Run history (`evalcore serve`) stores full run summaries, including case
  outputs, in the same file.

## Hardening notes

Two defaults are already locked down and are not configurable, deliberately:

- JSON Schema `$ref` resolution over the network and the filesystem is **compiled
  out** of the `json-schema` scorer. Validation cannot make a network request; an
  external `$ref` fails when the suite is built, not at scoring time.
- `evalcore serve` binds `127.0.0.1` with no bind-address option.

For CI, prefer `--cache replay`. A replay-only run never calls the network and
needs no API key configured at all, which is the smallest possible blast radius
for a job that runs on every pull request.
