//! The run engine: executes cases against a target and applies scorers.
//!
//! Results are returned in dataset order regardless of completion order, so
//! reports and future baseline diffs are stable.

use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt;

use crate::target::Target;
use crate::types::{CaseResult, CostRates, RunSummary, Score, Scorer, TestCase};

#[derive(Debug, Clone, Copy, Default)]
pub struct RunOptions {
    /// Maximum in-flight cases (minimum 1).
    pub concurrency: usize,
    /// Stop dispatching new cases once accumulated cost reaches this (USD).
    /// Cases skipped by the budget are failed cases with a reason — the run
    /// completes and reports rather than aborting. Requires `cost_rates`.
    pub budget_usd: Option<f64>,
    /// The selected target's token prices; enables per-case `cost_usd`.
    pub cost_rates: Option<CostRates>,
}

pub async fn run_suite(
    target: &dyn Target,
    cases: Vec<TestCase>,
    scorers: &[Box<dyn Scorer>],
    options: RunOptions,
) -> RunSummary {
    // Accumulated spend in micro-USD; atomic so concurrent cases stay honest.
    let spent_micros = AtomicU64::new(0);
    let results = futures::stream::iter(cases)
        .map(|case| {
            let spent_micros = &spent_micros;
            async move {
                if let Some(budget) = options.budget_usd {
                    let spent = spent_micros.load(Ordering::SeqCst) as f64 / 1e6;
                    if spent >= budget {
                        return CaseResult {
                            case_id: case.id,
                            output: None,
                            error: Some(format!(
                                "skipped: run budget of ${budget} exhausted (spent ${spent:.4})"
                            )),
                            scores: Vec::new(),
                            cost_usd: None,
                        };
                    }
                }
                let mut result = run_case(target, scorers, case).await;
                if let (Some(rates), Some(tokens)) = (
                    options.cost_rates,
                    result.output.as_ref().and_then(|o| o.tokens),
                ) {
                    let cost = rates.cost_of(tokens);
                    result.cost_usd = Some(cost);
                    spent_micros.fetch_add((cost * 1e6) as u64, Ordering::SeqCst);
                }
                result
            }
        })
        .buffered(options.concurrency.max(1))
        .collect()
        .await;
    // Gates are evaluated by the CLI after the run; the engine leaves them empty.
    RunSummary {
        results,
        gates: Vec::new(),
    }
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
                cost_usd: None,
            }
        }
        Err(err) => CaseResult {
            case_id: case.id,
            output: None,
            error: Some(format!("{err:#}")),
            scores: Vec::new(),
            cost_usd: None,
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
                tokens: Some(crate::types::TokenUsage {
                    input: 10,
                    output: 5,
                }),
                trajectory: None,
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
                trace: None,
            })
            .collect()
    }

    fn options(concurrency: usize) -> RunOptions {
        RunOptions {
            concurrency,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn preserves_dataset_order_and_counts() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let summary = run_suite(&Upper, cases(&["a", "b", "c"]), &scorers, options(8)).await;

        assert_eq!(summary.total(), 3);
        assert_eq!(summary.passed(), 3);
        assert!(summary.all_passed());
        let ids: Vec<_> = summary.results.iter().map(|r| r.case_id.as_str()).collect();
        assert_eq!(ids, ["case-0", "case-1", "case-2"]);
    }

    #[tokio::test]
    async fn target_errors_become_failed_cases_not_panics() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let summary = run_suite(&Upper, cases(&["ok", "explode"]), &scorers, options(2)).await;

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

    #[tokio::test]
    async fn costs_cases_and_totals_them() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let opts = RunOptions {
            concurrency: 2,
            budget_usd: None,
            // 10 input + 5 output tokens per case at $1/$2 per 1M
            cost_rates: Some(CostRates {
                input_per_1m: 1.0,
                output_per_1m: 2.0,
            }),
        };
        let summary = run_suite(&Upper, cases(&["a", "b"]), &scorers, opts).await;

        let expected_per_case = (10.0 * 1.0 + 5.0 * 2.0) / 1e6;
        assert_eq!(summary.results[0].cost_usd, Some(expected_per_case));
        assert_eq!(summary.total_cost_usd(), Some(expected_per_case * 2.0));
        let tokens = summary.total_tokens().unwrap();
        assert_eq!((tokens.input, tokens.output), (20, 10));
    }

    #[tokio::test]
    async fn budget_skips_remaining_cases_as_failures() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let opts = RunOptions {
            // Sequential so spend accumulates deterministically case by case.
            concurrency: 1,
            budget_usd: Some(0.000_02),
            cost_rates: Some(CostRates {
                input_per_1m: 1.0,
                output_per_1m: 2.0,
            }),
        };
        // Each case costs $0.00002, so case 1 runs (spent 0 < budget), and
        // every later case is over budget.
        let summary = run_suite(&Upper, cases(&["a", "b", "c"]), &scorers, opts).await;

        assert_eq!(summary.passed(), 1);
        assert_eq!(summary.failed(), 2);
        assert!(!summary.all_passed(), "budget-skipped cases fail the run");
        let skipped = &summary.results[1];
        let reason = skipped.error.as_deref().unwrap();
        assert!(reason.contains("budget"), "got: {reason}");
        assert!(skipped.output.is_none(), "skipped cases must not invoke");
    }
}
