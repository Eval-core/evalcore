//! Targets: the things being evaluated, plus the factory that builds them.
//!
//! Design rule: targets are protocol-shaped (shell command, HTTP wire
//! formats), not vendor SDKs. A vendor that speaks a format EvalCore already
//! knows needs zero new code here.
//!
//! The `Target` trait and the shared HTTP policy live here; the individual
//! chat wire formats live in `openai.rs`, `anthropic.rs`, and `gemini.rs`, over
//! the `ChatWireFormat` abstraction in `chat.rs`.

use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use async_trait::async_trait;
use evalcore_config::{SigV4Config, TargetConfig};
use tokio::io::AsyncWriteExt;

use crate::types::{TargetOutput, TestCase};

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
                crate::openai::OpenAiCompatTarget::new(
                    url.clone(),
                    model.clone(),
                    api_key,
                    *timeout_seconds,
                )?
                .with_max_retries(*max_retries)
                .with_system(system.clone())
                .with_params(params.clone()),
            ))
        }
        TargetConfig::Anthropic {
            url,
            model,
            api_key_env,
            max_retries,
            timeout_seconds,
            max_tokens,
            cost: _,
            system,
            params,
            auth,
            dialect,
            aws,
        } => {
            // Two independent axes. SigV4 signs a plain Messages request against
            // the Messages-shaped AWS endpoints (Bedrock Mantle, Claude Platform
            // on AWS) just as readily as it signs an InvokeModel one, so the
            // auth mode must not imply the dialect.
            let auth = match auth {
                evalcore_config::AnthropicAuth::ApiKey => {
                    crate::chat::ChatAuth::ApiKey(resolve_api_key(api_key_env, secrets)?)
                }
                evalcore_config::AnthropicAuth::Sigv4 => {
                    // Validation guarantees `aws` is present under sigv4.
                    let aws = aws.as_ref().context(
                        "internal: `auth: sigv4` reached the factory without an `aws` block",
                    )?;
                    crate::chat::ChatAuth::SigV4(Box::new(resolve_sigv4(aws, secrets)?))
                }
            };
            let dialect = match dialect {
                evalcore_config::AnthropicDialectConfig::Messages => {
                    crate::anthropic::AnthropicDialect::Messages
                }
                evalcore_config::AnthropicDialectConfig::BedrockInvoke => {
                    crate::anthropic::AnthropicDialect::BedrockInvoke
                }
            };
            Ok(Box::new(
                crate::anthropic::AnthropicTarget::with_auth(
                    url.clone(),
                    model.clone(),
                    auth,
                    *timeout_seconds,
                    dialect,
                )?
                .with_max_retries(*max_retries)
                .with_max_tokens(*max_tokens)
                .with_system(system.clone())
                .with_params(params.clone()),
            ))
        }
        TargetConfig::Gemini {
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
                crate::gemini::GeminiTarget::new(
                    url.clone(),
                    model.clone(),
                    api_key,
                    *timeout_seconds,
                )?
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
            trajectory: None,
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
        // `text` is the final answer when the trace carries one (judge/text
        // scorers grade it), else the serialized trajectory JSON exactly as
        // before (back-compat for suites that grade the JSON with `contains`).
        // The structured trajectory is ALWAYS attached so the `trajectory`
        // scorer asserts on the steps regardless of what `text` holds.
        let text = match normalized.final_output {
            Some(answer) => answer,
            None => serde_json::to_string(&normalized.trajectory)?,
        };
        Ok(TargetOutput {
            text,
            latency_ms: normalized.latency_ms,
            tokens: normalized.tokens,
            trajectory: Some(normalized.trajectory),
        })
    }
    // cache_identity stays None: traces are local files, never worth caching.
}

/// Outcome of one attempt, classified for the retry loop. Shared by every
/// HTTP-shaped target ([`crate::chat::ChatTarget`], [`crate::http_target::HttpTarget`]).
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

/// Resolve AWS credentials from the environment variables the config names.
/// Same secret model as [`resolve_api_key`]: the YAML holds variable names, not
/// values, and nothing here reads a profile file, the metadata service, or the
/// clock. Under [`SecretPolicy::Optional`] (replay-only runs) a missing key
/// yields empty credentials, which are never used because no call is made.
pub(crate) fn resolve_sigv4(
    config: &SigV4Config,
    secrets: SecretPolicy,
) -> anyhow::Result<crate::sigv4::SigV4Credentials> {
    let required = |var: &str| -> anyhow::Result<String> {
        match secrets {
            SecretPolicy::Require => {
                std::env::var(var).with_context(|| format!("environment variable {var} is not set"))
            }
            SecretPolicy::Optional => Ok(std::env::var(var).unwrap_or_default()),
        }
    };
    Ok(crate::sigv4::SigV4Credentials {
        access_key_id: required(&config.access_key_env)?,
        secret_access_key: required(&config.secret_key_env)?,
        // Always optional: long-lived credentials have no session token.
        session_token: std::env::var(&config.session_token_env)
            .ok()
            .filter(|token| !token.is_empty()),
        region: config.region.clone(),
        service: config.service.clone(),
    })
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

/// Deterministic backoff: 500ms, 1s, 2s, … capped at 10s. No jitter — a
/// reproducible schedule matters more here than herd avoidance.
fn backoff(attempt: u32) -> Duration {
    let ms = 500u64.saturating_mul(1 << attempt.min(8));
    Duration::from_millis(ms.min(10_000))
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
            context: None,
        }
    }

    async fn trace_output(trace_json: &str) -> TargetOutput {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.json");
        std::fs::write(&path, trace_json).unwrap();
        let case = TestCase {
            id: "trace-case".into(),
            input: String::new(),
            expected: None,
            trace: Some(path),
            context: None,
        };
        TraceTarget.invoke(&case).await.unwrap()
    }

    #[tokio::test]
    async fn trace_target_text_is_final_answer_when_present() {
        let out = trace_output(
            r#"{"final_output": "Refunds take 30 days.",
                "steps": [{"tool": "search_kb", "input": {"q": "refund"}}]}"#,
        )
        .await;
        // `text` is the final answer, not the trajectory JSON — judge/text
        // scorers grade the answer.
        assert_eq!(out.text, "Refunds take 30 days.");
        // The structured trajectory is still attached for the trajectory scorer.
        let trajectory = out.trajectory.expect("trace target always attaches it");
        assert_eq!(trajectory.steps.len(), 1);
        assert_eq!(trajectory.steps[0].tool, "search_kb");
    }

    #[tokio::test]
    async fn trace_target_text_is_trajectory_json_when_no_final_answer() {
        let out = trace_output(r#"{"steps": [{"tool": "search_kb", "input": {}}]}"#).await;
        // No final_output → back-compat: text is the serialized trajectory JSON.
        let parsed: serde_json::Value = serde_json::from_str(&out.text).unwrap();
        assert!(
            parsed.get("steps").is_some(),
            "text is trajectory JSON: {}",
            out.text
        );
        assert!(out.trajectory.is_some(), "trajectory always populated");
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
