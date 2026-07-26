//! OpenAI chat-completions wire format: `POST {url}/chat/completions`.
//!
//! The original wire format, and still the one most vendors speak — a vendor
//! that speaks it needs zero new code. Shared HTTP behavior lives in
//! [`crate::chat`]; this file is only the OpenAI-specific parts.

use anyhow::Context;

use crate::chat::{ChatAuth, ChatReply, ChatTarget, ChatWireFormat, KeyHeader};
use crate::types::{TestCase, TokenUsage};

/// The OpenAI chat-completions wire format.
pub struct OpenAiFormat {
    model: String,
    system: Option<String>,
    params: Option<serde_json::Map<String, serde_json::Value>>,
}

/// A target speaking the OpenAI chat-completions format.
pub type OpenAiCompatTarget = ChatTarget<OpenAiFormat>;

impl OpenAiCompatTarget {
    /// Build a target with a per-attempt request timeout. Fallible because the
    /// underlying [`reqwest::Client`] is constructed here (TLS init can fail);
    /// the error propagates out of the factory ([`crate::target::build_target_with`],
    /// which has no target name) for the caller to contextualize (e.g. the CLI
    /// attaches the target's config name; `build_scorers` labels judge targets).
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
            OpenAiFormat {
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

impl ChatWireFormat for OpenAiFormat {
    fn endpoint(&self, base_url: &str) -> String {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
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

    fn key_header(&self) -> KeyHeader {
        KeyHeader {
            name: "authorization",
            prefix: "Bearer ",
        }
    }

    fn parse_response(&self, url: &str, body: &serde_json::Value) -> anyhow::Result<ChatReply> {
        let text = body["choices"][0]["message"]["content"]
            .as_str()
            .with_context(|| format!("{url} response missing choices[0].message.content"))?
            .to_string();
        Ok(ChatReply {
            text,
            tokens: body.get("usage").and_then(|usage| {
                Some(TokenUsage {
                    input: usage.get("prompt_tokens")?.as_u64()?,
                    output: usage.get("completion_tokens").and_then(|v| v.as_u64())?,
                })
            }),
        })
    }

    fn cache_identity(&self, base_url: &str) -> serde_json::Value {
        // Everything that changes the request must be here (system, params) —
        // a temperature change must never replay a stale answer. max_retries
        // and cost rates stay excluded: they change how we call and account,
        // not what the model would answer. Unset fields are OMITTED (not
        // null) so pre-existing cassettes keep their keys.
        let mut identity = serde_json::Map::new();
        identity.insert("type".into(), serde_json::json!("openai-compatible"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Target;

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
}
