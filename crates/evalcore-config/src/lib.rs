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
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Maximum in-flight cases.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            concurrency: default_concurrency(),
        }
    }
}

fn default_concurrency() -> usize {
    4
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
    },
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
"#;

    #[test]
    fn parses_full_config() {
        let config = EvalConfig::from_yaml_str(VALID).unwrap();
        assert_eq!(config.targets.len(), 2);
        assert_eq!(config.scorers.len(), 5);
        assert_eq!(config.run.concurrency, 8);
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
}
