//! OpenAI-compatible embeddings target: the backend for the `similarity`
//! scorer. Speaks `POST {url}/embeddings`; the target "input" is the exact
//! text to embed, the output `text` is the embedding vector serialized as a
//! JSON array — which is what makes these calls cacheable through
//! `CachedTarget` like any other target call (the vector round-trips through
//! the cache as the recorded output).
//!
//! The scorers crate must not depend on `evalcore-store`, so — exactly like
//! the judge — the CLI injects a target-builder closure that wraps this target
//! in the record/replay cache. [`TargetSpec`] is what that closure receives:
//! `Chat` preserves today's judge behavior byte-for-byte (its cache identity
//! is unchanged), `Embeddings` selects this target.

use std::time::Instant;

use anyhow::Context;
use async_trait::async_trait;
use evalcore_config::{TargetConfig, DEFAULT_MAX_RETRIES};

use crate::target::{
    build_http_client, build_target_with, resolve_api_key, retry_with_backoff, AttemptError,
    SecretPolicy, Target,
};
use crate::types::{TargetOutput, TestCase};

/// What a scorer's injected target-builder closure is asked to build. Keeps the
/// judge's construction unchanged (`Chat` maps to the same `TargetConfig` it
/// always did) while letting the `similarity` scorer request an embeddings
/// target that has no `TargetConfig` variant (the config schema is frozen).
pub enum TargetSpec {
    /// A chat/completions target, built exactly like before — used by the
    /// judge scorer, so its cache identity stays byte-for-byte identical.
    Chat(TargetConfig),
    /// An OpenAI-compatible embeddings target for the `similarity` scorer.
    Embeddings {
        url: String,
        model: String,
        api_key_env: Option<String>,
        timeout_seconds: u64,
    },
}

/// Build the target a scorer needs from its [`TargetSpec`], honoring the secret
/// policy. `Chat` delegates to [`build_target_with`] so judge targets are built
/// (and cached) exactly as before; `Embeddings` constructs an
/// [`EmbeddingsTarget`], resolving its API key through the shared helper.
pub fn build_scorer_target(
    spec: TargetSpec,
    secrets: SecretPolicy,
) -> anyhow::Result<Box<dyn Target>> {
    match spec {
        TargetSpec::Chat(config) => build_target_with(&config, secrets),
        TargetSpec::Embeddings {
            url,
            model,
            api_key_env,
            timeout_seconds,
        } => {
            let api_key = resolve_api_key(&api_key_env, secrets)?;
            Ok(Box::new(EmbeddingsTarget::new(
                url,
                model,
                api_key,
                timeout_seconds,
            )?))
        }
    }
}

/// POSTs to `{url}/embeddings` in the OpenAI wire format
/// (`{"model", "input": [text]}` -> `data[0].embedding`). Shares the retry,
/// timeout, and transient-classification policy with the other HTTP targets;
/// the embedding vector is serialized into `TargetOutput.text` so it can be
/// recorded and replayed like any other output.
pub struct EmbeddingsTarget {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    max_retries: u32,
    /// Per-attempt budget the `client` was built with; kept so a timeout error
    /// can name it. Enforcement is the client's; this is only for the message.
    timeout_seconds: u64,
}

impl EmbeddingsTarget {
    /// Build an embeddings target with a per-attempt request timeout. Fallible
    /// because the [`reqwest::Client`] is constructed here (TLS init can fail).
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
            max_retries: DEFAULT_MAX_RETRIES,
            timeout_seconds,
        })
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    async fn attempt(&self, url: &str, case: &TestCase) -> Result<TargetOutput, AttemptError> {
        let start = Instant::now();
        let body = serde_json::json!({
            "model": self.model,
            "input": [case.input],
        });
        let mut request = self.client.post(url).json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }

        let response = request.send().await.map_err(|err| {
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
            .map(std::time::Duration::from_secs);
        let raw = match response.text().await {
            Ok(text) => text,
            Err(err) if err.is_timeout() => {
                return Err(AttemptError::Transient {
                    message: format!("request to {url} timed out after {}s", self.timeout_seconds),
                    retry_after: None,
                });
            }
            Err(_) => String::new(),
        };

        if !status.is_success() {
            let snippet: String = raw.chars().take(200).collect();
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
            let parsed: serde_json::Value = serde_json::from_str(&raw)
                .with_context(|| format!("{url} returned non-JSON body"))?;
            let embedding = parsed["data"][0]["embedding"]
                .as_array()
                .with_context(|| format!("{url} response missing data[0].embedding array"))?;
            let vector: Vec<f64> = embedding
                .iter()
                .map(|v| {
                    v.as_f64()
                        .with_context(|| format!("{url} embedding contains a non-number entry"))
                })
                .collect::<anyhow::Result<_>>()?;
            // The vector rides through the cache as the recorded output text.
            let text =
                serde_json::to_string(&vector).context("failed to serialize embedding vector")?;
            Ok(TargetOutput {
                text,
                latency_ms: start.elapsed().as_millis() as u64,
                tokens: None,
                trajectory: None,
            })
        };
        parse().map_err(AttemptError::Permanent)
    }
}

#[async_trait]
impl Target for EmbeddingsTarget {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        retry_with_backoff(self.max_retries, || self.attempt(&url, case)).await
    }

    fn cache_identity(&self) -> Option<serde_json::Value> {
        // The "embeddings" discriminator guarantees these can never collide
        // with a chat target at the same url/model. Like the chat target, only
        // what changes the response is included; secrets and operational knobs
        // (api_key, retries, timeout) stay out.
        Some(serde_json::json!({
            "type": "embeddings",
            "url": self.base_url,
            "model": self.model,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn case(input: &str) -> TestCase {
        TestCase {
            id: "t".into(),
            input: input.into(),
            expected: None,
            trace: None,
            context: None,
        }
    }

    async fn embeddings_server(vector: serde_json::Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": vector}],
            })))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn embeds_input_and_serializes_the_vector() {
        let server = embeddings_server(serde_json::json!([0.1, 0.2, 0.3])).await;
        let target =
            EmbeddingsTarget::new(format!("{}/v1", server.uri()), "embed".into(), None, 120)
                .unwrap();
        let out = target.invoke(&case("hello")).await.unwrap();
        let vector: Vec<f64> = serde_json::from_str(&out.text).unwrap();
        assert_eq!(vector, vec![0.1, 0.2, 0.3]);
    }

    #[tokio::test]
    async fn request_carries_model_and_input() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(body_string_contains("\"model\":\"embed\""))
            .and(body_string_contains("needle text"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": [1.0]}],
            })))
            .expect(1)
            .mount(&server)
            .await;
        let target =
            EmbeddingsTarget::new(format!("{}/v1", server.uri()), "embed".into(), None, 120)
                .unwrap();
        target.invoke(&case("needle text")).await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn non_200_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;
        let target =
            EmbeddingsTarget::new(format!("{}/v1", server.uri()), "embed".into(), None, 120)
                .unwrap();
        let err = target.invoke(&case("x")).await.unwrap_err();
        assert!(err.to_string().contains("400"), "got: {err}");
    }

    #[tokio::test]
    async fn malformed_body_missing_embedding_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"not_embedding": true}],
            })))
            .mount(&server)
            .await;
        let target =
            EmbeddingsTarget::new(format!("{}/v1", server.uri()), "embed".into(), None, 120)
                .unwrap();
        let err = target.invoke(&case("x")).await.unwrap_err();
        assert!(err.to_string().contains("data[0].embedding"), "got: {err}");
    }

    #[test]
    fn cache_identity_has_embeddings_discriminator() {
        let target =
            EmbeddingsTarget::new("http://x/v1".into(), "embed".into(), None, 120).unwrap();
        assert_eq!(
            target.cache_identity().unwrap(),
            serde_json::json!({"type": "embeddings", "url": "http://x/v1", "model": "embed"})
        );
    }

    #[test]
    fn embeddings_identity_never_collides_with_chat_at_same_url_model() {
        let embed = EmbeddingsTarget::new("http://x/v1".into(), "m".into(), None, 120).unwrap();
        let chat =
            crate::target::OpenAiCompatTarget::new("http://x/v1".into(), "m".into(), None, 120)
                .unwrap();
        assert_ne!(
            embed.cache_identity().unwrap(),
            chat.cache_identity().unwrap(),
            "the discriminator must keep embeddings and chat keys disjoint"
        );
    }

    #[test]
    fn build_scorer_target_chat_preserves_openai_identity() {
        // The judge routes through TargetSpec::Chat; its cache identity must be
        // byte-for-byte what an openai-compatible target has always produced,
        // or every recorded judge cassette invalidates on upgrade.
        let spec = TargetSpec::Chat(TargetConfig::OpenaiCompatible {
            url: "http://x/v1".into(),
            model: "m".into(),
            api_key_env: None,
            max_retries: DEFAULT_MAX_RETRIES,
            timeout_seconds: 120,
            cost: None,
            system: None,
            params: None,
        });
        let target = build_scorer_target(spec, SecretPolicy::Require).unwrap();
        assert_eq!(
            target.cache_identity().unwrap(),
            serde_json::json!({"model": "m", "type": "openai-compatible", "url": "http://x/v1"})
        );
    }
}
