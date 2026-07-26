//! Gemini wire format: `POST {url}/models/{model}:generateContent`.
//!
//! The third wire format, and the one that breaks every assumption the first
//! two shared: the model rides in the URL path (and in the *method* suffix
//! after a colon), the request is `contents[].parts[]` rather than `messages`,
//! and generation tunables nest under `generationConfig`.
//!
//! Google also ships an OpenAI-compatible endpoint that works with
//! `type: openai-compatible` today. This format exists for the native-only
//! surface (safety settings, `systemInstruction`, thought parts).

use anyhow::{bail, Context};

use crate::chat::{concat_text_blocks, ChatAuth, ChatReply, ChatTarget, ChatWireFormat, KeyHeader};
use crate::types::{TestCase, TokenUsage};

/// The Gemini generateContent wire format.
pub struct GeminiFormat {
    model: String,
    system: Option<String>,
    params: Option<serde_json::Map<String, serde_json::Value>>,
}

/// A Gemini target.
pub type GeminiTarget = ChatTarget<GeminiFormat>;

impl GeminiTarget {
    pub fn new(
        base_url: String,
        model: String,
        api_key: Option<String>,
        timeout_seconds: u64,
    ) -> anyhow::Result<Self> {
        ChatTarget::build(
            base_url,
            ChatAuth::ApiKey(api_key),
            timeout_seconds,
            GeminiFormat {
                model,
                system: None,
                params: None,
            },
        )
    }

    pub fn with_system(mut self, system: Option<String>) -> Self {
        self.format.system = system;
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

impl ChatWireFormat for GeminiFormat {
    fn endpoint(&self, base_url: &str) -> String {
        format!(
            "{}/models/{}:generateContent",
            base_url.trim_end_matches('/'),
            self.model
        )
    }

    fn request_body(&self, case: &TestCase) -> serde_json::Value {
        let mut body = serde_json::Map::new();
        body.insert(
            "contents".into(),
            serde_json::json!([{"role": "user", "parts": [{"text": case.input}]}]),
        );
        // A `Content` value, not a bare string — same shape as a turn.
        if let Some(system) = &self.system {
            body.insert(
                "systemInstruction".into(),
                serde_json::json!({"parts": [{"text": system}]}),
            );
        }
        if let Some(params) = &self.params {
            // Validation rejects reserved keys, so params can't clobber the
            // fields above. Generation tunables nest under `generationConfig`
            // here rather than sitting at the top level; EvalCore passes the
            // structure through instead of inventing a translation layer.
            body.extend(params.clone());
        }
        serde_json::Value::Object(body)
    }

    fn key_header(&self) -> KeyHeader {
        // A header, never the `?key=` query parameter the API also accepts:
        // the request URL appears verbatim in error messages and in reports,
        // so a query-string secret would leak straight into them.
        KeyHeader {
            name: "x-goog-api-key",
            prefix: "",
        }
    }

    fn parse_response(&self, url: &str, body: &serde_json::Value) -> anyhow::Result<ChatReply> {
        Ok(ChatReply {
            text: extract_text(url, body)?,
            tokens: body.get("usageMetadata").and_then(|usage| {
                Some(TokenUsage {
                    input: usage.get("promptTokenCount")?.as_u64()?,
                    // Absent when everything was filtered out; treat as zero
                    // rather than dropping the input count on the floor.
                    output: usage
                        .get("candidatesTokenCount")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                })
            }),
        })
    }

    fn cache_identity(&self, base_url: &str) -> serde_json::Value {
        let mut identity = serde_json::Map::new();
        identity.insert("type".into(), serde_json::json!("gemini"));
        identity.insert("url".into(), serde_json::json!(base_url));
        identity.insert("model".into(), serde_json::json!(self.model));
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

/// Pull the answer out of a generateContent response.
///
/// Two ways this returns nothing useful, and both are declines rather than
/// malformed bodies (failures are data):
///
/// - the whole prompt was blocked, giving `promptFeedback.blockReason` and no
///   candidates at all;
/// - the candidate stopped for a non-`STOP` reason such as `SAFETY`.
///
/// Otherwise the answer is the concatenation of the candidate's text parts.
/// `parts[0].text` is wrong here for the same reason it is wrong for Anthropic:
/// with thinking enabled a `thought` part comes first.
fn extract_text(url: &str, parsed: &serde_json::Value) -> anyhow::Result<String> {
    if let Some(reason) = parsed
        .get("promptFeedback")
        .and_then(|f| f.get("blockReason"))
        .and_then(|r| r.as_str())
    {
        bail!("{url} blocked the prompt before generating (blockReason: {reason})");
    }

    let candidate = parsed
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .with_context(|| format!("{url} response missing candidates[]"))?;
    let finish_reason = candidate
        .get("finishReason")
        .and_then(|r| r.as_str())
        .unwrap_or("none");

    let parts = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array());
    let text = parts.and_then(|parts| {
        concat_text_blocks(parts, |part| {
            // A `thought` part also carries `text`; it is the model's
            // reasoning, not its answer, so it must not be concatenated in.
            if part.get("thought").and_then(|t| t.as_bool()) == Some(true) {
                return None;
            }
            part.get("text").and_then(|t| t.as_str())
        })
    });

    match text {
        Some(text) => Ok(text),
        // A truncated or filtered candidate carries no text part at all.
        None if finish_reason != "STOP" && finish_reason != "none" => bail!(
            "{url} returned no answer: generation stopped early (finishReason: {finish_reason})"
        ),
        None => bail!("{url} response has no text part in candidates[0].content.parts[]"),
    }
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

    fn target() -> GeminiTarget {
        GeminiTarget::new("http://x/v1beta".into(), "gemini-2.5-pro".into(), None, 120).unwrap()
    }

    #[test]
    fn model_rides_in_the_path_after_a_colon() {
        assert_eq!(
            target().format.endpoint("http://x/v1beta"),
            "http://x/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn builds_contents_parts_and_system_instruction() {
        let body = target()
            .with_system(Some("be terse".into()))
            .format
            .request_body(&case("hi"));
        assert_eq!(
            body["contents"],
            serde_json::json!([{"role": "user", "parts": [{"text": "hi"}]}])
        );
        // systemInstruction is a Content value, not a bare string.
        assert_eq!(
            body["systemInstruction"],
            serde_json::json!({"parts": [{"text": "be terse"}]})
        );
    }

    #[test]
    fn params_nest_under_generation_config_verbatim() {
        let mut params = serde_json::Map::new();
        params.insert(
            "generationConfig".into(),
            serde_json::json!({"temperature": 0}),
        );
        let body = target()
            .with_params(Some(params))
            .format
            .request_body(&case("hi"));
        assert_eq!(
            body["generationConfig"]["temperature"],
            serde_json::json!(0)
        );
    }

    #[test]
    fn api_key_never_rides_in_the_url() {
        // A `?key=` query parameter would put the secret into every error
        // message (which quote the URL) and into reports.
        let header = target().format.key_header();
        assert_eq!(header.name, "x-goog-api-key");
        assert!(!target().format.endpoint("http://x/v1beta").contains("key="));
    }

    #[test]
    fn thought_parts_are_skipped_and_text_parts_concatenated() {
        let response = serde_json::json!({
            "candidates": [{
                "content": {"parts": [
                    {"text": "reasoning about refunds", "thought": true},
                    {"text": "Refunds take "},
                    {"text": "30 days."},
                ]},
                "finishReason": "STOP",
            }],
        });
        assert_eq!(
            extract_text("http://x/gen", &response).unwrap(),
            "Refunds take 30 days."
        );
    }

    #[test]
    fn blocked_prompt_and_filtered_candidate_are_readable_failures() {
        let blocked = serde_json::json!({
            "promptFeedback": {"blockReason": "SAFETY"},
        });
        let err = extract_text("http://x/gen", &blocked)
            .unwrap_err()
            .to_string();
        assert!(err.contains("blocked the prompt"), "got: {err}");
        assert!(err.contains("SAFETY"), "got: {err}");

        let filtered = serde_json::json!({
            "candidates": [{"content": {"parts": []}, "finishReason": "SAFETY"}],
        });
        let err = extract_text("http://x/gen", &filtered)
            .unwrap_err()
            .to_string();
        assert!(err.contains("stopped early"), "got: {err}");
        assert!(err.contains("SAFETY"), "got: {err}");
    }

    #[test]
    fn missing_candidates_names_the_field() {
        let err = extract_text("http://x/gen", &serde_json::json!({}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("candidates[]"), "got: {err}");
    }

    #[test]
    fn cache_identity_shape_is_pinned_and_disjoint() {
        assert_eq!(
            target().cache_identity().unwrap(),
            serde_json::json!({
                "type": "gemini",
                "url": "http://x/v1beta",
                "model": "gemini-2.5-pro",
            })
        );
        // Same url and model under a different wire format must not collide —
        // Gemini's own OpenAI-compatible endpoint is a real, common setup.
        let openai = crate::openai::OpenAiCompatTarget::new(
            "http://x/v1beta".into(),
            "gemini-2.5-pro".into(),
            None,
            120,
        )
        .unwrap();
        assert_ne!(target().cache_identity(), openai.cache_identity());
    }
}
