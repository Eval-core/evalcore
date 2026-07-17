//! Substring scorer.

use async_trait::async_trait;
use evalcore_core::{Score, Scorer, TargetOutput, TestCase};

use crate::snippet;

pub struct ContainsScorer {
    value: String,
    case_sensitive: bool,
}

impl ContainsScorer {
    pub fn new(value: String, case_sensitive: bool) -> Self {
        Self {
            value,
            case_sensitive,
        }
    }
}

#[async_trait]
impl Scorer for ContainsScorer {
    fn name(&self) -> String {
        "contains".into()
    }

    async fn score(&self, _case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        let passed = if self.case_sensitive {
            output.text.contains(&self.value)
        } else {
            output
                .text
                .to_lowercase()
                .contains(&self.value.to_lowercase())
        };
        Ok(Score {
            scorer: self.name(),
            value: if passed { 1.0 } else { 0.0 },
            passed,
            reason: (!passed).then(|| {
                format!(
                    "expected output to contain {:?}, got: {:?}",
                    self.value,
                    snippet(&output.text)
                )
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(text: &str) -> TargetOutput {
        TargetOutput {
            text: text.into(),
            latency_ms: 0,
        }
    }

    fn case() -> TestCase {
        TestCase {
            id: "t".into(),
            input: String::new(),
            expected: None,
        }
    }

    #[tokio::test]
    async fn passes_on_substring() {
        let scorer = ContainsScorer::new("refund".into(), true);
        let score = scorer
            .score(&case(), &output("your refund is on its way"))
            .await
            .unwrap();
        assert!(score.passed);
        assert_eq!(score.value, 1.0);
        assert!(score.reason.is_none());
    }

    #[tokio::test]
    async fn fails_with_expected_and_actual_in_reason() {
        let scorer = ContainsScorer::new("refund".into(), true);
        let score = scorer
            .score(&case(), &output("no help here"))
            .await
            .unwrap();
        assert!(!score.passed);
        let reason = score.reason.unwrap();
        assert!(reason.contains("refund"), "got: {reason}");
        assert!(reason.contains("no help here"), "got: {reason}");
    }

    #[tokio::test]
    async fn case_insensitive_mode_ignores_case() {
        let scorer = ContainsScorer::new("Refund".into(), false);
        assert!(
            scorer
                .score(&case(), &output("REFUND issued"))
                .await
                .unwrap()
                .passed
        );
    }
}
