//! JSON Schema scorer: passes iff the output parses as JSON and validates
//! against a compiled draft 2020-12 schema. The schema file is read and
//! compiled once, in the factory (`build_scorers`), so a bad schema fails the
//! whole run before any case executes rather than per case.
//!
//! Non-JSON output is a *failing score with a reason*, never an `Err` —
//! failures are data. Remote/`$ref` resolution is disabled (the `jsonschema`
//! dep is built with `default-features = false`), so validation never touches
//! the network and stays deterministic.

use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use evalcore_core::{Score, Scorer, TargetOutput, TestCase};

pub struct JsonSchemaScorer {
    validator: jsonschema::Validator,
}

impl JsonSchemaScorer {
    /// Read and compile the schema at `path` (already resolved against the
    /// config directory by the factory). Every failure mode — unreadable file,
    /// non-JSON schema, invalid schema, or an unresolvable remote `$ref` —
    /// names the file so config mistakes are actionable.
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read json-schema file {}", path.display()))?;
        let schema: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("json-schema file {} is not valid JSON", path.display()))?;
        // draft 2020-12, no network: with resolve-http/resolve-file compiled
        // out, an external `$ref` fails to resolve here (at construction),
        // never at score time and never over the wire.
        let validator = jsonschema::draft202012::options()
            .build(&schema)
            .map_err(|err| anyhow::anyhow!("invalid JSON Schema in {}: {err}", path.display()))?;
        Ok(Self { validator })
    }
}

#[async_trait]
impl Scorer for JsonSchemaScorer {
    fn name(&self) -> String {
        "json-schema".into()
    }

    async fn score(&self, _case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        let instance: serde_json::Value = match serde_json::from_str(&output.text) {
            Ok(value) => value,
            Err(err) => {
                return Ok(Score {
                    scorer: self.name(),
                    value: 0.0,
                    passed: false,
                    reason: Some(format!(
                        "output is not valid JSON: {err} (output: {:?})",
                        crate::snippet(&output.text)
                    )),
                });
            }
        };

        // Collect every violation, each tagged with its JSON pointer, then sort
        // for a deterministic reason regardless of the validator's iteration
        // order. Up to three are surfaced so reports stay readable.
        let mut violations: Vec<String> = self
            .validator
            .iter_errors(&instance)
            .map(|err| {
                let pointer = err.instance_path().to_string();
                let at = if pointer.is_empty() {
                    "<root>"
                } else {
                    &pointer
                };
                // Violation text embeds the offending instance value, which is
                // user output — truncate it like any other seen output.
                format!("{at}: {}", crate::snippet(&err.to_string()))
            })
            .collect();

        if violations.is_empty() {
            return Ok(Score {
                scorer: self.name(),
                value: 1.0,
                passed: true,
                reason: None,
            });
        }

        violations.sort();
        let shown = violations.len().min(3);
        let reason = violations[..shown].join("; ");
        Ok(Score {
            scorer: self.name(),
            value: 0.0,
            passed: false,
            reason: Some(reason),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn output(text: &str) -> TargetOutput {
        TargetOutput {
            text: text.into(),
            latency_ms: 0,
            tokens: None,
            trajectory: None,
        }
    }

    fn case() -> TestCase {
        TestCase {
            id: "t".into(),
            input: String::new(),
            expected: None,
            trace: None,
            context: None,
        }
    }

    fn schema_file(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f
    }

    const PERSON_SCHEMA: &str = r#"{
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer", "minimum": 0}
        },
        "required": ["name", "age"]
    }"#;

    #[tokio::test]
    async fn valid_output_passes() {
        let f = schema_file(PERSON_SCHEMA);
        let scorer = JsonSchemaScorer::new(f.path()).unwrap();
        let score = scorer
            .score(&case(), &output(r#"{"name": "Ada", "age": 36}"#))
            .await
            .unwrap();
        assert!(score.passed);
        assert_eq!(score.value, 1.0);
        assert!(score.reason.is_none());
    }

    #[tokio::test]
    async fn invalid_output_fails_with_deterministic_sorted_pointers() {
        let f = schema_file(PERSON_SCHEMA);
        let scorer = JsonSchemaScorer::new(f.path()).unwrap();
        // `age` is negative (violates minimum) and `name` is the wrong type.
        let score = scorer
            .score(&case(), &output(r#"{"name": 42, "age": -1}"#))
            .await
            .unwrap();
        assert!(!score.passed);
        assert_eq!(score.value, 0.0);
        let reason = score.reason.unwrap();
        // Deterministic: /age sorts before /name, both pointers are named.
        assert!(reason.contains("/age"), "got: {reason}");
        assert!(reason.contains("/name"), "got: {reason}");
        let age_at = reason.find("/age").unwrap();
        let name_at = reason.find("/name").unwrap();
        assert!(age_at < name_at, "sorted order is stable; got: {reason}");
        // Running it again yields the identical string.
        let again = scorer
            .score(&case(), &output(r#"{"name": 42, "age": -1}"#))
            .await
            .unwrap();
        assert_eq!(again.reason.unwrap(), reason);
    }

    #[tokio::test]
    async fn caps_reason_at_three_violations() {
        // Five properties each require a string; supplying integers for all
        // yields five distinct violations at five pointers — only three shown.
        let f = schema_file(
            r#"{"type": "object", "properties": {
                "a": {"type": "string"}, "b": {"type": "string"},
                "c": {"type": "string"}, "d": {"type": "string"},
                "e": {"type": "string"}
            }}"#,
        );
        let scorer = JsonSchemaScorer::new(f.path()).unwrap();
        let score = scorer
            .score(&case(), &output(r#"{"a":1,"b":2,"c":3,"d":4,"e":5}"#))
            .await
            .unwrap();
        assert!(!score.passed);
        let reason = score.reason.unwrap();
        assert_eq!(
            reason.matches("; ").count(),
            2,
            "at most three violations joined by two '; ': {reason}"
        );
    }

    #[tokio::test]
    async fn non_json_output_fails_gracefully() {
        let f = schema_file(PERSON_SCHEMA);
        let scorer = JsonSchemaScorer::new(f.path()).unwrap();
        let score = scorer
            .score(&case(), &output("not json at all"))
            .await
            .unwrap();
        assert!(!score.passed);
        assert_eq!(score.value, 0.0);
        assert!(
            score.reason.unwrap().contains("not valid JSON"),
            "non-JSON is a failing score, not an Err"
        );
    }

    #[test]
    fn bad_schema_file_is_a_construction_error_naming_the_file() {
        // Non-existent file.
        let missing = std::path::Path::new("/no/such/schema-file-xyz.json");
        let err = JsonSchemaScorer::new(missing).err().unwrap().to_string();
        assert!(err.contains("schema-file-xyz.json"), "got: {err}");

        // Present but not JSON.
        let f = schema_file("this is not json");
        let err = JsonSchemaScorer::new(f.path()).err().unwrap().to_string();
        assert!(err.contains("not valid JSON"), "got: {err}");
        assert!(
            err.contains(&f.path().display().to_string()),
            "error must name the file; got: {err}"
        );
    }

    #[test]
    fn remote_ref_schema_errors_at_construction_without_network() {
        // With resolve-http compiled out there is no HTTP client to attempt a
        // fetch: an external `$ref` simply fails to resolve at build time.
        let f = schema_file(r#"{"$ref": "https://example.com/does-not-exist.json"}"#);
        let err = JsonSchemaScorer::new(f.path());
        assert!(
            err.is_err(),
            "an unresolvable remote $ref must fail at construction, not attempt a fetch"
        );
    }
}
