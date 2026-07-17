//! `evals.yaml` schema, parsing, and validation.
//!
//! This crate is pure data: no network, no engine logic, no I/O beyond the
//! caller handing us file contents. Every EvalCore feature starts life here
//! as a config surface — the YAML file is the product's primary interface.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("invalid config: {0}")]
    Invalid(String),
}

/// Root of an `evals.yaml` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalConfig {
    /// Named targets; `evalcore run --target <name>` selects one.
    pub targets: BTreeMap<String, TargetConfig>,
    /// Datasets of test cases, merged in order.
    pub datasets: Vec<DatasetConfig>,
    /// Scorers applied to every case.
    pub scorers: Vec<ScorerConfig>,
    #[serde(default)]
    pub run: RunConfig,
}

impl EvalConfig {
    /// Parse and validate a config from YAML text.
    pub fn from_yaml_str(yaml: &str) -> Result<Self, ConfigError> {
        let config: Self = serde_yaml::from_str(yaml)?;
        config.validate()?;
        Ok(config)
    }

    /// Read, parse, and validate a config file.
    ///
    /// Relative paths inside the config (dataset files, scorer commands) are
    /// interpreted relative to the config file's directory by the CLI.
    pub fn from_path(path: &Path) -> Result<Self, ConfigError> {
        let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_yaml_str(&raw)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.targets.is_empty() {
            return Err(ConfigError::Invalid(
                "at least one target is required".into(),
            ));
        }
        if self.datasets.is_empty() {
            return Err(ConfigError::Invalid(
                "at least one dataset is required".into(),
            ));
        }
        if self.scorers.is_empty() {
            return Err(ConfigError::Invalid(
                "at least one scorer is required".into(),
            ));
        }
        if self.run.concurrency == 0 {
            return Err(ConfigError::Invalid(
                "run.concurrency must be at least 1".into(),
            ));
        }
        if let Some(budget) = self.run.budget_usd {
            if budget <= 0.0 {
                return Err(ConfigError::Invalid(format!(
                    "run.budget_usd must be positive, got {budget}"
                )));
            }
        }
        for (name, target) in &self.targets {
            let cost = match target {
                TargetConfig::OpenaiCompatible {
                    cost,
                    params,
                    timeout_seconds,
                    ..
                } => {
                    if *timeout_seconds == 0 {
                        return Err(ConfigError::Invalid(format!(
                            "target {name:?}: timeout_seconds must be at least 1"
                        )));
                    }
                    if let Some(params) = params {
                        for reserved in ["model", "messages", "stream"] {
                            if params.contains_key(reserved) {
                                return Err(ConfigError::Invalid(format!(
                                    "target {name:?}: params may not set {reserved:?} \
                                     (model/messages are managed by EvalCore; streaming \
                                     responses are unsupported)"
                                )));
                            }
                        }
                    }
                    cost
                }
                TargetConfig::Trace { cost } => cost,
                TargetConfig::Shell { .. } => &None,
                TargetConfig::Http {
                    url,
                    method,
                    headers,
                    api_key_env,
                    auth_header,
                    auth_prefix,
                    body,
                    response_path,
                    timeout_seconds,
                    ..
                } => {
                    if *timeout_seconds == 0 {
                        return Err(ConfigError::Invalid(format!(
                            "target {name:?}: timeout_seconds must be at least 1"
                        )));
                    }
                    validate_http_target(
                        name,
                        url,
                        method,
                        headers,
                        api_key_env,
                        auth_header,
                        auth_prefix,
                        body,
                        response_path,
                    )?;
                    &None
                }
            };
            if let Some(cost) = cost {
                if cost.input_per_1m < 0.0 || cost.output_per_1m < 0.0 {
                    return Err(ConfigError::Invalid(format!(
                        "target {name:?} has negative cost rates"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Structural checks for an `http` target. Deeper resolution (env vars,
/// method parsing) happens in the factory; this only rejects configs that can
/// never produce a meaningful request.
#[allow(clippy::too_many_arguments)]
fn validate_http_target(
    name: &str,
    url: &str,
    method: &str,
    headers: &Option<BTreeMap<String, String>>,
    api_key_env: &Option<String>,
    auth_header: &Option<String>,
    auth_prefix: &Option<String>,
    body: &Option<serde_json::Value>,
    response_path: &Option<String>,
) -> Result<(), ConfigError> {
    if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ConfigError::Invalid(format!(
            "target {name:?}: url must be a non-empty http:// or https:// URL"
        )));
    }
    const ALLOWED_METHODS: [&str; 4] = ["GET", "POST", "PUT", "PATCH"];
    let normalized = method.to_ascii_uppercase();
    if !ALLOWED_METHODS.contains(&normalized.as_str()) {
        return Err(ConfigError::Invalid(format!(
            "target {name:?}: method {method:?} is not one of GET, POST, PUT, PATCH"
        )));
    }
    if normalized == "GET" && body.is_some() {
        return Err(ConfigError::Invalid(format!(
            "target {name:?}: a GET request may not carry a body"
        )));
    }
    // Without a `{{input}}` anchor every case would send an identical request,
    // which is never what the user means.
    let body_has_input = body
        .as_ref()
        .map(|value| {
            serde_json::to_string(value)
                .unwrap_or_default()
                .contains("{{input}}")
        })
        .unwrap_or(false);
    if !url.contains("{{input}}") && !body_has_input {
        return Err(ConfigError::Invalid(format!(
            "target {name:?}: neither url nor body contains {{{{input}}}}; \
             every case would send the same request"
        )));
    }
    if api_key_env.is_none() && (auth_header.is_some() || auth_prefix.is_some()) {
        return Err(ConfigError::Invalid(format!(
            "target {name:?}: auth_header/auth_prefix require api_key_env"
        )));
    }
    // The API key rides in the auth header, so a static `headers:` entry with
    // the same (case-insensitive) name would send two conflicting header lines.
    if api_key_env.is_some() {
        if let Some(headers) = headers {
            let auth_name = auth_header
                .as_deref()
                .unwrap_or("authorization")
                .to_ascii_lowercase();
            if let Some(clash) = headers
                .keys()
                .find(|name| name.to_ascii_lowercase() == auth_name)
            {
                return Err(ConfigError::Invalid(format!(
                    "target {name:?}: header {clash:?} collides with the auth header \
                     (the api_key_env key is sent there); remove it from headers"
                )));
            }
        }
    }
    if let Some(path) = response_path {
        if !path.starts_with('/') {
            return Err(ConfigError::Invalid(format!(
                "target {name:?}: response_path must be an RFC 6901 JSON Pointer \
                 starting with '/'"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Maximum in-flight cases.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// Abort scheduling new cases once accumulated cost reaches this (USD).
    /// Requires the target to declare `cost` rates; costed from token usage,
    /// so replayed runs count their recorded (virtual) cost too.
    #[serde(default)]
    pub budget_usd: Option<f64>,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            concurrency: default_concurrency(),
            budget_usd: None,
        }
    }
}

fn default_concurrency() -> usize {
    4
}

/// Default number of retries for transient HTTP failures (429/5xx/transport).
pub const DEFAULT_MAX_RETRIES: u32 = 2;

fn default_max_retries() -> u32 {
    DEFAULT_MAX_RETRIES
}

/// Default per-attempt request timeout, in seconds, for HTTP-based targets.
/// The budget covers a single attempt (connect + reading the response body);
/// each retry gets a fresh budget.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

fn default_timeout_seconds() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

/// USD prices per **1 million** tokens, as published by the provider. EvalCore
/// deliberately ships no pricing table — prices change and differ per
/// deployment, so they're config.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostConfig {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

/// The thing being evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum TargetConfig {
    /// Run a shell command; the case input is piped to stdin, stdout is the output.
    Shell { cmd: String },
    /// POST to `{url}/chat/completions` in the OpenAI wire format.
    OpenaiCompatible {
        /// Base URL, e.g. `https://api.openai.com/v1`.
        url: String,
        model: String,
        /// Name of the environment variable holding the API key. Secrets are
        /// never written into the YAML itself.
        #[serde(default)]
        api_key_env: Option<String>,
        /// Retries on transient failures (429/5xx/network), with exponential
        /// backoff honoring `Retry-After`.
        #[serde(default = "default_max_retries")]
        max_retries: u32,
        /// Per-attempt total time budget in seconds (connect + reading the
        /// response body). Each retry gets a fresh budget. Must be at least 1.
        #[serde(default = "default_timeout_seconds")]
        timeout_seconds: u64,
        /// Token prices; enables per-case cost reporting and `run.budget_usd`.
        #[serde(default)]
        cost: Option<CostConfig>,
        /// System prompt sent before each case's input.
        #[serde(default)]
        system: Option<String>,
        /// Extra request-body fields passed through verbatim (temperature,
        /// max_tokens, top_p, …). EvalCore doesn't enumerate provider params —
        /// protocols over SDKs. `model`, `messages`, and `stream` are
        /// reserved and rejected at validation.
        #[serde(default)]
        params: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// Ingest recorded agent traces instead of invoking anything: each case
    /// names a trace file (`{"id": ..., "trace": "traces/run1.json"}`), in
    /// EvalCore's native trajectory format or OTel/OpenInference JSON export.
    /// Pair with the `trajectory` scorer.
    Trace {
        /// Token prices applied to usage extracted from trace spans; enables
        /// cost reporting and `run.budget_usd` for trace runs.
        #[serde(default)]
        cost: Option<CostConfig>,
    },
    /// Call an arbitrary HTTP/JSON endpoint — typically your own deployed
    /// app's REST API — so it can be evaluated through the record/replay cache
    /// like any LLM target. `{{input}}` is substituted into `url`
    /// (percent-encoded — every non-alphanumeric byte) and into every string
    /// value of `body` (verbatim).
    Http {
        /// Request URL; `{{input}}` is percent-encoded when substituted.
        url: String,
        /// HTTP method: GET, POST, PUT, or PATCH (case-insensitive). GET may
        /// not carry a `body`.
        #[serde(default = "default_http_method")]
        method: String,
        /// Static request headers, sent verbatim. Never put secrets here:
        /// header values are hashed into the cache identity and persisted in
        /// the committed `.evalcore/cache.db`. Use `api_key_env` for
        /// credentials — it never enters the cache. Keys are matched
        /// case-insensitively.
        #[serde(default)]
        headers: Option<BTreeMap<String, String>>,
        /// Name of the environment variable holding the API key. Secrets are
        /// never written into the YAML itself.
        #[serde(default)]
        api_key_env: Option<String>,
        /// Header the API key is sent in (default `authorization`). Only valid
        /// alongside `api_key_env`.
        #[serde(default)]
        auth_header: Option<String>,
        /// Prefix prepended to the key (default `"Bearer "`). For an
        /// `x-api-key` style header set both `auth_header: x-api-key` and
        /// `auth_prefix: ""`. Only valid alongside `api_key_env`.
        #[serde(default)]
        auth_prefix: Option<String>,
        /// Retries on transient failures (429/5xx/network), with exponential
        /// backoff honoring `Retry-After`.
        #[serde(default = "default_max_retries")]
        max_retries: u32,
        /// Per-attempt total time budget in seconds (connect + reading the
        /// response body). Each retry gets a fresh budget. Must be at least 1.
        #[serde(default = "default_timeout_seconds")]
        timeout_seconds: u64,
        /// JSON request body template; `{{input}}` inside any string value is
        /// replaced with the case input. Omit to send no body.
        #[serde(default)]
        body: Option<serde_json::Value>,
        /// RFC 6901 JSON Pointer into the JSON response (e.g. `/answer`).
        /// Omitted: the raw response body text is the output. A pointer that
        /// resolves to JSON `null` yields the literal string "null"; only an
        /// absent path is an error.
        #[serde(default)]
        response_path: Option<String>,
    },
}

fn default_http_method() -> String {
    "POST".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetConfig {
    /// JSONL file of test cases, relative to the config file.
    pub file: PathBuf,
}

/// How outputs are judged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ScorerConfig {
    /// Pass if the output contains `value`.
    Contains {
        value: String,
        #[serde(default = "default_true")]
        case_sensitive: bool,
    },
    /// Pass if the output matches the regular expression.
    Regex { pattern: String },
    /// Pass if the output equals `value`, or the case's `expected` field when
    /// `value` is omitted.
    Exact {
        #[serde(default)]
        value: Option<String>,
    },
    /// Any-language escape hatch: the command receives
    /// `{"input", "output", "expected"}` as JSON on stdin and must print
    /// `{"score": 0.0..=1.0, "passed"?: bool, "reason"?: string}` on stdout.
    Subprocess { cmd: String },
    /// Assert on an agent trajectory (tool calls, ordering, step budget).
    /// Requires a `trace` target, whose output is the normalized trajectory.
    Trajectory { rules: Vec<TrajectoryRule> },
    /// LLM-as-judge: grade the output against a rubric using any
    /// OpenAI-compatible endpoint. Judge calls go through the record/replay
    /// cache, so replayed verdicts are deterministic.
    Judge {
        /// Base URL, e.g. `https://api.openai.com/v1`.
        url: String,
        model: String,
        /// What the judge should assess, e.g. "Is the answer grounded in the
        /// provided context?".
        rubric: String,
        #[serde(default)]
        api_key_env: Option<String>,
        /// Minimum score (0.0..=1.0) to pass.
        #[serde(default = "default_judge_threshold")]
        threshold: f64,
    },
}

fn default_true() -> bool {
    true
}

fn default_judge_threshold() -> f64 {
    0.5
}

/// One trajectory assertion. Untagged: the distinctive required key
/// (`must_call` / `must_not_call` / `max_steps`) selects the variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TrajectoryRule {
    /// At least one call of this tool must exist (optionally with matching
    /// arguments, optionally only counting calls after another tool ran).
    MustCall {
        must_call: String,
        /// Argument constraints: field name → matcher.
        #[serde(default)]
        with: BTreeMap<String, FieldMatcher>,
        /// Only count calls strictly after the first call of this tool.
        #[serde(default)]
        after: Option<String>,
    },
    /// This tool must never be called — or, with `before`, never before the
    /// first call of another tool (if that tool never runs, any call fails).
    MustNotCall {
        must_not_call: String,
        #[serde(default)]
        before: Option<String>,
    },
    /// The trajectory must contain at most this many tool calls.
    MaxSteps { max_steps: usize },
}

/// Matches one argument field of a tool call. Both constraints may be given;
/// all present constraints must hold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldMatcher {
    /// Substring match on the field rendered as a string.
    #[serde(default)]
    pub contains: Option<String>,
    /// Exact JSON equality.
    #[serde(default)]
    pub equals: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
targets:
  echo:
    type: shell
    cmd: "cat"
  api:
    type: openai-compatible
    url: https://api.openai.com/v1
    model: gpt-test
    api_key_env: OPENAI_API_KEY
    max_retries: 5
    timeout_seconds: 90
    cost:
      input_per_1m: 0.15
      output_per_1m: 0.60
    system: "You are a terse support agent."
    params:
      temperature: 0
      max_tokens: 256
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: refund
  - type: regex
    pattern: "^[A-Z]"
  - type: exact
  - type: subprocess
    cmd: python3 scorers/faithfulness.py
  - type: judge
    url: https://api.openai.com/v1
    model: judge-model
    rubric: "Is the answer grounded in the provided context?"
    api_key_env: OPENAI_API_KEY
run:
  concurrency: 8
  budget_usd: 5.0
"#;

    #[test]
    fn parses_full_config() {
        let config = EvalConfig::from_yaml_str(VALID).unwrap();
        assert_eq!(config.targets.len(), 2);
        assert_eq!(config.scorers.len(), 5);
        assert_eq!(config.run.concurrency, 8);
        assert_eq!(config.run.budget_usd, Some(5.0));
        match config.targets.get("api") {
            Some(TargetConfig::OpenaiCompatible {
                max_retries,
                timeout_seconds,
                cost,
                system,
                params,
                ..
            }) => {
                assert_eq!(*max_retries, 5);
                assert_eq!(*timeout_seconds, 90);
                assert_eq!(cost.unwrap().input_per_1m, 0.15);
                assert_eq!(system.as_deref(), Some("You are a terse support agent."));
                let params = params.as_ref().unwrap();
                assert_eq!(params["temperature"], serde_json::json!(0));
                assert_eq!(params["max_tokens"], serde_json::json!(256));
            }
            other => panic!("expected openai-compatible target, got {other:?}"),
        }
        match config.targets.get("echo") {
            Some(TargetConfig::Shell { .. }) => {}
            other => panic!("expected shell target, got {other:?}"),
        }
        assert!(matches!(
            config.targets.get("echo"),
            Some(TargetConfig::Shell { cmd }) if cmd == "cat"
        ));
        match &config.scorers[0] {
            ScorerConfig::Contains {
                value,
                case_sensitive,
            } => {
                assert_eq!(value, "refund");
                assert!(*case_sensitive, "case_sensitive should default to true");
            }
            other => panic!("expected contains scorer, got {other:?}"),
        }
        match &config.scorers[4] {
            ScorerConfig::Judge {
                rubric, threshold, ..
            } => {
                assert!(rubric.contains("grounded"));
                assert_eq!(*threshold, 0.5, "threshold should default to 0.5");
            }
            other => panic!("expected judge scorer, got {other:?}"),
        }
    }

    #[test]
    fn retry_default_applies_and_bad_budget_rejected() {
        let yaml = r#"
targets:
  api: { type: openai-compatible, url: "http://x/v1", model: m }
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        match config.targets.get("api") {
            Some(TargetConfig::OpenaiCompatible {
                max_retries,
                timeout_seconds,
                cost,
                ..
            }) => {
                assert_eq!(*max_retries, DEFAULT_MAX_RETRIES);
                assert_eq!(*timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
                assert!(cost.is_none());
            }
            other => panic!("got {other:?}"),
        }

        let bad = format!("{yaml}run: {{ budget_usd: 0 }}\n");
        let err = EvalConfig::from_yaml_str(&bad).unwrap_err();
        assert!(err.to_string().contains("budget_usd"), "got: {err}");
    }

    #[test]
    fn concurrency_defaults_to_four() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets:
  - file: cases.jsonl
scorers:
  - type: exact
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        assert_eq!(config.run.concurrency, 4);
    }

    #[test]
    fn rejects_reserved_param_keys() {
        for reserved in ["model", "messages", "stream"] {
            let yaml = format!(
                r#"
targets:
  api:
    type: openai-compatible
    url: "http://x/v1"
    model: m
    params:
      {reserved}: whatever
datasets: [{{ file: cases.jsonl }}]
scorers: [{{ type: exact }}]
"#
            );
            let err = EvalConfig::from_yaml_str(&yaml).unwrap_err();
            assert!(err.to_string().contains(reserved), "got: {err}");
        }
    }

    #[test]
    fn rejects_zero_timeout_naming_the_target() {
        // Both HTTP-based variants reject timeout_seconds: 0, naming the target.
        let openai = r#"
targets:
  llm:
    type: openai-compatible
    url: "http://x/v1"
    model: m
    timeout_seconds: 0
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let err = EvalConfig::from_yaml_str(openai).unwrap_err().to_string();
        assert!(
            err.contains("timeout_seconds must be at least 1"),
            "got: {err}"
        );
        assert!(
            err.contains("llm"),
            "message must name the target, got: {err}"
        );

        let http = r#"
targets:
  rag:
    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
    timeout_seconds: 0
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let err = EvalConfig::from_yaml_str(http).unwrap_err().to_string();
        assert!(
            err.contains("timeout_seconds must be at least 1"),
            "got: {err}"
        );
        assert!(
            err.contains("rag"),
            "message must name the target, got: {err}"
        );
    }

    #[test]
    fn parses_trace_target_and_trajectory_scorer() {
        let yaml = r#"
targets:
  agent: { type: trace }
datasets:
  - file: traces.jsonl
scorers:
  - type: trajectory
    rules:
      - must_call: search_kb
        with:
          query: { contains: "refund" }
      - must_not_call: issue_refund
        before: verify_identity
      - max_steps: 8
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        assert!(matches!(
            config.targets.get("agent"),
            Some(TargetConfig::Trace { .. })
        ));
        match &config.scorers[0] {
            ScorerConfig::Trajectory { rules } => {
                assert_eq!(rules.len(), 3);
                match &rules[0] {
                    TrajectoryRule::MustCall {
                        must_call, with, ..
                    } => {
                        assert_eq!(must_call, "search_kb");
                        assert_eq!(with["query"].contains.as_deref(), Some("refund"));
                    }
                    other => panic!("expected must_call, got {other:?}"),
                }
                assert!(matches!(
                    &rules[1],
                    TrajectoryRule::MustNotCall { must_not_call, before: Some(b) }
                        if must_not_call == "issue_refund" && b == "verify_identity"
                ));
                assert!(matches!(
                    &rules[2],
                    TrajectoryRule::MaxSteps { max_steps: 8 }
                ));
            }
            other => panic!("expected trajectory scorer, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_scorer_type() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets:
  - file: cases.jsonl
scorers:
  - type: telepathy
"#;
        let err = EvalConfig::from_yaml_str(yaml).unwrap_err();
        assert!(matches!(err, ConfigError::Yaml(_)), "got: {err}");
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets:
  - file: cases.jsonl
scorers:
  - type: exact
judges: []
"#;
        assert!(EvalConfig::from_yaml_str(yaml).is_err());
    }

    #[test]
    fn rejects_empty_sections_and_zero_concurrency() {
        let no_targets = r#"
targets: {}
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let err = EvalConfig::from_yaml_str(no_targets).unwrap_err();
        assert!(
            err.to_string().contains("at least one target"),
            "got: {err}"
        );

        let zero_concurrency = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
run: { concurrency: 0 }
"#;
        let err = EvalConfig::from_yaml_str(zero_concurrency).unwrap_err();
        assert!(err.to_string().contains("concurrency"), "got: {err}");
    }

    #[test]
    fn parses_full_http_target() {
        let yaml = r#"
targets:
  my-rag:
    type: http
    url: https://api.myapp.com/chat
    method: post
    headers:
      x-tenant: acme
    api_key_env: MYAPP_API_KEY
    auth_header: authorization
    auth_prefix: "Bearer "
    max_retries: 3
    timeout_seconds: 30
    body:
      question: "{{input}}"
      session: eval
    response_path: /answer
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        match config.targets.get("my-rag") {
            Some(TargetConfig::Http {
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
            }) => {
                assert_eq!(url, "https://api.myapp.com/chat");
                assert_eq!(
                    method, "post",
                    "method stored verbatim; normalized in factory"
                );
                assert_eq!(headers.as_ref().unwrap()["x-tenant"], "acme");
                assert_eq!(api_key_env.as_deref(), Some("MYAPP_API_KEY"));
                assert_eq!(auth_header.as_deref(), Some("authorization"));
                assert_eq!(auth_prefix.as_deref(), Some("Bearer "));
                assert_eq!(*max_retries, 3);
                assert_eq!(*timeout_seconds, 30);
                let body = body.as_ref().unwrap();
                assert_eq!(body["question"], serde_json::json!("{{input}}"));
                assert_eq!(body["session"], serde_json::json!("eval"));
                assert_eq!(response_path.as_deref(), Some("/answer"));
            }
            other => panic!("expected http target, got {other:?}"),
        }
    }

    #[test]
    fn http_minimal_defaults_method_to_post() {
        let yaml = r#"
targets:
  api:
    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        match config.targets.get("api") {
            Some(TargetConfig::Http {
                method,
                headers,
                api_key_env,
                auth_header,
                auth_prefix,
                max_retries,
                timeout_seconds,
                body,
                response_path,
                ..
            }) => {
                assert_eq!(method, "POST", "method defaults to POST");
                assert!(headers.is_none());
                assert!(api_key_env.is_none());
                assert!(auth_header.is_none());
                assert!(auth_prefix.is_none());
                assert_eq!(*max_retries, DEFAULT_MAX_RETRIES);
                assert_eq!(*timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
                assert!(body.is_none());
                assert!(response_path.is_none());
            }
            other => panic!("expected http target, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_http_configs() {
        // (target block, fragment the error must mention)
        let cases = [
            (
                r#"    type: http
    url: "ftp://api.myapp.com/chat?q={{input}}""#,
                "http://",
            ),
            (
                r#"    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
    method: DELETE"#,
                "GET, POST, PUT, PATCH",
            ),
            (
                r#"    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
    method: get
    body:
      question: "{{input}}""#,
                "GET request may not carry a body",
            ),
            (
                r#"    type: http
    url: "https://api.myapp.com/chat"
    body:
      question: fixed"#,
                "{{input}}",
            ),
            (
                r#"    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
    auth_header: x-api-key"#,
                "require api_key_env",
            ),
            (
                r#"    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
    auth_prefix: "Token ""#,
                "require api_key_env",
            ),
            (
                r#"    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
    response_path: answer"#,
                "JSON Pointer",
            ),
            (
                // A static header collides (case-insensitively) with the
                // header the API key is sent in.
                r#"    type: http
    url: "https://api.myapp.com/chat?q={{input}}"
    api_key_env: MYAPP_API_KEY
    headers:
      Authorization: "Bearer nope""#,
                "collides with the auth header",
            ),
        ];
        for (block, fragment) in cases {
            let yaml = format!(
                "targets:\n  t:\n{block}\ndatasets: [{{ file: cases.jsonl }}]\nscorers: [{{ type: exact }}]\n"
            );
            let err = EvalConfig::from_yaml_str(&yaml).unwrap_err().to_string();
            assert!(
                err.contains(fragment),
                "block {block:?} should be rejected mentioning {fragment:?}, got: {err}"
            );
        }
    }
}
