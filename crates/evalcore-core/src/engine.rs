//! The run engine: executes cases against a target and applies scorers.
//!
//! Results are returned in dataset order regardless of completion order, so
//! reports and future baseline diffs are stable. (Record/replay caching and
//! rate-limit awareness land here in v0.1 — see PRD §6.2/§6.3.)

use futures::StreamExt;

use crate::target::Target;
use crate::types::{CaseResult, RunSummary, Score, Scorer, TestCase};

pub async fn run_suite(
    target: &dyn Target,
    cases: Vec<TestCase>,
    scorers: &[Box<dyn Scorer>],
    concurrency: usize,
) -> RunSummary {
    let results = futures::stream::iter(cases)
        .map(|case| run_case(target, scorers, case))
        .buffered(concurrency.max(1))
        .collect()
        .await;
    RunSummary { results }
}

async fn run_case(target: &dyn Target, scorers: &[Box<dyn Scorer>], case: TestCase) -> CaseResult {
    match target.invoke(&case).await {
        Ok(output) => {
            let mut scores = Vec::with_capacity(scorers.len());
            for scorer in scorers {
                let score = scorer
                    .score(&case, &output)
                    .await
                    .unwrap_or_else(|err| Score {
                        scorer: scorer.name(),
                        value: 0.0,
                        passed: false,
                        reason: Some(format!("scorer error: {err}")),
                    });
                scores.push(score);
            }
            CaseResult {
                case_id: case.id,
                output: Some(output),
                error: None,
                scores,
            }
        }
        Err(err) => CaseResult {
            case_id: case.id,
            output: None,
            error: Some(format!("{err:#}")),
            scores: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TargetOutput;
    use async_trait::async_trait;

    struct Upper;

    #[async_trait]
    impl Target for Upper {
        async fn invoke(&self, case: &TestCase) -> anyhow::Result<TargetOutput> {
            if case.input == "explode" {
                anyhow::bail!("target blew up");
            }
            Ok(TargetOutput {
                text: case.input.to_uppercase(),
                latency_ms: 1,
            })
        }
    }

    struct NonEmpty;

    #[async_trait]
    impl Scorer for NonEmpty {
        fn name(&self) -> String {
            "non-empty".into()
        }

        async fn score(&self, _case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
            let passed = !output.text.is_empty();
            Ok(Score {
                scorer: self.name(),
                value: if passed { 1.0 } else { 0.0 },
                passed,
                reason: (!passed).then(|| "output was empty".into()),
            })
        }
    }

    fn cases(inputs: &[&str]) -> Vec<TestCase> {
        inputs
            .iter()
            .enumerate()
            .map(|(i, input)| TestCase {
                id: format!("case-{i}"),
                input: (*input).into(),
                expected: None,
            })
            .collect()
    }

    #[tokio::test]
    async fn preserves_dataset_order_and_counts() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let summary = run_suite(&Upper, cases(&["a", "b", "c"]), &scorers, 8).await;

        assert_eq!(summary.total(), 3);
        assert_eq!(summary.passed(), 3);
        assert!(summary.all_passed());
        let ids: Vec<_> = summary.results.iter().map(|r| r.case_id.as_str()).collect();
        assert_eq!(ids, ["case-0", "case-1", "case-2"]);
    }

    #[tokio::test]
    async fn target_errors_become_failed_cases_not_panics() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let summary = run_suite(&Upper, cases(&["ok", "explode"]), &scorers, 2).await;

        assert_eq!(summary.passed(), 1);
        assert_eq!(summary.failed(), 1);
        let failed = &summary.results[1];
        assert!(!failed.passed());
        assert!(failed.error.as_deref().unwrap().contains("target blew up"));
        assert!(
            failed.scores.is_empty(),
            "scorers must not run on target errors"
        );
    }
}
