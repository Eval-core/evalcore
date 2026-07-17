//! The `http` target: evaluate an arbitrary HTTP/JSON endpoint — usually your
//! own deployed app's REST API — through the same record/replay cache and
//! retry policy as an LLM target.
//!
//! Design rule (see the crate CLAUDE.md): targets are protocol-shaped, not
//! vendor SDKs. This one speaks plain HTTP+JSON so any service is reachable
//! without new code.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use async_trait::async_trait;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::Method;

use crate::target::{retry_with_backoff, AttemptError, Target};
use crate::types::{TargetOutput, TestCase};

/// The `{{input}}` placeholder substituted into `url` and `body`.
const PLACEHOLDER: &str = "{{input}}";

/// Map a config method string to a [`reqwest::Method`], normalizing case.
/// Config validation already restricts the set; this fails fast for callers
/// that build a target directly.
pub(crate) fn parse_method(raw: &str) -> anyhow::Result<Method> {
    match raw.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        other => bail!("unsupported HTTP method {other:?} (expected GET, POST, PUT, or PATCH)"),
    }
}

/// Calls an HTTP/JSON endpoint per case. `{{input}}` is percent-encoded into
/// `url` (every non-alphanumeric byte) and substituted verbatim into every
/// string value of the JSON `body`. Transient failures (429/5xx/transport)
/// retry with the shared deterministic backoff.
pub struct HttpTarget {
    client: reqwest::Client,
    /// Pre-substitution URL; the cache identity keys on this template.
    url_template: String,
    method: Method,
    /// Static headers with lowercased names (deterministic, case-insensitive).
    headers: BTreeMap<String, String>,
    api_key: Option<String>,
    /// Lowercased header name the key rides in (default `authorization`).
    auth_header: String,
    auth_prefix: String,
    max_retries: u32,
    /// Pre-substitution JSON body template; the cache identity keys on this.
    body: Option<serde_json::Value>,
    response_path: Option<String>,
}

impl HttpTarget {
    /// A POST/GET/… target against `url` with default auth conventions
    /// (`authorization: Bearer <key>`) and the default retry budget.
    pub fn new(url: String, method: Method) -> Self {
        Self {
            client: reqwest::Client::new(),
            url_template: url,
            method,
            headers: BTreeMap::new(),
            api_key: None,
            auth_header: "authorization".into(),
            auth_prefix: "Bearer ".into(),
            max_retries: evalcore_config::DEFAULT_MAX_RETRIES,
            body: None,
            response_path: None,
        }
    }

    /// Static headers; names are lowercased so Content-Type detection and the
    /// cache identity are case-insensitive and deterministic.
    pub fn with_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers = headers
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect();
        self
    }

    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key;
        self
    }

    pub fn with_auth_header(mut self, auth_header: String) -> Self {
        self.auth_header = auth_header.to_ascii_lowercase();
        self
    }

    pub fn with_auth_prefix(mut self, auth_prefix: String) -> Self {
        self.auth_prefix = auth_prefix;
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_body(mut self, body: Option<serde_json::Value>) -> Self {
        self.body = body;
        self
    }

    pub fn with_response_path(mut self, response_path: Option<String>) -> Self {
        self.response_path = response_path;
        self
    }

    async fn attempt(&self, url: &str, body: Option<&[u8]>) -> Result<TargetOutput, AttemptError> {
        let start = Instant::now();
        let mut request = self.client.request(self.method.clone(), url);
        // Default Content-Type only when we send a body, and only when the user
        // hasn't set one themselves (header names are already lowercased).
        if body.is_some() && !self.headers.contains_key("content-type") {
            request = request.header("content-type", "application/json");
        }
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        if let Some(key) = &self.api_key {
            request = request.header(&self.auth_header, format!("{}{key}", self.auth_prefix));
        }
        if let Some(body) = body {
            request = request.body(body.to_vec());
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
        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            let snippet: String = text.chars().take(200).collect();
            let message = format!("{url} returned {status}: {snippet}");
            return if status.as_u16() == 429 || status.is_server_error() {
                Err(AttemptError::Transient {
                    message,
                    retry_after,
                })
            } else {
                Err(AttemptError::Permanent(anyhow!(message)))
            };
        }

        let output_text = match &self.response_path {
            Some(pointer) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
                    let snippet: String = text.chars().take(200).collect();
                    AttemptError::Permanent(anyhow!(
                        "{url} returned {status} with non-JSON body: {snippet}"
                    ))
                })?;
                let found = parsed.pointer(pointer).ok_or_else(|| {
                    let snippet: String = text.chars().take(200).collect();
                    AttemptError::Permanent(anyhow!(
                        "{url} response has no value at JSON Pointer {pointer:?}: {snippet}"
                    ))
                })?;
                match found {
                    // A string is used as-is; anything else is serialized
                    // compactly so the scorers always see a string.
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                }
            }
            None => text,
        };

        Ok(TargetOutput {
            text: output_text,
            latency_ms: start.elapsed().as_millis() as u64,
            // Generic HTTP APIs have no standard usage shape, so v1 reports no
            // tokens (and therefore no cost) for http targets.
            tokens: None,
        })
    }
}

/// Substitute `{{input}}` into every string value of a JSON template,
/// recursively. Object keys are never touched; non-string scalars pass through.
fn substitute_body(value: &serde_json::Value, input: &str) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(s.replace(PLACEHOLDER, input)),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(|v| substitute_body(v, input)).collect())
        }
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, v) in map {
                out.insert(key.clone(), substitute_body(v, input));
            }
            serde_json::Value::Object(out)
        }
        other => other.clone(),
    }
}

#[async_trait]
impl Target for HttpTarget {
    async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
        // Substitution is per-case and identical across retries, so do it once.
        let encoded = utf8_percent_encode(&case.input, NON_ALPHANUMERIC).to_string();
        let url = self.url_template.replace(PLACEHOLDER, &encoded);
        let body_bytes = match &self.body {
            Some(template) => {
                let substituted = substitute_body(template, &case.input);
                Some(
                    serde_json::to_vec(&substituted)
                        .context("failed to serialize http target request body")?,
                )
            }
            None => None,
        };

        retry_with_backoff(self.max_retries, || {
            self.attempt(&url, body_bytes.as_deref())
        })
        .await
    }

    fn cache_identity(&self) -> Option<serde_json::Value> {
        // Everything that changes the request is IN (url/method/headers/body/
        // response_path); secrets and call-mechanics (api_key_env, auth_header,
        // auth_prefix, max_retries) are OUT. Unset fields are OMITTED, not
        // null, so adding a field never invalidates existing cassettes.
        let mut http = serde_json::Map::new();
        http.insert(
            "url".into(),
            serde_json::Value::String(self.url_template.clone()),
        );
        http.insert(
            "method".into(),
            serde_json::Value::String(self.method.as_str().to_string()),
        );
        if !self.headers.is_empty() {
            let mut headers = serde_json::Map::new();
            for (name, value) in &self.headers {
                headers.insert(name.clone(), serde_json::Value::String(value.clone()));
            }
            http.insert("headers".into(), serde_json::Value::Object(headers));
        }
        if let Some(body) = &self.body {
            http.insert("body".into(), body.clone());
        }
        if let Some(path) = &self.response_path {
            http.insert(
                "response_path".into(),
                serde_json::Value::String(path.clone()),
            );
        }
        let mut identity = serde_json::Map::new();
        identity.insert("http".into(), serde_json::Value::Object(http));
        Some(serde_json::Value::Object(identity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_method_normalizes_case_and_rejects_unknown() {
        assert_eq!(parse_method("post").unwrap(), Method::POST);
        assert_eq!(parse_method("GeT").unwrap(), Method::GET);
        assert!(parse_method("DELETE").is_err());
    }

    #[test]
    fn substitute_body_only_touches_string_values() {
        let template = serde_json::json!({
            "question": "{{input}}",
            "{{input}}": "key stays literal",
            "nested": {"q": "ask {{input}} now"},
            "list": ["{{input}}", 7],
            "count": 3,
        });
        let out = substitute_body(&template, "refunds");
        assert_eq!(out["question"], serde_json::json!("refunds"));
        // Keys are never substituted.
        assert_eq!(out["{{input}}"], serde_json::json!("key stays literal"));
        assert_eq!(out["nested"]["q"], serde_json::json!("ask refunds now"));
        assert_eq!(out["list"][0], serde_json::json!("refunds"));
        assert_eq!(out["list"][1], serde_json::json!(7));
        assert_eq!(out["count"], serde_json::json!(3));
    }

    #[test]
    fn cache_identity_omits_unset_fields_and_pins_shape() {
        // Bare target: only url + method (method always present, normalized).
        let bare = HttpTarget::new("https://api.myapp.com/chat".into(), Method::POST);
        assert_eq!(
            bare.cache_identity().unwrap(),
            serde_json::json!({
                "http": {"url": "https://api.myapp.com/chat", "method": "POST"}
            }),
            "adding keys for unset fields would invalidate existing cassettes"
        );

        // Set fields appear; headers are lowercased and sorted.
        let full = HttpTarget::new("https://api.myapp.com/chat".into(), Method::PUT)
            .with_headers(BTreeMap::from([("X-Tenant".into(), "acme".into())]))
            .with_body(Some(serde_json::json!({"question": "{{input}}"})))
            .with_response_path(Some("/answer".into()));
        assert_eq!(
            full.cache_identity().unwrap(),
            serde_json::json!({
                "http": {
                    "url": "https://api.myapp.com/chat",
                    "method": "PUT",
                    "headers": {"x-tenant": "acme"},
                    "body": {"question": "{{input}}"},
                    "response_path": "/answer",
                }
            })
        );
    }

    #[test]
    fn cache_identity_never_leaks_auth_or_secrets() {
        let target = HttpTarget::new("https://api.myapp.com/chat?q={{input}}".into(), Method::GET)
            .with_api_key(Some("super-secret-value".into()))
            .with_auth_header("x-api-key".into())
            .with_auth_prefix("Token ".into())
            .with_max_retries(9);
        let identity = serde_json::to_string(&target.cache_identity().unwrap()).unwrap();
        for forbidden in [
            "super-secret-value",
            "x-api-key",
            "Token ",
            "api_key",
            "auth",
            "max_retries",
            "9",
        ] {
            assert!(
                !identity.contains(forbidden),
                "identity must not contain {forbidden:?}, got: {identity}"
            );
        }
    }
}
