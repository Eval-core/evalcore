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
        TargetConfig::OpenaiCompatible {
            url,
            model,
            api_key_env,
            max_retries,
            cost: _,
        } => {
            let api_key = match (api_key_env, secrets) {
                (Some(var), SecretPolicy::Require) => Some(
                    std::env::var(var)
                        .with_context(|| format!("environment variable {var} is not set"))?,
                ),
                (Some(var), SecretPolicy::Optional) => std::env::var(var).ok(),
                (None, _) => None,
            };
            Ok(Box::new(
                OpenAiCompatTarget::new(url.clone(), model.clone(), api_key)
                    .with_max_retries(*max_retries),
            ))
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
            stdin.write_all(case.input.as_bytes()).await?;
            stdin.shutdown().await?;
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
}

/// Outcome of one attempt, classified for the retry loop.
enum AttemptError {
    /// 429/5xx/transport — worth retrying.
    Transient {
        message: String,
        retry_after: Option<Duration>,
    },
    /// Anything else — retrying would just repeat the failure.
    Permanent(anyhow::Error),
}

impl OpenAiCompatTarget {
    pub fn new(base_url: String, model: String, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            model,
            api_key,
            max_retries: evalcore_config::DEFAULT_MAX_RETRIES,
        }
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    async fn attempt(&self, url: &str, case: &TestCase) -> Result<TargetOutput, AttemptError> {
        let start = Instant::now();
        let mut request = self.client.post(url).json(&serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": case.input}],
        }));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }

        let response = request
            .send()
            .await
            .map_err(|err| AttemptError::Transient {
                message: format!("request to {url} failed: {err}"),
                retry_after: None,
            })?;

        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = response.text().await.unwrap_or_default();

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
        let mut attempt = 0;
        loop {
            match self.attempt(&url, case).await {
                Ok(output) => return Ok(output),
                Err(AttemptError::Permanent(err)) => return Err(err),
                Err(AttemptError::Transient {
                    message,
                    retry_after,
                }) => {
                    if attempt >= self.max_retries {
                        bail!("{message} (after {} attempts)", attempt + 1);
                    }
                    tokio::time::sleep(retry_after.unwrap_or_else(|| backoff(attempt))).await;
                    attempt += 1;
                }
            }
        }
    }

    fn cache_identity(&self) -> Option<serde_json::Value> {
        // max_retries and cost rates deliberately excluded: they change how we
        // call and account, not what the model would answer.
        Some(serde_json::json!({
            "type": "openai-compatible",
            "url": self.base_url,
            "model": self.model,
        }))
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
        }
    }

    #[tokio::test]
    async fn shell_target_pipes_stdin_to_stdout() {
        let target = ShellTarget::new("tr 'a-z' 'A-Z'".into());
        let out = target.invoke(&case("hello")).await.unwrap();
        assert_eq!(out.text, "HELLO");
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
            cost: None,
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
            cost: None,
        };
        assert!(
            build_target_with(&config, SecretPolicy::Optional).is_ok(),
            "replay-only runs must build without secrets"
        );
    }
}
