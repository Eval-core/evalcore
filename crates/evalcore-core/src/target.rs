//! Targets: the things being evaluated.
//!
//! Design rule: targets are protocol-shaped (shell command, OpenAI-compatible
//! HTTP), not vendor SDKs. A vendor that speaks the OpenAI wire format needs
//! zero new code here.

use std::process::Stdio;
use std::time::Instant;

use anyhow::{bail, Context};
use async_trait::async_trait;
use evalcore_config::TargetConfig;
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
        TargetConfig::OpenaiCompatible {
            url,
            model,
            api_key_env,
        } => {
            let api_key = match (api_key_env, secrets) {
                (Some(var), SecretPolicy::Require) => Some(
                    std::env::var(var)
                        .with_context(|| format!("environment variable {var} is not set"))?,
                ),
                (Some(var), SecretPolicy::Optional) => std::env::var(var).ok(),
                (None, _) => None,
            };
            Ok(Box::new(OpenAiCompatTarget::new(
                url.clone(),
                model.clone(),
                api_key,
            )))
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
        })
    }
}

/// POSTs to `{url}/chat/completions` in the OpenAI wire format.
pub struct OpenAiCompatTarget {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiCompatTarget {
    pub fn new(base_url: String, model: String, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            model,
            api_key,
        }
    }
}

#[async_trait]
impl Target for OpenAiCompatTarget {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        let start = Instant::now();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut request = self.client.post(&url).json(&serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": case.input}],
        }));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("request to {url} failed"))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let snippet: String = body.chars().take(200).collect();
            bail!("{url} returned {status}: {snippet}");
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&body).with_context(|| format!("{url} returned non-JSON body"))?;
        let text = parsed["choices"][0]["message"]["content"]
            .as_str()
            .with_context(|| format!("{url} response missing choices[0].message.content"))?
            .to_string();

        Ok(TargetOutput {
            text,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn cache_identity(&self) -> Option<serde_json::Value> {
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
        };
        assert!(
            build_target_with(&config, SecretPolicy::Optional).is_ok(),
            "replay-only runs must build without secrets"
        );
    }
}
