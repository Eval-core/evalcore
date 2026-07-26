//! `evals.yaml` schema, parsing, and validation.
//!
//! This crate is pure data: no network, no engine logic, no I/O beyond the
//! caller handing us file contents. Every EvalCore feature starts life here
//! as a config surface — the YAML file is the product's primary interface.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer, MapAccess, Visitor};
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
        if self.run.trials.count < 1 {
            return Err(ConfigError::Invalid(
                "run.trials count must be at least 1".into(),
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
                TargetConfig::Gemini {
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
                        for reserved in ["model", "contents", "systemInstruction"] {
                            if params.contains_key(reserved) {
                                return Err(ConfigError::Invalid(format!(
                                    "target {name:?}: params may not set {reserved:?} \
                                     (model/contents/systemInstruction are managed by EvalCore)"
                                )));
                            }
                        }
                    }
                    cost
                }
                TargetConfig::Anthropic {
                    cost,
                    params,
                    timeout_seconds,
                    max_tokens,
                    auth,
                    dialect,
                    aws,
                    api_key_env,
                    url,
                    ..
                } => {
                    validate_anthropic_auth(name, *auth, *dialect, aws, api_key_env, url)?;
                    if *timeout_seconds == 0 {
                        return Err(ConfigError::Invalid(format!(
                            "target {name:?}: timeout_seconds must be at least 1"
                        )));
                    }
                    if *max_tokens == 0 {
                        return Err(ConfigError::Invalid(format!(
                            "target {name:?}: max_tokens must be at least 1"
                        )));
                    }
                    if let Some(params) = params {
                        // `system` and `max_tokens` join the reserved set here:
                        // unlike OpenAI, the Messages API carries both as
                        // top-level body fields that EvalCore itself sets.
                        for reserved in ["model", "messages", "system", "max_tokens", "stream"] {
                            if params.contains_key(reserved) {
                                return Err(ConfigError::Invalid(format!(
                                    "target {name:?}: params may not set {reserved:?} \
                                     (model/messages/system/max_tokens are managed by EvalCore; \
                                     streaming responses are unsupported)"
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
        for scorer in &self.scorers {
            match scorer {
                ScorerConfig::JsonSchema { schema } => {
                    if schema.as_os_str().is_empty() {
                        return Err(ConfigError::Invalid(
                            "json-schema scorer: schema path must be non-empty".into(),
                        ));
                    }
                }
                ScorerConfig::Similarity { url, threshold, .. } => {
                    if url.is_empty() {
                        return Err(ConfigError::Invalid(
                            "similarity scorer: url must be non-empty".into(),
                        ));
                    }
                    if !threshold.is_finite() || *threshold < -1.0 || *threshold > 1.0 {
                        return Err(ConfigError::Invalid(format!(
                            "similarity scorer: threshold must be within [-1, 1], got {threshold}"
                        )));
                    }
                }
                ScorerConfig::Judge {
                    cost: Some(cost), ..
                } if cost.input_per_1m < 0.0 || cost.output_per_1m < 0.0 => {
                    return Err(ConfigError::Invalid(
                        "judge scorer has negative cost rates".into(),
                    ));
                }
                _ => {}
            }
        }
        for gate in &self.run.gates {
            match gate {
                GateConfig::PassRate { min } => {
                    if !min.is_finite() || *min < 0.0 || *min > 1.0 {
                        return Err(ConfigError::Invalid(format!(
                            "run.gates: pass_rate min must be within [0, 1], got {min}"
                        )));
                    }
                }
                GateConfig::MeanScore { scorer, min } => {
                    if !min.is_finite() {
                        return Err(ConfigError::Invalid(format!(
                            "run.gates: mean_score min must be finite, got {min}"
                        )));
                    }
                    // A scorer name that no configured scorer produces is a
                    // typo — fail fast rather than silently gating on nothing.
                    if let Some(scorer) = scorer {
                        if !self.scorers.iter().any(|s| s.type_name() == scorer) {
                            return Err(ConfigError::Invalid(format!(
                                "run.gates: mean_score scorer {scorer:?} is not among the \
                                 configured scorers"
                            )));
                        }
                    }
                }
                GateConfig::Accuracy { min } => {
                    if !min.is_finite() || *min < 0.0 || *min > 1.0 {
                        return Err(ConfigError::Invalid(format!(
                            "run.gates: accuracy min must be within [0, 1], got {min}"
                        )));
                    }
                }
                GateConfig::MacroF1 { min } => {
                    if !min.is_finite() || *min < 0.0 || *min > 1.0 {
                        return Err(ConfigError::Invalid(format!(
                            "run.gates: macro_f1 min must be within [0, 1], got {min}"
                        )));
                    }
                }
            }
        }
        if let Some(matrix) = &self.run.matrix {
            validate_matrix_names(matrix, &self.targets)
                .map_err(|msg| ConfigError::Invalid(format!("run.{msg}")))?;
        }
        Ok(())
    }
}

/// Structural checks for an `anthropic` target's auth mode. The two modes take
/// disjoint credential config, so a target carrying both — or neither — is a
/// mistake worth catching before any case runs.
fn validate_anthropic_auth(
    name: &str,
    auth: AnthropicAuth,
    dialect: AnthropicDialectConfig,
    aws: &Option<SigV4Config>,
    api_key_env: &Option<String>,
    url: &str,
) -> Result<(), ConfigError> {
    let invalid = |msg: String| Err(ConfigError::Invalid(msg));
    match auth {
        AnthropicAuth::ApiKey => {
            if aws.is_some() {
                return invalid(format!(
                    "target {name:?}: `aws` is only valid with `auth: sigv4`"
                ));
            }
            // `bedrock-invoke` exists only on AWS bedrock-runtime, which has no
            // API-key mode — accepting the pair would produce a 403 at run time.
            if dialect == AnthropicDialectConfig::BedrockInvoke {
                return invalid(format!(
                    "target {name:?}: `dialect: bedrock-invoke` requires `auth: sigv4` \
                     (legacy bedrock-runtime accepts no API key)"
                ));
            }
        }
        AnthropicAuth::Sigv4 => {
            let Some(aws) = aws else {
                return invalid(format!(
                    "target {name:?}: `auth: sigv4` requires an `aws` block with a `region`"
                ));
            };
            if aws.region.trim().is_empty() {
                return invalid(format!("target {name:?}: `aws.region` must be non-empty"));
            }
            if api_key_env.is_some() {
                return invalid(format!(
                    "target {name:?}: `api_key_env` is not used with `auth: sigv4` \
                     (credentials come from the `aws` block); remove one of them"
                ));
            }
            // The default URL is Anthropic's own API, which does not accept a
            // SigV4 signature. Signing a request to it would fail at run time
            // with an opaque 401; say so now instead.
            if url == DEFAULT_ANTHROPIC_URL {
                return invalid(format!(
                    "target {name:?}: `auth: sigv4` needs an explicit AWS `url`, e.g. \
                     https://bedrock-runtime.{}.amazonaws.com",
                    aws.region
                ));
            }
        }
    }
    Ok(())
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
    /// Suite-level aggregate gates — absolute floors on the whole run, checked
    /// after every case runs. They are additive to the per-case and baseline
    /// contracts: a run exits non-zero if any case fails (or, with
    /// `--baseline`, regresses) *or* any gate falls below its floor. Empty by
    /// default. Evaluated in list order. JUnit output is unchanged in v1 — the
    /// exit code carries the gate outcome for CI.
    #[serde(default)]
    pub gates: Vec<GateConfig>,
    /// Repeated executions per case for statistical evals. Accepts an integer
    /// shorthand (`trials: 3`, meaning `require: all`) or the full
    /// `{ count, require }` map. Absent: one trial with `require: all`, which
    /// is byte-identical to a run with no trials configured.
    #[serde(default = "default_trials", deserialize_with = "deserialize_trials")]
    pub trials: TrialsConfig,
    /// Opt in to classification aggregates: treat each labeled case's `expected`
    /// as the true class and its output as the predicted class, then compute
    /// accuracy and macro-F1 over the labeled cases. Off by default; also turned
    /// on implicitly by an `accuracy`/`macro_f1` gate, which needs the metrics.
    #[serde(default)]
    pub classification: bool,
    /// Matrix mode: run the whole suite once per named target, in list order,
    /// producing a side-by-side comparison. At least two distinct names, each
    /// defined in `targets`. Absent (the default): a single-target run, exactly
    /// as before. `--matrix` on the CLI overrides this; combining it with
    /// `--target`, `--baseline`, or `--save-baseline` is an error (baselines are
    /// per-run; `run.budget_usd` applies per arm). Omitted from serialization
    /// when absent so single-target configs round-trip unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<Vec<String>>,
    /// Append one run-history row per executed run (matrix: one per arm) to the
    /// local store, so `evalcore serve` can list past runs. On by default;
    /// `--no-history` on the CLI overrides to false. History is metadata only —
    /// its I/O never affects the eval verdict, the exit code, or the report
    /// bytes; a recording failure is a stderr warning, nothing more.
    #[serde(default = "default_true")]
    pub history: bool,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            concurrency: default_concurrency(),
            budget_usd: None,
            gates: Vec::new(),
            trials: default_trials(),
            classification: false,
            matrix: None,
            history: true,
        }
    }
}

/// Validate a matrix target list: at least two names, all distinct, and each
/// defined in `targets`. Returns an error message — naming the available
/// targets when a name is unknown — suitable for either a [`ConfigError`] (the
/// `run.matrix` form) or a CLI error (the `--matrix` form), so both report the
/// same thing. Pure: does not touch I/O or the environment.
pub fn validate_matrix_names(
    names: &[String],
    targets: &BTreeMap<String, TargetConfig>,
) -> Result<(), String> {
    if names.len() < 2 {
        return Err(format!(
            "matrix must list at least two targets, got {}",
            names.len()
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for name in names {
        if !seen.insert(name.as_str()) {
            return Err(format!("matrix lists target {name:?} more than once"));
        }
    }
    for name in names {
        if !targets.contains_key(name) {
            let available = targets.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(format!(
                "matrix target {name:?} is not defined; available: {available}"
            ));
        }
    }
    Ok(())
}

/// How many times each case runs and how per-trial verdicts fold into the
/// case verdict. `count` is at least 1; `count: 1` with `require: all` is the
/// default and behaves exactly like a run with no trials.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrialsConfig {
    /// Number of trials executed per case (indices `0..count`); at least 1.
    pub count: u32,
    /// Policy folding per-trial pass/fail into the case verdict.
    pub require: TrialRequire,
}

/// Fold policy from per-trial verdicts to the case verdict. A trial passes iff
/// every scorer passes for that trial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialRequire {
    /// The case passes only if every trial passes.
    #[default]
    All,
    /// The case passes if strictly more than half the trials pass.
    Majority,
    /// The case passes if at least one trial passes.
    Any,
}

fn default_trials() -> TrialsConfig {
    TrialsConfig {
        count: 1,
        require: TrialRequire::All,
    }
}

/// Accept either the integer shorthand (`trials: 3`) or the full
/// `{ count, require }` map, mirroring the `context` deserializer in
/// `evalcore-core`. A bare integer sets `count` with `require: all`.
fn deserialize_trials<'de, D>(deserializer: D) -> Result<TrialsConfig, D::Error>
where
    D: Deserializer<'de>,
{
    /// The full-form shape; `deny_unknown_fields` rejects typo'd keys.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TrialsForm {
        count: u32,
        #[serde(default)]
        require: TrialRequire,
    }

    struct TrialsVisitor;

    impl<'de> Visitor<'de> for TrialsVisitor {
        type Value = TrialsConfig;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a positive integer or a { count, require } map")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let count = u32::try_from(value)
                .map_err(|_| E::custom(format!("trials count {value} is too large")))?;
            Ok(TrialsConfig {
                count,
                require: TrialRequire::All,
            })
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let count = u32::try_from(value).map_err(|_| {
                E::custom(format!("trials count {value} must be a positive integer"))
            })?;
            Ok(TrialsConfig {
                count,
                require: TrialRequire::All,
            })
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let form = TrialsForm::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(TrialsConfig {
                count: form.count,
                require: form.require,
            })
        }
    }

    deserializer.deserialize_any(TrialsVisitor)
}

/// One suite-level aggregate gate. Gates express CI acceptance criteria over
/// the whole run rather than per case, e.g. "at least 95% of cases pass" or
/// "the judge's mean score is at least 0.8". Floors compare with a `1e-9`
/// absolute tolerance, so a run that exactly meets its floor is not failed by
/// floating-point rounding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GateConfig {
    /// Fraction of cases passing every scorer must be at least `min`
    /// (`min` in `[0, 1]`). Target-error cases count in the denominator —
    /// failures are data — so an error storm sinks this gate.
    PassRate { min: f64 },
    /// Mean of scorer `Score.value` must be at least `min` (`min` any finite
    /// `f64`; subprocess scorers may use arbitrary scales). With `scorer` set,
    /// only that scorer type's scores are averaged; omitted, all scores are.
    ///
    /// Cases whose target errored produce no scores, so they contribute
    /// nothing to the mean — pair a `mean_score` gate with a `pass_rate` gate
    /// to catch error storms that would otherwise leave a high mean intact.
    MeanScore {
        /// Scorer type to restrict the mean to (a config `type:` tag, e.g.
        /// `judge`, `contains`). Omitted: average across all scorers.
        #[serde(default)]
        scorer: Option<String>,
        min: f64,
    },
    /// Fraction of labeled cases predicted correctly must be at least `min`
    /// (`min` in `[0, 1]`). Reads the run's classification aggregates, so it
    /// turns them on implicitly. A run with zero labeled cases scores `0.0` and
    /// fails with a "no labeled cases" reason rather than passing vacuously.
    Accuracy { min: f64 },
    /// Macro-averaged F1 over the observed (expected) label set must be at least
    /// `min` (`min` in `[0, 1]`). Like `accuracy`, reads the classification
    /// aggregates and fails loudly on zero labeled cases.
    MacroF1 { min: f64 },
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

/// Default base URL for `anthropic` targets.
pub const DEFAULT_ANTHROPIC_URL: &str = "https://api.anthropic.com/v1";

fn default_anthropic_url() -> String {
    DEFAULT_ANTHROPIC_URL.into()
}

/// Default base URL for `gemini` targets.
pub const DEFAULT_GEMINI_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

fn default_gemini_url() -> String {
    DEFAULT_GEMINI_URL.into()
}

/// How an `anthropic` target proves who it is. Orthogonal to the wire format:
/// the request body is the same either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnthropicAuth {
    /// Send `api_key_env`'s value as `x-api-key`. Works against
    /// api.anthropic.com and any Messages-shaped gateway that accepts it.
    #[default]
    ApiKey,
    /// Sign with AWS Signature Version 4. Pairs with either dialect: the
    /// Messages-shaped AWS endpoints (Bedrock Mantle, Claude Platform on AWS)
    /// take `dialect: messages`, while legacy `bedrock-runtime` takes
    /// `dialect: bedrock-invoke`.
    Sigv4,
}

/// Which deployment of the Messages format an endpoint speaks. Separate from
/// [`AnthropicAuth`]: the two axes combine freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnthropicDialectConfig {
    /// `POST {url}/messages` with `model` in the body. api.anthropic.com, any
    /// Messages-shaped gateway, Bedrock Mantle, and Claude Platform on AWS.
    #[default]
    Messages,
    /// `POST {url}/model/{model}/invoke` with `anthropic_version` in the body
    /// and the model in the path. Legacy AWS `bedrock-runtime` only.
    BedrockInvoke,
}

/// AWS credentials and scope for `auth: sigv4`, as **names of environment
/// variables** — the same secret model as `api_key_env`. EvalCore deliberately
/// does not implement the AWS credential chain (profile files, IMDS, assumed
/// roles): resolving those reads the network and the clock at target-build
/// time. Export the variables yourself, or have your CI do it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigV4Config {
    /// AWS region the signature is scoped to, e.g. `us-east-1`. Required.
    pub region: String,
    /// Environment variable holding the access key id.
    #[serde(default = "default_access_key_env")]
    pub access_key_env: String,
    /// Environment variable holding the secret access key.
    #[serde(default = "default_secret_key_env")]
    pub secret_key_env: String,
    /// Environment variable holding the session token, for temporary
    /// credentials. Absent or unset means long-lived credentials.
    #[serde(default = "default_session_token_env")]
    pub session_token_env: String,
    /// AWS service the signature is scoped to.
    #[serde(default = "default_sigv4_service")]
    pub service: String,
}

fn default_access_key_env() -> String {
    "AWS_ACCESS_KEY_ID".into()
}

fn default_secret_key_env() -> String {
    "AWS_SECRET_ACCESS_KEY".into()
}

fn default_session_token_env() -> String {
    "AWS_SESSION_TOKEN".into()
}

fn default_sigv4_service() -> String {
    "bedrock".into()
}

/// Default `max_tokens` for `anthropic` targets and Anthropic-backed judges.
/// The Messages API requires the field, so EvalCore must always send one.
pub const DEFAULT_MAX_TOKENS: u64 = 4096;

fn default_max_tokens() -> u64 {
    DEFAULT_MAX_TOKENS
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
    /// POST to `{url}/messages` in the Anthropic Messages wire format. A second
    /// wire format, not a second SDK: the differences from `openai-compatible`
    /// are the auth headers, a top-level `system` field, a required
    /// `max_tokens`, typed content blocks in the response, and
    /// `input_tokens`/`output_tokens` usage.
    Anthropic {
        /// Base URL; `/messages` is appended. Override for gateways, proxies,
        /// or the bearer-token AWS endpoints (Claude Platform on AWS, Bedrock
        /// Mantle), which speak this same format.
        #[serde(default = "default_anthropic_url")]
        url: String,
        model: String,
        /// Name of the environment variable holding the API key, sent as
        /// `x-api-key`. Secrets are never written into the YAML itself.
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
        /// Maximum tokens to sample. First-class rather than a `params`
        /// passthrough because the Messages API rejects a request without it.
        /// Must be at least 1.
        #[serde(default = "default_max_tokens")]
        max_tokens: u64,
        /// Token prices; enables per-case cost reporting and `run.budget_usd`.
        #[serde(default)]
        cost: Option<CostConfig>,
        /// System prompt, sent as the top-level `system` body field (the
        /// Messages API has no `system` message role).
        #[serde(default)]
        system: Option<String>,
        /// Extra request-body fields passed through verbatim (temperature,
        /// top_p, thinking, …). `model`, `messages`, `system`, `max_tokens`,
        /// and `stream` are reserved and rejected at validation.
        #[serde(default)]
        params: Option<serde_json::Map<String, serde_json::Value>>,
        /// How requests authenticate. A genuinely orthogonal axis: the same
        /// Messages body is accepted by api.anthropic.com with an API key and
        /// by Bedrock Mantle with a SigV4 signature.
        #[serde(default)]
        auth: AnthropicAuth,
        /// Which deployment of the Messages format the endpoint speaks.
        /// Independent of `auth` — SigV4 pairs with either.
        #[serde(default)]
        dialect: AnthropicDialectConfig,
        /// AWS settings. Required when `auth: sigv4`, rejected otherwise.
        #[serde(default)]
        aws: Option<SigV4Config>,
    },
    /// POST to `{url}/models/{model}:generateContent` in the Gemini wire
    /// format. Google also ships an OpenAI-compatible endpoint — prefer
    /// `openai-compatible` unless you need a native-only feature.
    Gemini {
        /// Base URL; `/models/{model}:generateContent` is appended.
        #[serde(default = "default_gemini_url")]
        url: String,
        model: String,
        /// Name of the environment variable holding the API key, sent as
        /// `x-goog-api-key`. Secrets are never written into the YAML itself,
        /// and never into the URL — a `?key=` query parameter would leak the
        /// key into error messages, reports, and the cache identity.
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
        /// System prompt, sent as the top-level `systemInstruction` field.
        #[serde(default)]
        system: Option<String>,
        /// Extra request-body fields passed through verbatim. Generation
        /// tunables live under `generationConfig` in this API, so they nest:
        /// `params: {generationConfig: {temperature: 0}}`. `contents`,
        /// `systemInstruction`, and `model` are reserved.
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
    /// LLM-as-judge: grade the output against a rubric using an
    /// OpenAI-compatible or Anthropic Messages endpoint. Judge calls go through
    /// the record/replay cache, so replayed verdicts are deterministic.
    Judge {
        /// Base URL, e.g. `https://api.openai.com/v1`.
        url: String,
        model: String,
        /// Wire format the judge endpoint speaks. Defaults to `openai`, which
        /// keeps every existing suite (and its recorded verdicts) unchanged.
        #[serde(default)]
        provider: JudgeProvider,
        /// What the judge should assess, e.g. "Is the answer grounded in the
        /// provided context?".
        rubric: String,
        #[serde(default)]
        api_key_env: Option<String>,
        /// Minimum score (0.0..=1.0) to pass.
        #[serde(default = "default_judge_threshold")]
        threshold: f64,
        /// Token prices for the judge endpoint. When set, the judge call's
        /// usage is costed and attributed to per-case cost, run totals, and
        /// the run budget — exactly like target `cost`. Omit to leave judge
        /// tokens uncosted (they still count toward token totals).
        #[serde(default)]
        cost: Option<CostConfig>,
    },
    /// Validate the output against a JSON Schema (draft 2020-12). Passes iff
    /// the output parses as JSON and validates; non-JSON output is a failing
    /// score, not an error. The schema file is read and compiled in the
    /// factory.
    JsonSchema {
        /// Path to the JSON Schema file, relative to the config file. Must be
        /// non-empty; existence and compilation are checked in the factory.
        schema: PathBuf,
    },
    /// Embedding cosine-similarity scorer: embeds the case's `expected` and the
    /// output via an OpenAI-compatible `/embeddings` endpoint and passes iff
    /// their cosine similarity is at least `threshold`. Embedding calls go
    /// through the record/replay cache, so replayed scores are deterministic.
    Similarity {
        /// Base URL of the OpenAI-compatible embeddings API, e.g.
        /// `https://api.openai.com/v1`. Must be non-empty.
        url: String,
        /// Embedding model name, e.g. `text-embedding-3-small`.
        model: String,
        /// Name of the environment variable holding the API key. Secrets are
        /// never written into the YAML itself.
        #[serde(default)]
        api_key_env: Option<String>,
        /// Minimum cosine similarity to pass; a finite value in `[-1, 1]`.
        #[serde(default = "default_similarity_threshold")]
        threshold: f64,
    },
}

impl ScorerConfig {
    /// The config `type:` tag for this scorer (e.g. `"contains"`, `"judge"`).
    /// This is the name that appears in `Score.scorer` and that a
    /// `mean_score` gate's `scorer` field references.
    pub fn type_name(&self) -> &'static str {
        match self {
            ScorerConfig::Contains { .. } => "contains",
            ScorerConfig::Regex { .. } => "regex",
            ScorerConfig::Exact { .. } => "exact",
            ScorerConfig::Subprocess { .. } => "subprocess",
            ScorerConfig::Trajectory { .. } => "trajectory",
            ScorerConfig::Judge { .. } => "judge",
            ScorerConfig::JsonSchema { .. } => "json-schema",
            ScorerConfig::Similarity { .. } => "similarity",
        }
    }
}

/// Wire format a `judge` scorer's endpoint speaks. `Openai` is the default so
/// configs written before this field parse — and cache-key — identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JudgeProvider {
    /// `POST {url}/chat/completions` in the OpenAI wire format.
    #[default]
    Openai,
    /// `POST {url}/messages` in the Anthropic Messages wire format.
    Anthropic,
    /// `POST {url}/models/{model}:generateContent` in the Gemini wire format.
    Gemini,
}

fn default_true() -> bool {
    true
}

fn default_judge_threshold() -> f64 {
    0.5
}

/// Default cosine-similarity pass threshold for the `similarity` scorer.
fn default_similarity_threshold() -> f64 {
    0.8
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
    fn parses_anthropic_target_with_defaults_and_overrides() {
        let yaml = r#"
targets:
  bare: { type: anthropic, model: claude-opus-5 }
  tuned:
    type: anthropic
    url: https://gateway.internal/v1
    model: claude-opus-5
    api_key_env: ANTHROPIC_API_KEY
    max_tokens: 1024
    system: "You are a support agent."
    cost:
      input_per_1m: 5.0
      output_per_1m: 25.0
    params:
      temperature: 0
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        match config.targets.get("bare") {
            Some(TargetConfig::Anthropic {
                url,
                max_tokens,
                max_retries,
                timeout_seconds,
                system,
                params,
                cost,
                ..
            }) => {
                assert_eq!(url, DEFAULT_ANTHROPIC_URL);
                assert_eq!(*max_tokens, DEFAULT_MAX_TOKENS);
                assert_eq!(*max_retries, DEFAULT_MAX_RETRIES);
                assert_eq!(*timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
                assert!(system.is_none() && params.is_none() && cost.is_none());
            }
            other => panic!("expected anthropic target, got {other:?}"),
        }
        match config.targets.get("tuned") {
            Some(TargetConfig::Anthropic {
                url,
                max_tokens,
                api_key_env,
                system,
                params,
                cost,
                ..
            }) => {
                assert_eq!(url, "https://gateway.internal/v1");
                assert_eq!(*max_tokens, 1024);
                assert_eq!(api_key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
                assert_eq!(system.as_deref(), Some("You are a support agent."));
                assert_eq!(
                    params.as_ref().unwrap()["temperature"],
                    serde_json::json!(0)
                );
                assert_eq!(cost.unwrap().output_per_1m, 25.0);
            }
            other => panic!("expected anthropic target, got {other:?}"),
        }
    }

    #[test]
    fn rejects_anthropic_targets_that_can_never_produce_a_request() {
        let with = |field: &str| {
            format!(
                "targets:\n  claude: {{ type: anthropic, model: m, {field} }}\n\
                 datasets: [{{ file: cases.jsonl }}]\nscorers: [{{ type: exact }}]\n"
            )
        };
        // max_tokens is required by the Messages API, so 0 can never be sent.
        let err = EvalConfig::from_yaml_str(&with("max_tokens: 0")).unwrap_err();
        assert!(err.to_string().contains("max_tokens must be at least 1"));
        let err = EvalConfig::from_yaml_str(&with("timeout_seconds: 0")).unwrap_err();
        assert!(err
            .to_string()
            .contains("timeout_seconds must be at least 1"));
        // `system` and `max_tokens` are top-level Messages fields EvalCore
        // owns; a params passthrough would silently fight the builder.
        for reserved in ["system", "max_tokens", "messages", "model", "stream"] {
            let yaml = with(&format!("params: {{ {reserved}: 1 }}"));
            let err = EvalConfig::from_yaml_str(&yaml)
                .err()
                .unwrap_or_else(|| panic!("params.{reserved} must be rejected"));
            assert!(
                err.to_string()
                    .contains(&format!("params may not set {reserved:?}")),
                "got: {err}"
            );
        }
    }

    #[test]
    fn parses_gemini_target_with_defaults_and_rejects_reserved_params() {
        let yaml = r#"
targets:
  bare: { type: gemini, model: gemini-2.5-pro }
  tuned:
    type: gemini
    url: https://gateway.internal/v1beta
    model: gemini-2.5-pro
    api_key_env: GEMINI_API_KEY
    system: "Be terse."
    params:
      generationConfig:
        temperature: 0
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        match config.targets.get("bare") {
            Some(TargetConfig::Gemini { url, system, .. }) => {
                assert_eq!(url, DEFAULT_GEMINI_URL);
                assert!(system.is_none());
            }
            other => panic!("expected gemini target, got {other:?}"),
        }
        match config.targets.get("tuned") {
            Some(TargetConfig::Gemini { params, .. }) => {
                // Generation tunables nest under generationConfig in this API;
                // EvalCore passes the structure through rather than inventing
                // a translation layer.
                assert_eq!(
                    params.as_ref().unwrap()["generationConfig"]["temperature"],
                    serde_json::json!(0)
                );
            }
            other => panic!("expected gemini target, got {other:?}"),
        }

        for reserved in ["model", "contents", "systemInstruction"] {
            let yaml = format!(
                "targets:\n  g: {{ type: gemini, model: m, params: {{ {reserved}: 1 }} }}\n\
                 datasets: [{{ file: cases.jsonl }}]\nscorers: [{{ type: exact }}]\n"
            );
            let err = EvalConfig::from_yaml_str(&yaml)
                .err()
                .unwrap_or_else(|| panic!("params.{reserved} must be rejected"));
            assert!(
                err.to_string()
                    .contains(&format!("params may not set {reserved:?}")),
                "got: {err}"
            );
        }
    }

    #[test]
    fn anthropic_auth_and_dialect_are_independent_axes() {
        let target = |fields: &str| {
            format!(
                "targets:\n  t:\n    type: anthropic\n    model: m\n{fields}\
                 datasets: [{{ file: cases.jsonl }}]\nscorers: [{{ type: exact }}]\n"
            )
        };
        // SigV4 + the plain Messages dialect is a real, supported combination:
        // it is how the Messages-shaped AWS endpoints are reached.
        let yaml = target(
            "    url: https://bedrock-mantle.us-east-1.api.aws/anthropic\n\
             \x20   auth: sigv4\n    aws:\n      region: us-east-1\n",
        );
        let config = EvalConfig::from_yaml_str(&yaml).unwrap();
        match config.targets.get("t") {
            Some(TargetConfig::Anthropic {
                auth, dialect, aws, ..
            }) => {
                assert_eq!(*auth, AnthropicAuth::Sigv4);
                assert_eq!(*dialect, AnthropicDialectConfig::Messages);
                let aws = aws.as_ref().unwrap();
                assert_eq!(aws.region, "us-east-1");
                // Credentials are env-var NAMES, never values.
                assert_eq!(aws.access_key_env, "AWS_ACCESS_KEY_ID");
                assert_eq!(aws.service, "bedrock");
            }
            other => panic!("expected anthropic target, got {other:?}"),
        }

        // Every way of configuring AWS auth incoherently is rejected up front.
        let cases = [
            ("    auth: sigv4\n", "requires an `aws` block"),
            (
                "    url: https://bedrock-runtime.us-east-1.amazonaws.com\n\
                 \x20   auth: sigv4\n    aws:\n      region: \"\"\n",
                "`aws.region` must be non-empty",
            ),
            (
                "    url: https://bedrock-runtime.us-east-1.amazonaws.com\n\
                 \x20   auth: sigv4\n    api_key_env: K\n    aws:\n      region: us-east-1\n",
                "`api_key_env` is not used with `auth: sigv4`",
            ),
            (
                "    auth: sigv4\n    aws:\n      region: us-east-1\n",
                "needs an explicit AWS `url`",
            ),
            (
                "    aws:\n      region: us-east-1\n",
                "`aws` is only valid with `auth: sigv4`",
            ),
            ("    dialect: bedrock-invoke\n", "requires `auth: sigv4`"),
        ];
        for (fields, expected) in cases {
            let err = EvalConfig::from_yaml_str(&target(fields))
                .err()
                .unwrap_or_else(|| panic!("must be rejected: {fields}"));
            assert!(
                err.to_string().contains(expected),
                "expected {expected:?}, got: {err}"
            );
        }
    }

    #[test]
    fn judge_provider_defaults_to_openai_and_accepts_every_wire_format() {
        let with_provider = |line: &str| {
            format!(
                "targets:\n  echo: {{ type: shell, cmd: \"cat\" }}\n\
                 datasets: [{{ file: cases.jsonl }}]\n\
                 scorers:\n  - type: judge\n    url: \"http://x/v1\"\n    model: m\n\
                 \x20   rubric: r\n{line}"
            )
        };
        // Omitting `provider` must keep pre-existing suites on the OpenAI wire
        // format — otherwise every recorded judge verdict would re-record.
        let config = EvalConfig::from_yaml_str(&with_provider("")).unwrap();
        match &config.scorers[0] {
            ScorerConfig::Judge { provider, .. } => {
                assert_eq!(*provider, JudgeProvider::Openai);
            }
            other => panic!("expected judge scorer, got {other:?}"),
        }
        for (name, expected) in [
            ("anthropic", JudgeProvider::Anthropic),
            ("gemini", JudgeProvider::Gemini),
            ("openai", JudgeProvider::Openai),
        ] {
            let yaml = with_provider(&format!("    provider: {name}\n"));
            let config = EvalConfig::from_yaml_str(&yaml).unwrap();
            match &config.scorers[0] {
                ScorerConfig::Judge { provider, .. } => assert_eq!(*provider, expected),
                other => panic!("expected judge scorer, got {other:?}"),
            }
        }
        let err = EvalConfig::from_yaml_str(&with_provider("    provider: mistral\n")).unwrap_err();
        assert!(err.to_string().contains("mistral"), "got: {err}");
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
    fn gates_default_to_empty() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        assert!(config.run.gates.is_empty());
    }

    #[test]
    fn parses_both_gate_types() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: contains
    value: refund
  - type: judge
    url: https://api.openai.com/v1
    model: judge-model
    rubric: "grounded?"
run:
  gates:
    - type: pass_rate
      min: 0.95
    - type: mean_score
      min: 0.5
    - type: mean_score
      scorer: judge
      min: 0.8
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        assert_eq!(config.run.gates.len(), 3);
        assert!(matches!(
            config.run.gates[0],
            GateConfig::PassRate { min } if min == 0.95
        ));
        assert!(matches!(
            &config.run.gates[1],
            GateConfig::MeanScore { scorer: None, min } if *min == 0.5
        ));
        assert!(matches!(
            &config.run.gates[2],
            GateConfig::MeanScore { scorer: Some(s), min } if s == "judge" && *min == 0.8
        ));
    }

    #[test]
    fn rejects_invalid_gates() {
        // (run.gates block, fragment the error must mention)
        let cases = [
            (
                r#"    - type: pass_rate
      min: 1.5"#,
                "pass_rate min must be within [0, 1]",
            ),
            (
                r#"    - type: pass_rate
      min: -0.1"#,
                "pass_rate min must be within [0, 1]",
            ),
            (
                r#"    - type: mean_score
      min: .nan"#,
                "mean_score min must be finite",
            ),
            (
                r#"    - type: mean_score
      scorer: telepathy
      min: 0.8"#,
                "not among the configured scorers",
            ),
        ];
        for (block, fragment) in cases {
            let yaml = format!(
                "targets:\n  echo: {{ type: shell, cmd: \"cat\" }}\n\
                 datasets: [{{ file: cases.jsonl }}]\n\
                 scorers: [{{ type: exact }}]\n\
                 run:\n  gates:\n{block}\n"
            );
            let err = EvalConfig::from_yaml_str(&yaml).unwrap_err().to_string();
            assert!(
                err.contains(fragment),
                "block {block:?} should be rejected mentioning {fragment:?}, got: {err}"
            );
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

    /// A minimal valid config with an optional trailing `run:` block appended.
    fn config_with_run(run: &str) -> String {
        format!(
            "targets:\n  echo: {{ type: shell, cmd: \"cat\" }}\n\
             datasets: [{{ file: cases.jsonl }}]\n\
             scorers: [{{ type: exact }}]\n{run}"
        )
    }

    #[test]
    fn trials_defaults_to_one_all_when_absent() {
        let config = EvalConfig::from_yaml_str(&config_with_run("")).unwrap();
        assert_eq!(config.run.trials.count, 1);
        assert_eq!(config.run.trials.require, TrialRequire::All);
    }

    #[test]
    fn trials_int_shorthand_parses() {
        let config = EvalConfig::from_yaml_str(&config_with_run("run:\n  trials: 3\n")).unwrap();
        assert_eq!(config.run.trials.count, 3);
        assert_eq!(
            config.run.trials.require,
            TrialRequire::All,
            "int shorthand implies require: all"
        );
    }

    #[test]
    fn trials_full_form_parses() {
        let config = EvalConfig::from_yaml_str(&config_with_run(
            "run:\n  trials:\n    count: 5\n    require: majority\n",
        ))
        .unwrap();
        assert_eq!(config.run.trials.count, 5);
        assert_eq!(config.run.trials.require, TrialRequire::Majority);

        // require defaults to all when the map omits it.
        let config =
            EvalConfig::from_yaml_str(&config_with_run("run:\n  trials:\n    count: 2\n")).unwrap();
        assert_eq!(config.run.trials.count, 2);
        assert_eq!(config.run.trials.require, TrialRequire::All);
    }

    #[test]
    fn rejects_zero_trials() {
        let err = EvalConfig::from_yaml_str(&config_with_run("run:\n  trials: 0\n"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("run.trials"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_trials_require() {
        let err = EvalConfig::from_yaml_str(&config_with_run(
            "run:\n  trials:\n    count: 5\n    require: most\n",
        ))
        .unwrap_err();
        assert!(matches!(err, ConfigError::Yaml(_)), "got: {err}");
    }

    #[test]
    fn default_trials_round_trips_through_serialization() {
        // trials: 1 (the default) must survive a serialize -> parse round-trip
        // unchanged. RunConfig serializes concurrency unconditionally, so the
        // full trials form is emitted; re-parsing must recover count 1 / all.
        let config = EvalConfig::from_yaml_str(&config_with_run("run:\n  trials: 1\n")).unwrap();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let reparsed = EvalConfig::from_yaml_str(&yaml).unwrap();
        assert_eq!(reparsed.run.trials.count, 1);
        assert_eq!(reparsed.run.trials.require, TrialRequire::All);
        assert_eq!(reparsed.run.trials, default_trials());
    }

    #[test]
    fn parses_json_schema_scorer() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: json-schema
    schema: schemas/reply.json
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        match &config.scorers[0] {
            ScorerConfig::JsonSchema { schema } => {
                assert_eq!(schema, &PathBuf::from("schemas/reply.json"));
            }
            other => panic!("expected json-schema scorer, got {other:?}"),
        }
        assert_eq!(config.scorers[0].type_name(), "json-schema");
    }

    #[test]
    fn parses_similarity_scorer_with_and_without_optionals() {
        let full = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: similarity
    url: https://api.openai.com/v1
    model: text-embedding-3-small
    api_key_env: OPENAI_API_KEY
    threshold: 0.9
"#;
        let config = EvalConfig::from_yaml_str(full).unwrap();
        match &config.scorers[0] {
            ScorerConfig::Similarity {
                url,
                model,
                api_key_env,
                threshold,
            } => {
                assert_eq!(url, "https://api.openai.com/v1");
                assert_eq!(model, "text-embedding-3-small");
                assert_eq!(api_key_env.as_deref(), Some("OPENAI_API_KEY"));
                assert_eq!(*threshold, 0.9);
            }
            other => panic!("expected similarity scorer, got {other:?}"),
        }
        assert_eq!(config.scorers[0].type_name(), "similarity");

        let minimal = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: similarity
    url: https://api.openai.com/v1
    model: text-embedding-3-small
"#;
        let config = EvalConfig::from_yaml_str(minimal).unwrap();
        match &config.scorers[0] {
            ScorerConfig::Similarity {
                api_key_env,
                threshold,
                ..
            } => {
                assert!(api_key_env.is_none());
                assert_eq!(*threshold, 0.8, "threshold defaults to 0.8");
            }
            other => panic!("expected similarity scorer, got {other:?}"),
        }
    }

    #[test]
    fn parses_judge_scorer_with_and_without_cost() {
        let priced = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: judge-model
    rubric: "grounded?"
    cost:
      input_per_1m: 0.5
      output_per_1m: 1.5
"#;
        let config = EvalConfig::from_yaml_str(priced).unwrap();
        match &config.scorers[0] {
            ScorerConfig::Judge { cost, .. } => {
                let cost = cost.as_ref().expect("judge cost block parses");
                assert_eq!(cost.input_per_1m, 0.5);
                assert_eq!(cost.output_per_1m, 1.5);
            }
            other => panic!("expected judge scorer, got {other:?}"),
        }

        // Cost is optional: a judge without it parses to None (byte-compatible
        // with judge configs written before the cost block existed).
        let bare = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: judge-model
    rubric: "grounded?"
"#;
        let config = EvalConfig::from_yaml_str(bare).unwrap();
        match &config.scorers[0] {
            ScorerConfig::Judge { cost, .. } => assert!(cost.is_none()),
            other => panic!("expected judge scorer, got {other:?}"),
        }
    }

    #[test]
    fn rejects_negative_judge_cost() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: judge
    url: https://api.openai.com/v1
    model: judge-model
    rubric: "grounded?"
    cost:
      input_per_1m: -0.5
      output_per_1m: 1.0
"#;
        let err = EvalConfig::from_yaml_str(yaml).unwrap_err().to_string();
        assert!(
            err.contains("judge scorer has negative cost rates"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_similarity_threshold_out_of_range() {
        let yaml = r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers:
  - type: similarity
    url: https://api.openai.com/v1
    model: text-embedding-3-small
    threshold: 2.0
"#;
        let err = EvalConfig::from_yaml_str(yaml).unwrap_err().to_string();
        assert!(
            err.contains("threshold must be within [-1, 1]"),
            "got: {err}"
        );
    }

    #[test]
    fn classification_defaults_off_and_parses_when_set() {
        // Absent: off, byte-identically to a run with no classification block.
        let config = EvalConfig::from_yaml_str(&config_with_run("")).unwrap();
        assert!(!config.run.classification);

        let config =
            EvalConfig::from_yaml_str(&config_with_run("run:\n  classification: true\n")).unwrap();
        assert!(config.run.classification);
    }

    #[test]
    fn history_defaults_on_and_parses_when_disabled() {
        // Absent: on, so `evalcore serve` sees runs without extra config.
        let config = EvalConfig::from_yaml_str(&config_with_run("")).unwrap();
        assert!(config.run.history, "history defaults to true");

        let config =
            EvalConfig::from_yaml_str(&config_with_run("run:\n  history: false\n")).unwrap();
        assert!(!config.run.history, "history: false disables recording");
    }

    #[test]
    fn parses_accuracy_and_macro_f1_gates() {
        let config = EvalConfig::from_yaml_str(&config_with_run(
            "run:\n  gates:\n    - type: accuracy\n      min: 0.9\n\
             \x20   - type: macro_f1\n      min: 0.8\n",
        ))
        .unwrap();
        assert_eq!(config.run.gates.len(), 2);
        assert!(matches!(
            config.run.gates[0],
            GateConfig::Accuracy { min } if min == 0.9
        ));
        assert!(matches!(
            config.run.gates[1],
            GateConfig::MacroF1 { min } if min == 0.8
        ));
    }

    #[test]
    fn parses_matrix_and_defaults_to_none() {
        // Absent: single-target mode, byte-identical to a run with no matrix.
        let config = EvalConfig::from_yaml_str(&config_with_run("")).unwrap();
        assert!(config.run.matrix.is_none());

        let yaml = r#"
targets:
  gpt: { type: shell, cmd: "cat" }
  claude: { type: shell, cmd: "cat" }
datasets: [{ file: cases.jsonl }]
scorers: [{ type: exact }]
run:
  matrix: [gpt, claude]
"#;
        let config = EvalConfig::from_yaml_str(yaml).unwrap();
        assert_eq!(
            config.run.matrix.as_deref(),
            Some(["gpt".to_string(), "claude".to_string()].as_slice())
        );
    }

    #[test]
    fn rejects_invalid_matrix() {
        // (matrix list, fragment the error must mention)
        let cases = [
            ("[gpt]", "at least two targets"),
            ("[gpt, gpt]", "more than once"),
            ("[gpt, mystery]", "not defined; available: claude, gpt"),
        ];
        for (list, fragment) in cases {
            let yaml = format!(
                r#"
targets:
  gpt: {{ type: shell, cmd: "cat" }}
  claude: {{ type: shell, cmd: "cat" }}
datasets: [{{ file: cases.jsonl }}]
scorers: [{{ type: exact }}]
run:
  matrix: {list}
"#
            );
            let err = EvalConfig::from_yaml_str(&yaml).unwrap_err().to_string();
            assert!(
                err.contains(fragment),
                "matrix {list:?} should be rejected mentioning {fragment:?}, got: {err}"
            );
        }
    }

    #[test]
    fn rejects_classification_gates_out_of_range() {
        // (run.gates block, fragment the error must mention)
        let cases = [
            (
                "run:\n  gates:\n    - type: accuracy\n      min: 1.5\n",
                "accuracy min must be within [0, 1]",
            ),
            (
                "run:\n  gates:\n    - type: accuracy\n      min: -0.1\n",
                "accuracy min must be within [0, 1]",
            ),
            (
                "run:\n  gates:\n    - type: macro_f1\n      min: 2.0\n",
                "macro_f1 min must be within [0, 1]",
            ),
            (
                "run:\n  gates:\n    - type: macro_f1\n      min: .nan\n",
                "macro_f1 min must be within [0, 1]",
            ),
        ];
        for (run, fragment) in cases {
            let err = EvalConfig::from_yaml_str(&config_with_run(run))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(fragment),
                "run {run:?} should be rejected mentioning {fragment:?}, got: {err}"
            );
        }
    }
}
