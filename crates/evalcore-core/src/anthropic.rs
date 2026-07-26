//! Anthropic Messages wire format: `POST {url}/messages`.
//!
//! A wire format, not an SDK — users still write YAML. Everything shared with
//! the other chat targets (retry policy, HTTP client construction, status
//! classification, auth application) lives in [`crate::chat`]; this file is
//! only what makes Anthropic Anthropic.
//!
//! Auth and dialect are independent axes, which is why neither is a target
//! type: SigV4 signs a plain Messages request against the Messages-shaped AWS
//! endpoints (Bedrock Mantle, Claude Platform on AWS) exactly as it signs a
//! legacy `bedrock-runtime` InvokeModel one.

use anyhow::{bail, Context};
use evalcore_config::DEFAULT_MAX_TOKENS;

use crate::chat::{concat_text_blocks, ChatAuth, ChatReply, ChatTarget, ChatWireFormat, KeyHeader};
use crate::types::{TestCase, TokenUsage};

/// The Messages API version header. Pinned: a newer version can change the
/// response shape this format parses, so it moves only with the parser.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The `anthropic_version` body field Bedrock requires in place of `model`
/// (the model is in the InvokeModel path instead).
const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";

/// Which deployment of the Messages format an endpoint speaks.
///
/// Three things move together within a dialect — the path, the
/// `model`/`anthropic_version` body field, and the version header — so they are
/// one enum rather than three knobs. The dialect itself is independent of
/// [`crate::chat::ChatAuth`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnthropicDialect {
    /// `POST {url}/messages` with `model` in the body. api.anthropic.com, any
    /// Messages-shaped gateway, and the Messages-shaped AWS endpoints (Bedrock
    /// Mantle, Claude Platform on AWS) — the latter under `auth: sigv4`.
    Messages,
    /// `POST {url}/model/{model}/invoke` with `anthropic_version` in the body
    /// and the model in the path. Legacy AWS `bedrock-runtime` only.
    BedrockInvoke,
}

/// The Anthropic Messages wire format.
pub struct AnthropicFormat {
    model: String,
    dialect: AnthropicDialect,
    /// Sent as the top-level `system` body field — the Messages API has no
    /// `system` message role.
    system: Option<String>,
    /// Required by the API, so it is always sent.
    max_tokens: u64,
    params: Option<serde_json::Map<String, serde_json::Value>>,
}

/// An Anthropic Messages target.
pub type AnthropicTarget = ChatTarget<AnthropicFormat>;

impl AnthropicTarget {
    /// Build a Messages target against an API-key endpoint.
    pub fn new(
        base_url: String,
        model: String,
        api_key: Option<String>,
        timeout_seconds: u64,
    ) -> anyhow::Result<Self> {
        Self::with_auth(
            base_url,
            model,
            ChatAuth::ApiKey(api_key),
            timeout_seconds,
            AnthropicDialect::Messages,
        )
    }

    /// Build a Messages target with an explicit auth mode and dialect. Used by
    /// the factory for `auth: sigv4`, which pairs SigV4 signing with the
    /// Bedrock InvokeModel dialect.
    pub(crate) fn with_auth(
        base_url: String,
        model: String,
        auth: ChatAuth,
        timeout_seconds: u64,
        dialect: AnthropicDialect,
    ) -> anyhow::Result<Self> {
        ChatTarget::build(
            base_url,
            auth,
            timeout_seconds,
            AnthropicFormat {
                model,
                dialect,
                system: None,
                max_tokens: DEFAULT_MAX_TOKENS,
                params: None,
            },
        )
    }

    pub fn with_system(mut self, system: Option<String>) -> Self {
        self.format.system = system;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u64) -> Self {
        self.format.max_tokens = max_tokens;
        self
    }

    pub fn with_params(
        mut self,
        params: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Self {
        self.format.params = params;
        self
    }
}

impl ChatWireFormat for AnthropicFormat {
    fn endpoint(&self, base_url: &str) -> String {
        let base = base_url.trim_end_matches('/');
        match self.dialect {
            AnthropicDialect::Messages => format!("{base}/messages"),
            // The model rides in the path here, so it must be escaped: Bedrock
            // ids contain `:` (e.g. `anthropic.claude-…-v1:0`).
            AnthropicDialect::BedrockInvoke => {
                format!("{base}/model/{}/invoke", encode_path_segment(&self.model))
            }
        }
    }

    fn request_body(&self, case: &TestCase) -> serde_json::Value {
        let mut body = serde_json::Map::new();
        match self.dialect {
            AnthropicDialect::Messages => {
                body.insert("model".into(), serde_json::json!(self.model));
            }
            // Bedrock takes the model from the path and requires the version
            // in the body instead.
            AnthropicDialect::BedrockInvoke => {
                body.insert(
                    "anthropic_version".into(),
                    serde_json::json!(BEDROCK_ANTHROPIC_VERSION),
                );
            }
        }
        body.insert("max_tokens".into(), serde_json::json!(self.max_tokens));
        // Top-level field, not a message: this is where the request shape forks
        // from the OpenAI wire format.
        if let Some(system) = &self.system {
            body.insert("system".into(), serde_json::json!(system));
        }
        body.insert(
            "messages".into(),
            serde_json::json!([{"role": "user", "content": case.input}]),
        );
        if let Some(params) = &self.params {
            // Validation rejects reserved keys, so params can't clobber the
            // fields above.
            body.extend(params.clone());
        }
        serde_json::Value::Object(body)
    }

    fn key_header(&self) -> KeyHeader {
        KeyHeader {
            name: "x-api-key",
            prefix: "",
        }
    }

    fn extra_headers(&self) -> &[(&'static str, &'static str)] {
        match self.dialect {
            AnthropicDialect::Messages => &[("anthropic-version", ANTHROPIC_VERSION)],
            // Bedrock carries the version in the body, and an unexpected header
            // would only add noise to the signature.
            AnthropicDialect::BedrockInvoke => &[],
        }
    }

    fn parse_response(&self, url: &str, body: &serde_json::Value) -> anyhow::Result<ChatReply> {
        Ok(ChatReply {
            text: extract_text(url, body)?,
            // `input_tokens`/`output_tokens`, not `prompt_tokens`/`completion_tokens`.
            tokens: body.get("usage").and_then(|usage| {
                Some(TokenUsage {
                    input: usage.get("input_tokens")?.as_u64()?,
                    output: usage.get("output_tokens").and_then(|v| v.as_u64())?,
                })
            }),
        })
    }

    fn cache_identity(&self, base_url: &str) -> serde_json::Value {
        // `max_tokens` is IN, unlike timeout_seconds and max_retries: a lower
        // cap truncates the answer, so it changes what the model returns rather
        // than how we call. Unset fields are OMITTED (not null) so adding a
        // config field later can't invalidate recorded cassettes. The `type`
        // discriminator differs from "openai-compatible", so keys across the
        // wire formats can never collide.
        let mut identity = serde_json::Map::new();
        identity.insert("type".into(), serde_json::json!("anthropic"));
        identity.insert("url".into(), serde_json::json!(base_url));
        identity.insert("model".into(), serde_json::json!(self.model));
        identity.insert("max_tokens".into(), serde_json::json!(self.max_tokens));
        if self.dialect == AnthropicDialect::BedrockInvoke {
            // A different deployment of the same format: different path and
            // body, so it must not share cassettes with a Messages endpoint.
            identity.insert("dialect".into(), serde_json::json!("bedrock-invoke"));
        }
        if let Some(system) = &self.system {
            identity.insert("system".into(), serde_json::json!(system));
        }
        if let Some(params) = &self.params {
            if !params.is_empty() {
                identity.insert("params".into(), serde_json::Value::Object(params.clone()));
            }
        }
        serde_json::Value::Object(identity)
    }
}

/// Percent-encode a single path segment. Bedrock model ids contain `:`, which
/// must not be mistaken for a URI delimiter (and must match what SigV4 signs).
fn encode_path_segment(segment: &str) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
    const SEGMENT: &AsciiSet = &CONTROLS
        .add(b':')
        .add(b'/')
        .add(b'?')
        .add(b'#')
        .add(b'[')
        .add(b']')
        .add(b'@')
        .add(b' ');
    utf8_percent_encode(segment, SEGMENT).to_string()
}

/// Pull the answer out of a Messages response.
///
/// `content` is an array of typed blocks, so reading `content[0].text` is wrong
/// the moment extended thinking is on: a `thinking` block precedes the text one
/// and carries no `text` field.
///
/// A policy decline arrives as **HTTP 200** with `stop_reason: "refusal"` and no
/// text. That is a failed case with a readable reason (failures are data), not a
/// body-parse error blaming the server.
fn extract_text(url: &str, parsed: &serde_json::Value) -> anyhow::Result<String> {
    let stop_reason = parsed.get("stop_reason").and_then(|v| v.as_str());
    if stop_reason == Some("refusal") {
        bail!("{url} declined to answer: the model's safety system stopped generation (stop_reason: refusal)");
    }

    let blocks = parsed
        .get("content")
        .and_then(|c| c.as_array())
        .with_context(|| format!("{url} response missing content[]"))?;

    concat_text_blocks(blocks, |block| {
        (block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .then(|| block.get("text").and_then(|t| t.as_str()))
            .flatten()
    })
    .with_context(|| {
        let reason = stop_reason.unwrap_or("none");
        format!("{url} response has no text block in content[] (stop_reason: {reason})")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Target;

    fn case(input: &str) -> TestCase {
        TestCase {
            id: "t".into(),
            input: input.into(),
            expected: None,
            trace: None,
            context: None,
        }
    }

    fn target() -> AnthropicTarget {
        AnthropicTarget::new("http://x/v1".into(), "m".into(), None, 120).unwrap()
    }

    fn bedrock() -> AnthropicTarget {
        AnthropicTarget::with_auth(
            "https://bedrock-runtime.us-east-1.amazonaws.com".into(),
            "anthropic.claude-v1:0".into(),
            ChatAuth::ApiKey(None),
            120,
            AnthropicDialect::BedrockInvoke,
        )
        .unwrap()
    }

    #[test]
    fn system_is_a_top_level_field_not_a_message() {
        let body = target()
            .with_system(Some("be terse".into()))
            .format
            .request_body(&case("hi"));
        assert_eq!(body["system"], serde_json::json!("be terse"));
        // The Messages API has no `system` role; a system message here would be
        // a 400 from the real API.
        assert_eq!(
            body["messages"],
            serde_json::json!([{"role": "user", "content": "hi"}])
        );
        assert_eq!(body["max_tokens"], serde_json::json!(DEFAULT_MAX_TOKENS));
    }

    #[test]
    fn bedrock_dialect_moves_the_model_from_body_to_path() {
        let bedrock = bedrock();
        let body = bedrock.format.request_body(&case("hi"));
        assert!(
            body.get("model").is_none(),
            "Bedrock takes it from the path"
        );
        assert_eq!(
            body["anthropic_version"],
            serde_json::json!(BEDROCK_ANTHROPIC_VERSION)
        );
        // The `:` in a Bedrock model id must be escaped, or it reads as a URI
        // delimiter and the SigV4 canonical path stops matching.
        assert_eq!(
            bedrock
                .format
                .endpoint("https://bedrock-runtime.us-east-1.amazonaws.com"),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-v1%3A0/invoke"
        );
        // The version header belongs in the body here, not the headers.
        assert!(bedrock.format.extra_headers().is_empty());
    }

    #[test]
    fn thinking_blocks_are_skipped_and_text_blocks_concatenated() {
        // `content[0].text` would be absent here — the trap this parser exists
        // to avoid.
        let response = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "let me reason about refunds"},
                {"type": "text", "text": "Refunds take "},
                {"type": "text", "text": "30 days."},
            ],
            "stop_reason": "end_turn",
        });
        assert_eq!(
            extract_text("http://x/v1/messages", &response).unwrap(),
            "Refunds take 30 days."
        );
    }

    #[test]
    fn refusal_is_a_readable_failure_not_a_parse_error() {
        // HTTP 200 with empty content — must not read as a malformed body.
        let response = serde_json::json!({"content": [], "stop_reason": "refusal"});
        let err = extract_text("http://x/v1/messages", &response).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("declined to answer"), "got: {msg}");
        assert!(msg.contains("refusal"), "got: {msg}");
    }

    #[test]
    fn missing_text_block_names_the_stop_reason() {
        let response = serde_json::json!({
            "content": [{"type": "thinking", "thinking": "…"}],
            "stop_reason": "max_tokens",
        });
        let err = extract_text("http://x/v1/messages", &response).unwrap_err();
        assert!(err.to_string().contains("max_tokens"), "got: {err}");
        let err = extract_text("http://x/v1/messages", &serde_json::json!({})).unwrap_err();
        assert!(err.to_string().contains("content[]"), "got: {err}");
    }

    #[test]
    fn empty_text_block_is_an_empty_answer_not_an_error() {
        // A model that returns nothing is a failing case downstream, not a
        // target error: the distinction keeps "failures are data" honest.
        let response = serde_json::json!({
            "content": [{"type": "text", "text": ""}],
            "stop_reason": "end_turn",
        });
        assert_eq!(extract_text("http://x/v1/messages", &response).unwrap(), "");
    }

    #[test]
    fn cache_identity_shape_is_pinned() {
        // Exact bytes are load-bearing: the shape is what cassettes are keyed
        // on, and adding a key for an unset field would invalidate them.
        assert_eq!(
            target().cache_identity().unwrap(),
            serde_json::json!({
                "type": "anthropic",
                "url": "http://x/v1",
                "model": "m",
                "max_tokens": DEFAULT_MAX_TOKENS,
            })
        );

        let mut params = serde_json::Map::new();
        params.insert("temperature".into(), serde_json::json!(0));
        let tuned = target()
            .with_system(Some("be terse".into()))
            .with_params(Some(params));
        let identity = tuned.cache_identity().unwrap();
        assert_eq!(identity["system"], serde_json::json!("be terse"));
        assert_eq!(identity["params"]["temperature"], serde_json::json!(0));
    }

    #[test]
    fn bedrock_dialect_is_a_separate_keyspace() {
        // Same format, different path and body: sharing cassettes with a
        // Messages endpoint would replay an answer for a request never made.
        let identity = bedrock().cache_identity().unwrap();
        assert_eq!(identity["dialect"], serde_json::json!("bedrock-invoke"));
        assert!(
            target().cache_identity().unwrap().get("dialect").is_none(),
            "the plain Messages identity must keep its pinned shape"
        );
    }

    #[test]
    fn max_tokens_changes_the_identity_but_operational_knobs_do_not() {
        // max_tokens truncates the answer, so it is request content.
        assert_ne!(
            target().with_max_tokens(256).cache_identity(),
            target().with_max_tokens(512).cache_identity()
        );
        // Retries and timeout change how we call, not what the model answers.
        let slow = AnthropicTarget::new("http://x/v1".into(), "m".into(), None, 5).unwrap();
        assert_eq!(
            target().with_max_retries(9).cache_identity(),
            slow.cache_identity()
        );
    }

    #[test]
    fn identity_cannot_collide_with_an_openai_target() {
        // Same url and model, different wire format: the `type` discriminator
        // keeps the keyspaces disjoint, so no recorded openai-compatible
        // cassette is invalidated by this target existing.
        let openai = crate::openai::OpenAiCompatTarget::new(
            "http://x/v1".into(),
            "m".into(),
            None,
            evalcore_config::DEFAULT_TIMEOUT_SECONDS,
        )
        .unwrap();
        assert_ne!(target().cache_identity(), openai.cache_identity());
    }
}
