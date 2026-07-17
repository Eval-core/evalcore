//! Targets: the things being evaluated.
//!
//! Design rule: targets are protocol-shaped (shell command, OpenAI-compatible
//! HTTP), not vendor SDKs. A vendor that speaks the OpenAI wire format needs
//! zero new code here.

use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use async_trait::async_trait;
use evalcore_config::TargetConfig;
use tokio::io::AsyncWriteExt;

use crate::types::{TargetOutput, TestCase, TokenUsage};

#[async_trait]
pub trait Target: Send + Sync {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput>;

    /// Stable, canonical description of what this target calls, used as part
    /// of the record/replay cache key. `None` (the default) marks calls as
    /// uncacheable — correct for local code like shell targets, whose behavior
    /// can change without the config string changing. Secrets must never
    /// appear in the identity.
    fn cache_identity(&self) -> Option<serde_json::Value> {
        None
    }
}

/// How target factories resolve secrets from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretPolicy {
    /// Missing env vars are build errors — fail fast before any case runs.
    /// Correct for any mode that may call the network.
    Require,
    /// Missing env vars resolve to no key. Only safe for replay-only runs,
    /// which never invoke the live target — this is what lets CI replay a
    /// committed cache with no API keys configured at all.
    Optional,
}

/// Build a runnable target from its config, requiring all secrets to be
/// present (see [`build_target_with`] for replay-mode leniency).
pub fn build_target(config: &TargetConfig) -> anyhow::Result<Box<dyn Target>> {
    build_target_with(config, SecretPolicy::Require)
}

pub fn build_target_with(
    config: &TargetConfig,
    secrets: SecretPolicy,
) -> anyhow::Result<Box<dyn Target>> {
    match config {
        TargetConfig::Shell { cmd } => Ok(Box::new(ShellTarget::new(cmd.clone()))),
        TargetConfig::Trace { cost: _ } => Ok(Box::new(TraceTarget)),
        TargetConfig::OpenaiCompatible {
            url,
            model,
            api_key_env,
            max_retries,
            timeout_seconds,
            cost: _,
            system,
            params,
        } => {
            let api_key = resolve_api_key(api_key_env, secrets)?;
            Ok(Box::new(
                OpenAiCompatTarget::new(url.clone(), model.clone(), api_key, *timeout_seconds)?
                    .with_max_retries(*max_retries)
                    .with_system(system.clone())
                    .with_params(params.clone()),
            ))
        }
        TargetConfig::Http {
            url,
            method,
            headers,
            api_key_env,
            auth_header,
            auth_prefix,
            max_retries,
            timeout_seconds,
            body,
            response_path,
        } => {
            let api_key = resolve_api_key(api_key_env, secrets)?;
            let mut target = crate::http_target::HttpTarget::new(
                url.clone(),
                crate::http_target::parse_method(method)?,
                *timeout_seconds,
            )?
            .with_max_retries(*max_retries)
            .with_api_key(api_key)
            .with_body(body.clone())
            .with_response_path(response_path.clone());
            if let Some(headers) = headers {
                target = target.with_headers(headers.clone());
            }
            if let Some(auth_header) = auth_header {
                target = target.with_auth_header(auth_header.clone());
            }
            if let Some(auth_prefix) = auth_prefix {
                target = target.with_auth_prefix(auth_prefix.clone());
            }
            Ok(Box::new(target))
        }
    }
}

/// Runs a shell command per case: input on stdin, stdout is the output.
pub struct ShellTarget {
    cmd: String,
}

impl ShellTarget {
    pub fn new(cmd: String) -> Self {
        Self { cmd }
    }
}

#[async_trait]
impl Target for ShellTarget {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        let start = Instant::now();
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&self.cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn shell target: {}", self.cmd))?;

        {
            let mut stdin = child.stdin.take().context("shell target has no stdin")?;
            // A command that exits without reading stdin (or without reading
            // all of it) is legitimate — EPIPE here must not mask the child's
            // real exit status/stderr. Any other write error still propagates.
            let write_result = async {
                stdin.write_all(case.input.as_bytes()).await?;
                stdin.shutdown().await
            }
            .await;
            if let Err(err) = write_result {
                if err.kind() != std::io::ErrorKind::BrokenPipe {
                    return Err(err).context("failed writing input to shell target");
                }
            }
        }

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            bail!(
                "shell target exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(TargetOutput {
            text: String::from_utf8_lossy(&output.stdout).into_owned(),
            latency_ms: start.elapsed().as_millis() as u64,
            tokens: None,
        })
    }
}

/// Ingests recorded agent traces instead of invoking anything: reads each
/// case's `trace` file, normalizes it (native trajectory or OTel/
/// OpenInference JSON export), and outputs canonical trajectory JSON for the
/// `trajectory` scorer. Latency and token usage come from the trace itself.
pub struct TraceTarget;

#[async_trait]
impl Target for TraceTarget {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        let path = case.trace.as_ref().with_context(|| {
            format!(
                "case {:?} has no `trace` field (required by trace targets)",
                case.id
            )
        })?;
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read trace {}", path.display()))?;
        let normalized = crate::trace::normalize_trace(&raw)
            .with_context(|| format!("failed to normalize trace {}", path.display()))?;
        Ok(TargetOutput {
            text: serde_json::to_string(&normalized.trajectory)?,
            latency_ms: normalized.latency_ms,
            tokens: normalized.tokens,
        })
    }
    // cache_identity stays None: traces are local files, never worth caching.
}

/// POSTs to `{url}/chat/completions` in the OpenAI wire format.
///
/// Transient failures (429, 5xx, transport errors) are retried with
/// exponential backoff, honoring `Retry-After` when the server sends one.
/// Non-transient failures (4xx other than 429, unparseable 200 bodies) are
/// never retried.
pub struct OpenAiCompatTarget {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    max_retries: u32,
    /// Per-attempt budget the `client` was built with; kept so a timeout error
    /// can name it. Enforcement is the client's; this is only for the message.
    timeout_seconds: u64,
    system: Option<String>,
    params: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Outcome of one attempt, classified for the retry loop. Shared by every
/// HTTP-shaped target ([`OpenAiCompatTarget`], [`crate::http_target::HttpTarget`]).
pub(crate) enum AttemptError {
    /// 429/5xx/transport — worth retrying.
    Transient {
        message: String,
        retry_after: Option<Duration>,
    },
    /// Anything else — retrying would just repeat the failure.
    Permanent(anyhow::Error),
}

/// Drives `attempt` under the shared retry policy: transient failures back off
/// deterministically (honoring `Retry-After`) up to `max_retries`, permanent
/// failures return immediately, and an exhausted budget yields the attempt's
/// message tagged with the attempt count. One policy for every HTTP target so
/// retry semantics can't drift between them.
pub(crate) async fn retry_with_backoff<F, Fut>(
    max_retries: u32,
    mut attempt: F,
) -> anyhow::Result<TargetOutput>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<TargetOutput, AttemptError>>,
{
    let mut retries = 0;
    loop {
        match attempt().await {
            Ok(output) => return Ok(output),
            Err(AttemptError::Permanent(err)) => return Err(err),
            Err(AttemptError::Transient {
                message,
                retry_after,
            }) => {
                if retries >= max_retries {
                    bail!("{message} (after {} attempts)", retries + 1);
                }
                tokio::time::sleep(retry_after.unwrap_or_else(|| backoff(retries))).await;
                retries += 1;
            }
        }
    }
}

/// Build a pooled [`reqwest::Client`] with a per-attempt total timeout
/// (connect + reading the response body). Shared by every HTTP-shaped target so
/// timeout and pooling behavior can't drift between them. `reqwest::Client::new`
/// panics on TLS init failure; building fallibly here lets factories fail fast
/// with context instead of aborting the process.
pub(crate) fn build_http_client(timeout_seconds: u64) -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .context("failed to build the HTTP client")
}

/// Resolve an optional API key from the environment under the given policy.
/// Shared by every factory that reads `api_key_env`.
pub(crate) fn resolve_api_key(
    api_key_env: &Option<String>,
    secrets: SecretPolicy,
) -> anyhow::Result<Option<String>> {
    match (api_key_env, secrets) {
        (Some(var), SecretPolicy::Require) => {
            Ok(Some(std::env::var(var).with_context(|| {
                format!("environment variable {var} is not set")
            })?))
        }
        (Some(var), SecretPolicy::Optional) => Ok(std::env::var(var).ok()),
        (None, _) => Ok(None),
    }
}

impl OpenAiCompatTarget {
    /// Build a target with a per-attempt request timeout. Fallible because the
    /// underlying [`reqwest::Client`] is constructed here (TLS init can fail);
    /// the error propagates out of the factory ([`build_target_with`], which has
    /// no target name) for the caller to contextualize (e.g. the CLI attaches
    /// the target's config name; `build_scorers` labels judge targets).
    pub fn new(
        base_url: String,
        model: String,
        api_key: Option<String>,
        timeout_seconds: u64,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: build_http_client(timeout_seconds)?,
            base_url,
            model,
            api_key,
            max_retries: evalcore_config::DEFAULT_MAX_RETRIES,
            timeout_seconds,
            system: None,
            params: None,
        })
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_system(mut self, system: Option<String>) -> Self {
        self.system = system;
        self
    }

    pub fn with_params(
        mut self,
        params: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Self {
        self.params = params;
        self
    }

    fn request_body(&self, case: &TestCase) -> serde_json::Value {
        let mut messages = Vec::new();
        if let Some(system) = &self.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": case.input}));

        let mut body = serde_json::Map::new();
        body.insert("model".into(), serde_json::json!(self.model));
        body.insert("messages".into(), serde_json::json!(messages));
        if let Some(params) = &self.params {
            // Validation rejects reserved keys, so params can't clobber the
            // fields above.
            body.extend(params.clone());
        }
        serde_json::Value::Object(body)
    }

    async fn attempt(&self, url: &str, case: &TestCase) -> Result<TargetOutput, AttemptError> {
        let start = Instant::now();
        let mut request = self.client.post(url).json(&self.request_body(case));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }

        let response = request.send().await.map_err(|err| {
            // A timeout is transient like any transport error, but its message
            // must name the budget so a wedged endpoint is obvious in reports.
            let message = if err.is_timeout() {
                format!("request to {url} timed out after {}s", self.timeout_seconds)
            } else {
                format!("request to {url} failed: {err}")
            };
            AttemptError::Transient {
                message,
                retry_after: None,
            }
        })?;

        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs);
        // The per-attempt budget also covers the body read: a server that sends
        // 2xx headers promptly then stalls mid-body times out here, not in
        // `send`. Classify that as transient (like the send path) so it retries
        // and reports the budget, instead of being swallowed into an empty body.
        let body = match response.text().await {
            Ok(text) => text,
            Err(err) if err.is_timeout() => {
                return Err(AttemptError::Transient {
                    message: format!("request to {url} timed out after {}s", self.timeout_seconds),
                    retry_after: None,
                });
            }
            // Other body-read errors stay lenient (prior behavior): treat as an
            // empty body and let the parse below produce the failure reason.
            Err(_) => String::new(),
        };

        if !status.is_success() {
            let snippet: String = body.chars().take(200).collect();
            let message = format!("{url} returned {status}: {snippet}");
            return if status.as_u16() == 429 || status.is_server_error() {
                Err(AttemptError::Transient {
                    message,
                    retry_after,
                })
            } else {
                Err(AttemptError::Permanent(anyhow::anyhow!(message)))
            };
        }

        let parse = || -> anyhow::Result<TargetOutput> {
            let parsed: serde_json::Value = serde_json::from_str(&body)
                .with_context(|| format!("{url} returned non-JSON body"))?;
            let text = parsed["choices"][0]["message"]["content"]
                .as_str()
                .with_context(|| format!("{url} response missing choices[0].message.content"))?
                .to_string();
            let tokens = parsed.get("usage").and_then(|usage| {
                Some(TokenUsage {
                    input: usage.get("prompt_tokens")?.as_u64()?,
                    output: usage.get("completion_tokens").and_then(|v| v.as_u64())?,
                })
            });
            Ok(TargetOutput {
                text,
                latency_ms: start.elapsed().as_millis() as u64,
                tokens,
            })
        };
        parse().map_err(AttemptError::Permanent)
    }
}

/// Deterministic backoff: 500ms, 1s, 2s, … capped at 10s. No jitter — a
/// reproducible schedule matters more here than herd avoidance.
fn backoff(attempt: u32) -> Duration {
    let ms = 500u64.saturating_mul(1 << attempt.min(8));
    Duration::from_millis(ms.min(10_000))
}

#[async_trait]
impl Target for OpenAiCompatTarget {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        retry_with_backoff(self.max_retries, || self.attempt(&url, case)).await
    }

    fn cache_identity(&self) -> Option<serde_json::Value> {
        // Everything that changes the request must be here (system, params) —
        // a temperature change must never replay a stale answer. max_retries
        // and cost rates stay excluded: they change how we call and account,
        // not what the model would answer. Unset fields are OMITTED (not
        // null) so pre-existing cassettes keep their keys.
        let mut identity = serde_json::Map::new();
        identity.insert("type".into(), serde_json::json!("openai-compatible"));
        identity.insert("url".into(), serde_json::json!(self.base_url));
        identity.insert("model".into(), serde_json::json!(self.model));
        if let Some(system) = &self.system {
            identity.insert("system".into(), serde_json::json!(system));
        }
        if let Some(params) = &self.params {
            if !params.is_empty() {
                identity.insert("params".into(), serde_json::Value::Object(params.clone()));
            }
        }
        Some(serde_json::Value::Object(identity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(input: &str) -> TestCase {
        TestCase {
            id: "t".into(),
            input: input.into(),
            expected: None,
            trace: None,
        }
    }

    #[tokio::test]
    async fn shell_target_pipes_stdin_to_stdout() {
        let target = ShellTarget::new("tr 'a-z' 'A-Z'".into());
        let out = target.invoke(&case("hello")).await.unwrap();
        assert_eq!(out.text, "HELLO");
    }

    #[tokio::test]
    async fn shell_target_tolerates_commands_that_ignore_stdin() {
        // `echo` never reads stdin; on Linux the child can exit before the
        // input write finishes, producing EPIPE. That must not fail the case.
        let target = ShellTarget::new("echo ok".into());
        let out = target.invoke(&case("ignored input")).await.unwrap();
        assert_eq!(out.text.trim(), "ok");
    }

    #[tokio::test]
    async fn shell_target_surfaces_failures_with_stderr() {
        let target = ShellTarget::new("echo boom >&2; exit 3".into());
        let err = target.invoke(&case("x")).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("boom"), "got: {msg}");
    }

    #[test]
    fn build_target_fails_fast_on_missing_env_var() {
        let config = TargetConfig::OpenaiCompatible {
            url: "http://localhost:9999/v1".into(),
            model: "m".into(),
            api_key_env: Some("EVALCORE_TEST_KEY_THAT_DOES_NOT_EXIST".into()),
            max_retries: 2,
            timeout_seconds: evalcore_config::DEFAULT_TIMEOUT_SECONDS,
            cost: None,
            system: None,
            params: None,
        };
        let err = build_target(&config)
            .err()
            .expect("missing env var must be a build error");
        assert!(err
            .to_string()
            .contains("EVALCORE_TEST_KEY_THAT_DOES_NOT_EXIST"));
    }

    #[test]
    fn cache_identity_omits_unset_fields_and_includes_set_ones() {
        let bare = OpenAiCompatTarget::new(
            "http://x/v1".into(),
            "m".into(),
            None,
            evalcore_config::DEFAULT_TIMEOUT_SECONDS,
        )
        .unwrap();
        // Exact shape is load-bearing: adding keys for unset fields would
        // invalidate every cassette recorded before system/params existed.
        assert_eq!(
            bare.cache_identity().unwrap(),
            serde_json::json!({"model": "m", "type": "openai-compatible", "url": "http://x/v1"})
        );

        let mut params = serde_json::Map::new();
        params.insert("temperature".into(), serde_json::json!(0));
        let tuned = OpenAiCompatTarget::new(
            "http://x/v1".into(),
            "m".into(),
            None,
            evalcore_config::DEFAULT_TIMEOUT_SECONDS,
        )
        .unwrap()
        .with_system(Some("be terse".into()))
        .with_params(Some(params));
        let identity = tuned.cache_identity().unwrap();
        assert_eq!(identity["system"], serde_json::json!("be terse"));
        assert_eq!(identity["params"]["temperature"], serde_json::json!(0));
        assert_ne!(identity, bare.cache_identity().unwrap());
    }

    #[test]
    fn cache_identity_excludes_timeout() {
        // timeout_seconds is an operational knob (like max_retries): it changes
        // how we call, not what the model answers, so it must never enter the
        // identity — cassettes recorded before this knob must keep their keys.
        let default = OpenAiCompatTarget::new("http://x/v1".into(), "m".into(), None, 120).unwrap();
        let custom = OpenAiCompatTarget::new("http://x/v1".into(), "m".into(), None, 5).unwrap();
        assert_eq!(
            default.cache_identity().unwrap(),
            custom.cache_identity().unwrap(),
            "changing timeout_seconds must not change the cache identity"
        );
        let serialized = serde_json::to_string(&custom.cache_identity().unwrap()).unwrap();
        assert!(
            !serialized.contains("timeout"),
            "identity must not mention timeout, got: {serialized}"
        );
    }

    #[test]
    fn optional_secret_policy_tolerates_missing_env_var() {
        let config = TargetConfig::OpenaiCompatible {
            url: "http://localhost:9999/v1".into(),
            model: "m".into(),
            api_key_env: Some("EVALCORE_TEST_KEY_THAT_DOES_NOT_EXIST".into()),
            max_retries: 2,
            timeout_seconds: evalcore_config::DEFAULT_TIMEOUT_SECONDS,
            cost: None,
            system: None,
            params: None,
        };
        assert!(
            build_target_with(&config, SecretPolicy::Optional).is_ok(),
            "replay-only runs must build without secrets"
        );
    }
}
