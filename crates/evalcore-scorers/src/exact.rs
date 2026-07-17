//! Exact-match scorer: compares against an inline `value`, or the case's
//! `expected` field when no value is configured.

use async_trait::async_trait;
use evalcore_core::{Score, Scorer, TargetOutput, TestCase};

use crate::snippet;

pub struct ExactScorer {
    value: Option<String>,
}

impl ExactScorer {
    pub fn new(value: Option<String>) -> Self {
        Self { value }
    }

    fn expected_for(&self, case: &TestCase) -> Option<String> {
        if let Some(value) = &self.value {
            return Some(value.clone());
        }
        match case.expected.as_ref()? {
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }
}

#[async_trait]
impl Scorer for ExactScorer {
    fn name(&self) -> String {
        "exact".into()
    }

    async fn score(&self, case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        let Some(expected) = self.expected_for(case) else {
            return Ok(Score {
                scorer: self.name(),
                value: 0.0,
                passed: false,
                reason: Some(format!(
                    "case {:?} has no `expected` field and the scorer has no inline `value`",
                    case.id
                )),
            });
        };

        let passed = output.text == expected;
        Ok(Score {
            scorer: self.name(),
            value: if passed { 1.0 } else { 0.0 },
            passed,
            reason: (!passed)
                .then(|| format!("expected {:?}, got {:?}", expected, snippet(&output.text))),
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
            tokens: None,
        }
    }

    fn case_with_expected(expected: Option<serde_json::Value>) -> TestCase {
        TestCase {
            id: "t".into(),
            input: String::new(),
            expected,
            trace: None,
        }
    }

    #[tokio::test]
    async fn inline_value_wins() {
        let scorer = ExactScorer::new(Some("yes".into()));
        assert!(
            scorer
                .score(&case_with_expected(None), &output("yes"))
                .await
                .unwrap()
                .passed
        );
    }

    #[tokio::test]
    async fn falls_back_to_case_expected() {
        let scorer = ExactScorer::new(None);
        let case = case_with_expected(Some(serde_json::json!("HELLO")));
        assert!(scorer.score(&case, &output("HELLO")).await.unwrap().passed);
        assert!(!scorer.score(&case, &output("hello")).await.unwrap().passed);
    }

    #[tokio::test]
    async fn fails_with_reason_when_nothing_to_compare() {
        let scorer = ExactScorer::new(None);
        let score = scorer
            .score(&case_with_expected(None), &output("anything"))
            .await
            .unwrap();
        assert!(!score.passed);
        assert!(score.reason.unwrap().contains("expected"));
    }
}
