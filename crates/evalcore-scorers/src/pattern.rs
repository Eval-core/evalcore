//! Regex scorer. The pattern is compiled once in `build_scorers`, so an
//! invalid pattern fails the run before any case executes.

use async_trait::async_trait;
use evalcore_core::{Score, Scorer, TargetOutput, TestCase};
use regex::Regex;

use crate::snippet;

pub struct RegexScorer {
    regex: Regex,
}

impl RegexScorer {
    pub fn new(pattern: &str) -> anyhow::Result<Self> {
        Ok(Self {
            regex: Regex::new(pattern)?,
        })
    }
}

#[async_trait]
impl Scorer for RegexScorer {
    fn name(&self) -> String {
        "regex".into()
    }

    async fn score(&self, _case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        let passed = self.regex.is_match(&output.text);
        Ok(Score {
            scorer: self.name(),
            value: if passed { 1.0 } else { 0.0 },
            passed,
            reason: (!passed).then(|| {
                format!(
                    "expected output to match /{}/, got: {:?}",
                    self.regex.as_str(),
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
        }
    }

    #[tokio::test]
    async fn matches_pattern() {
        let scorer = RegexScorer::new(r"^order #\d+").unwrap();
        assert!(
            scorer
                .score(&case(), &output("order #42 shipped"))
                .await
                .unwrap()
                .passed
        );
    }

    #[tokio::test]
    async fn failure_reason_shows_pattern() {
        let scorer = RegexScorer::new(r"\d{4}").unwrap();
        let score = scorer.score(&case(), &output("no digits")).await.unwrap();
        assert!(!score.passed);
        assert!(score.reason.unwrap().contains(r"\d{4}"));
    }

    #[test]
    fn invalid_pattern_is_a_construction_error() {
        assert!(RegexScorer::new("(unclosed").is_err());
    }
}
