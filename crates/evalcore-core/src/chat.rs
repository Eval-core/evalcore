//! Shared machinery for chat-shaped HTTP targets.
//!
//! Three wire formats now speak to LLM endpoints (OpenAI chat-completions,
//! Anthropic Messages, Gemini generateContent). At two implementations an
//! abstraction wasn't worth it; at three the shape of the variation is finally
//! known, and it is small:
//!
//! - the endpoint path,
//! - the request body,
//! - which header the API key rides in (plus any constant headers),
//! - how the answer and token usage are read out of a 2xx body,
//! - the cache identity.
//!
//! Everything else — retry policy, client construction, transient/permanent
//! classification, status handling, the body-read timeout, latency measurement
//! — was byte-identical in all three copies and lives here once, in
//! [`ChatTarget`]. Authentication is a *separate* axis from wire format
//! ([`ChatAuth`]): AWS SigV4 signs an Anthropic-shaped request, so it must not
//! be modeled as a third format.

use std::time::{Duration, Instant};

use anyhow::Context;
use async_trait::async_trait;
use evalcore_config::DEFAULT_MAX_RETRIES;

use crate::sigv4::SigV4Credentials;
use crate::target::{build_http_client, retry_with_backoff, AttemptError, Target};
use crate::types::{TargetOutput, TestCase, TokenUsage};

/// What a wire format pulls out of a successful response.
pub struct ChatReply {
    pub text: String,
    pub tokens: Option<TokenUsage>,
}

/// The header an API key rides in, as (name, value prefix). `Authorization`
/// takes a `"Bearer "` prefix; `x-api-key` and `x-goog-api-key` take none.
pub struct KeyHeader {
    pub name: &'static str,
    pub prefix: &'static str,
}

/// One LLM wire format: everything that differs between providers, and nothing
/// that doesn't.
///
/// Public only because [`ChatTarget`] is (through the per-provider type
/// aliases). It is an internal seam, not a supported extension point —
/// architecture rule 1 stands: adding a provider is EvalCore's job, and no user
/// should ever have to write Rust to evaluate one.
pub trait ChatWireFormat: Send + Sync + 'static {
    /// Full request URL, given the configured base URL.
    fn endpoint(&self, base_url: &str) -> String;

    /// The JSON request body for one case.
    fn request_body(&self, case: &TestCase) -> serde_json::Value;

    /// Where an API key goes when auth is [`ChatAuth::ApiKey`].
    fn key_header(&self) -> KeyHeader;

    /// Constant headers every request carries (e.g. `anthropic-version`).
    fn extra_headers(&self) -> &[(&'static str, &'static str)] {
        &[]
    }

    /// Read the answer and usage out of a parsed 2xx body. A provider-side
    /// decline (content policy, safety filter) belongs here as a readable
    /// `Err`, never as a parse failure blaming the server.
    fn parse_response(&self, url: &str, body: &serde_json::Value) -> anyhow::Result<ChatReply>;

    /// Everything about this format that changes what would be sent, including
    /// the base URL it was configured with. Secrets must never appear; unset
    /// optional fields must be omitted, not null.
    fn cache_identity(&self, base_url: &str) -> serde_json::Value;
}

/// How a request proves who it is. Orthogonal to [`ChatWireFormat`] — the same
/// Anthropic body is authenticated by an API key against api.anthropic.com and
/// by a SigV4 signature against bedrock-runtime.
pub enum ChatAuth {
    /// Send the key in the format's own header. `None` sends no credential at
    /// all, which is what replay-only runs and local mock servers use.
    ApiKey(Option<String>),
    /// AWS Signature Version 4 over the request.
    SigV4(Box<SigV4Credentials>),
}

/// An HTTP chat target: one wire format, one auth mode, and the shared retry,
/// timeout, and error-classification policy every LLM target uses.
///
/// Transient failures (429, 5xx, transport errors, timeouts) are retried with
/// deterministic backoff honoring `Retry-After`. Non-transient failures (4xx
/// other than 429, unparseable 200 bodies, provider declines) are never
/// retried.
pub struct ChatTarget<F> {
    client: reqwest::Client,
    base_url: String,
    auth: ChatAuth,
    max_retries: u32,
    /// Per-attempt budget the `client` was built with; kept so a timeout error
    /// can name it. Enforcement is the client's; this is only for the message.
    timeout_seconds: u64,
    pub(crate) format: F,
}

impl<F: ChatWireFormat> ChatTarget<F> {
    /// Build a target with a per-attempt request timeout. Fallible because the
    /// [`reqwest::Client`] is constructed here (TLS init can fail); the error
    /// propagates out of the factory for the caller to contextualize.
    pub(crate) fn build(
        base_url: String,
        auth: ChatAuth,
        timeout_seconds: u64,
        format: F,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: build_http_client(timeout_seconds)?,
            base_url,
            auth,
            max_retries: DEFAULT_MAX_RETRIES,
            timeout_seconds,
            format,
        })
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    async fn attempt(&self, url: &str, case: &TestCase) -> Result<TargetOutput, AttemptError> {
        let start = Instant::now();
        // Serialized once, up front: SigV4 hashes the exact payload bytes, so
        // the body cannot be re-serialized between signing and sending.
        let body = serde_json::to_vec(&self.format.request_body(case))
            .map_err(|err| AttemptError::Permanent(anyhow::anyhow!(err)))?;

        let mut request = self
            .client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        for (name, value) in self.format.extra_headers() {
            request = request.header(*name, *value);
        }
        request = match &self.auth {
            ChatAuth::ApiKey(None) => request,
            ChatAuth::ApiKey(Some(key)) => {
                let KeyHeader { name, prefix } = self.format.key_header();
                request.header(name, format!("{prefix}{key}"))
            }
            ChatAuth::SigV4(credentials) => {
                // Signing reads the clock. That is deliberate and contained: a
                // signature timestamp is never user-visible output and never
                // enters a cache identity, so determinism is untouched.
                let signed = credentials
                    .sign_post(url, self.format.extra_headers(), &body)
                    .map_err(AttemptError::Permanent)?;
                signed
                    .into_iter()
                    .fold(request, |req, (name, value)| req.header(name, value))
            }
        };

        let response = request.body(body).send().await.map_err(|err| {
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
        let text = match response.text().await {
            Ok(text) => text,
            Err(err) if err.is_timeout() => {
                return Err(AttemptError::Transient {
                    message: format!("request to {url} timed out after {}s", self.timeout_seconds),
                    retry_after: None,
                });
            }
            // Other body-read errors stay lenient: treat as an empty body and
            // let the parse below produce the failure reason.
            Err(_) => String::new(),
        };

        if !status.is_success() {
            let snippet: String = text.chars().take(200).collect();
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
            let parsed: serde_json::Value = serde_json::from_str(&text)
                .with_context(|| format!("{url} returned non-JSON body"))?;
            let reply = self.format.parse_response(url, &parsed)?;
            Ok(TargetOutput {
                text: reply.text,
                latency_ms: start.elapsed().as_millis() as u64,
                tokens: reply.tokens,
                trajectory: None,
            })
        };
        parse().map_err(AttemptError::Permanent)
    }
}

#[async_trait]
impl<F: ChatWireFormat> Target for ChatTarget<F> {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        let url = self.format.endpoint(&self.base_url);
        retry_with_backoff(self.max_retries, || self.attempt(&url, case)).await
    }

    fn cache_identity(&self) -> Option<serde_json::Value> {
        // The auth mode is deliberately absent: credentials are secrets, and
        // signing the same body a different way does not change what the model
        // answers. The `type` discriminator inside each format's identity is
        // what keeps the wire formats in disjoint keyspaces.
        Some(self.format.cache_identity(&self.base_url))
    }
}

/// Read the answer out of an array of typed content blocks — the shape both
/// Anthropic (`content[]`) and Gemini (`candidates[0].content.parts[]`) use.
///
/// Reading index 0 is wrong for both the moment reasoning is enabled: a
/// thinking/thought block precedes the answer and carries no text. Blocks that
/// carry text are concatenated in order; blocks that don't are skipped.
/// Returns `None` when no text-bearing block was present at all, which callers
/// turn into a message naming the provider's own stop/finish reason.
pub(crate) fn concat_text_blocks(
    blocks: &[serde_json::Value],
    text_of: impl Fn(&serde_json::Value) -> Option<&str>,
) -> Option<String> {
    let mut text = String::new();
    let mut saw_text = false;
    for block in blocks {
        if let Some(chunk) = text_of(block) {
            saw_text = true;
            text.push_str(chunk);
        }
    }
    saw_text.then_some(text)
}
