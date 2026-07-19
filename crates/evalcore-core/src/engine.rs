//! The run engine: executes cases against a target and applies scorers.
//!
//! Results are returned in dataset order regardless of completion order, so
//! reports and future baseline diffs are stable.

use std::sync::atomic::{AtomicU64, Ordering};

use evalcore_config::{TrialRequire, TrialsConfig};
use futures::StreamExt;

use crate::target::Target;
use crate::types::{
    CaseResult, CostRates, RunSummary, Score, Scorer, TargetOutput, TestCase, TrialResult,
};

tokio::task_local! {
    /// The trial index (0-based) of the trial currently executing, if any.
    static TRIAL: u32;
}

/// The trial index visible to the currently executing trial, or `0` when no
/// trial scope is active (single-trial and non-trial runs).
///
/// The record/replay cache reads this to salt cache keys for trials `i > 0`
/// while leaving trial 0 byte-identical to a non-trial run — so a cassette
/// recorded before trials existed replays as trial 0. Lives here (not in the
/// store) so the engine owns trial sequencing and the store merely observes it.
pub fn current_trial() -> u32 {
    TRIAL.try_with(|trial| *trial).unwrap_or(0)
}

/// Run `fut` with `trial` visible to [`current_trial`] for the duration of its
/// execution. Trial 0 keeps cache keys byte-identical to a non-trial run.
pub async fn with_trial<F>(trial: u32, fut: F) -> F::Output
where
    F: std::future::Future,
{
    TRIAL.scope(trial, fut).await
}

/// A per-case-completion callback the CLI installs to drive an interactive
/// progress display. Fired exactly once as each case finishes — in completion
/// order, not dataset order — so the callback must be cheap and internally
/// synchronized (cases run concurrently). Purely observational: it never touches
/// results, ordering, or the verdict, and `RunOptions::on_progress` is `None` by
/// default, so non-interactive runs pay nothing and stay byte-identical.
#[derive(Clone)]
pub struct ProgressSink(std::sync::Arc<dyn Fn() + Send + Sync>);

impl ProgressSink {
    /// Wrap a closure fired once per completed case.
    pub fn new(f: impl Fn() + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(f))
    }

    fn notify(&self) {
        (self.0)()
    }
}

impl std::fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProgressSink(..)")
    }
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Maximum in-flight cases (minimum 1).
    pub concurrency: usize,
    /// Stop dispatching new cases once accumulated cost reaches this (USD).
    /// Cases skipped by the budget are failed cases with a reason — the run
    /// completes and reports rather than aborting. Requires `cost_rates`.
    pub budget_usd: Option<f64>,
    /// The selected target's token prices; enables per-case `cost_usd`.
    pub cost_rates: Option<CostRates>,
    /// Trials per case: how many times each case runs and how per-trial
    /// verdicts fold into the case verdict. `count == 1` with `require: all`
    /// (the default) behaves exactly like a run with no trials configured —
    /// byte-identical results, cache keys, and reporter output.
    pub trials: TrialsConfig,
    /// Optional interactive-progress callback, fired once per completed case.
    /// `None` (the default) for non-interactive/CI runs; the engine's results,
    /// ordering, and reporter bytes are identical either way.
    pub on_progress: Option<ProgressSink>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            concurrency: 0,
            budget_usd: None,
            cost_rates: None,
            trials: TrialsConfig {
                count: 1,
                require: TrialRequire::All,
            },
            on_progress: None,
        }
    }
}

pub async fn run_suite(
    target: &dyn Target,
    cases: Vec<TestCase>,
    scorers: &[Box<dyn Scorer>],
    options: RunOptions,
) -> RunSummary {
    // Accumulated spend in micro-USD; atomic so concurrent cases stay honest.
    let spent_micros = AtomicU64::new(0);
    let options = &options;
    let results = futures::stream::iter(cases)
        .map(|case| {
            let spent_micros = &spent_micros;
            async move {
                if let Some(budget) = options.budget_usd {
                    let spent = spent_micros.load(Ordering::SeqCst) as f64 / 1e6;
                    if spent >= budget {
                        // A budget-skipped case is still a completed case.
                        if let Some(sink) = &options.on_progress {
                            sink.notify();
                        }
                        return CaseResult {
                            case_id: case.id,
                            output: None,
                            error: Some(format!(
                                "skipped: run budget of ${budget} exhausted (spent ${spent:.4})"
                            )),
                            scores: Vec::new(),
                            cost_usd: None,
                            context: case.context,
                            trials: None,
                        };
                    }
                }
                let result =
                    run_case(target, scorers, case, options.cost_rates, &options.trials).await;
                // Every trial's cost counts toward the budget; `cost_usd` is the
                // sum across trials (a single trial's cost for count == 1).
                if let Some(cost) = result.cost_usd {
                    spent_micros.fetch_add((cost * 1e6) as u64, Ordering::SeqCst);
                }
                // Observational only — after results/costs are settled, never
                // before, so a slow callback can't perturb budget accounting.
                if let Some(sink) = &options.on_progress {
                    sink.notify();
                }
                result
            }
        })
        .buffered(options.concurrency.max(1))
        .collect()
        .await;
    // Gates and classification are attached by the CLI after the run; the
    // engine leaves them empty.
    RunSummary {
        results,
        gates: Vec::new(),
        classification: None,
    }
}

/// Execute one case. Dispatches to the single-trial path (byte-identical to a
/// non-trial run) or the multi-trial path based on `trials.count`.
async fn run_case(
    target: &dyn Target,
    scorers: &[Box<dyn Scorer>],
    case: TestCase,
    cost_rates: Option<CostRates>,
    trials: &TrialsConfig,
) -> CaseResult {
    if trials.count <= 1 {
        run_single(target, scorers, case, cost_rates).await
    } else {
        run_multi(
            target,
            scorers,
            case,
            cost_rates,
            trials.count as usize,
            trials.require,
        )
        .await
    }
}

/// The single-trial path. Byte-identical (result shape, cache key, cost) to the
/// engine before trials existed: `trials` is `None` and no trial scope is set,
/// so `current_trial()` is 0 and cache keys are unchanged.
async fn run_single(
    target: &dyn Target,
    scorers: &[Box<dyn Scorer>],
    case: TestCase,
    cost_rates: Option<CostRates>,
) -> CaseResult {
    match target.invoke(&case).await {
        Ok(output) => {
            let mut scores = Vec::with_capacity(scorers.len());
            for scorer in scorers {
                scores.push(score_one(scorer.as_ref(), &case, &output).await);
            }
            // Case cost = target-call cost (when the target is priced) plus the
            // cost of any priced scorer's own LLM call (the judge). `Some` when
            // either contributed, so a judge-only cost still surfaces and feeds
            // the budget even for an unpriced target.
            let target_cost = cost_rates
                .zip(output.tokens)
                .map(|(rates, tokens)| rates.cost_of(tokens));
            let any_scorer_cost = scores.iter().any(|s| s.cost_usd.is_some());
            let scorer_cost: f64 = scores.iter().filter_map(|s| s.cost_usd).sum();
            let cost_usd = (target_cost.is_some() || any_scorer_cost)
                .then(|| target_cost.unwrap_or(0.0) + scorer_cost);
            CaseResult {
                case_id: case.id,
                output: Some(output),
                error: None,
                scores,
                cost_usd,
                context: case.context,
                trials: None,
            }
        }
        Err(err) => CaseResult {
            case_id: case.id,
            output: None,
            error: Some(format!("{err:#}")),
            scores: Vec::new(),
            cost_usd: None,
            context: case.context,
            trials: None,
        },
    }
}

/// The multi-trial path (`count > 1`). Runs `count` trials sequentially in
/// trial-index order (determinism), each under a trial scope so its target and
/// scorer cache calls re-key per trial. Folds per-trial verdicts into the case
/// verdict via `require`; the case-level `scores` are per-scorer means and the
/// case latency is the mean of the trial latencies.
async fn run_multi(
    target: &dyn Target,
    scorers: &[Box<dyn Scorer>],
    case: TestCase,
    cost_rates: Option<CostRates>,
    count: usize,
    require: TrialRequire,
) -> CaseResult {
    let mut trial_results: Vec<TrialResult> = Vec::with_capacity(count);
    let mut outputs: Vec<Option<TargetOutput>> = Vec::with_capacity(count);
    for i in 0..count {
        let (trial, output) = with_trial(i as u32, run_trial(target, scorers, &case)).await;
        trial_results.push(trial);
        outputs.push(output);
    }

    let passed_trials = trial_results.iter().filter(|t| t.passed).count();
    let verdict = match require {
        TrialRequire::All => passed_trials == count,
        TrialRequire::Any => passed_trials >= 1,
        // Majority = strictly more than half.
        TrialRequire::Majority => passed_trials * 2 > count,
    };

    // Per-scorer case score = mean of that scorer's value across the trials
    // that produced it (errored trials contribute none). `passed` is the case
    // verdict so `CaseResult::passed()` reflects the require policy; the
    // granular per-trial scores live in the trials detail.
    let mut scores = Vec::with_capacity(scorers.len());
    for (j, scorer) in scorers.iter().enumerate() {
        let values: Vec<f64> = trial_results
            .iter()
            .filter_map(|t| t.scores.get(j))
            .map(|s| s.value)
            .collect();
        if values.is_empty() {
            continue;
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        // A failing case keeps the first failing trial's reason for this scorer
        // (trial-index order, so deterministic) — the terminal report's reason
        // line stays actionable; the full per-trial detail lives in `trials`.
        let reason = (!verdict)
            .then(|| {
                trial_results.iter().enumerate().find_map(|(i, t)| {
                    let score = t.scores.get(j)?;
                    (!score.passed).then(|| match &score.reason {
                        Some(r) => format!("trial {i}: {r}"),
                        None => format!("trial {i}: failed"),
                    })
                })
            })
            .flatten();
        scores.push(Score {
            scorer: scorer.name(),
            value: mean,
            passed: verdict,
            reason,
            tokens: None,
            cost_usd: None,
        });
    }

    let latency_mean = trial_results.iter().map(|t| t.latency_ms).sum::<u64>() / count as u64;

    // Every trial's cost counts toward the budget; the case cost is their sum
    // over all trials: target-call cost (when priced) plus each trial's priced
    // scorer costs (the judge). `Some` when anything was costed.
    let target_cost: f64 = cost_rates
        .map(|rates| {
            outputs
                .iter()
                .filter_map(|o| o.as_ref().and_then(|o| o.tokens))
                .map(|tokens| rates.cost_of(tokens))
                .sum()
        })
        .unwrap_or(0.0);
    let any_target_cost = cost_rates.is_some()
        && outputs
            .iter()
            .any(|o| o.as_ref().and_then(|o| o.tokens).is_some());
    let scorer_cost: f64 = trial_results
        .iter()
        .flat_map(|t| t.scores.iter())
        .filter_map(|s| s.cost_usd)
        .sum();
    let any_scorer_cost = trial_results
        .iter()
        .any(|t| t.scores.iter().any(|s| s.cost_usd.is_some()));
    let cost_usd = (any_target_cost || any_scorer_cost).then_some(target_cost + scorer_cost);

    // Surface the first successful trial's output (with the mean latency); if
    // every trial errored, surface a case error so `passed()` stays false.
    let first_ok = outputs.into_iter().flatten().next();
    let (output, error) = match first_ok {
        Some(mut output) => {
            output.latency_ms = latency_mean;
            (Some(output), None)
        }
        None => (None, trial_results.iter().find_map(|t| t.error.clone())),
    };

    CaseResult {
        case_id: case.id,
        output,
        error,
        scores,
        cost_usd,
        context: case.context,
        trials: Some(trial_results),
    }
}

/// Run one trial: invoke the target and, on success, apply every scorer. A
/// target error is a failed trial with a reason (no scores). Returns the trial
/// record plus the raw output (for aggregation) when the target succeeded.
async fn run_trial(
    target: &dyn Target,
    scorers: &[Box<dyn Scorer>],
    case: &TestCase,
) -> (TrialResult, Option<TargetOutput>) {
    match target.invoke(case).await {
        Ok(output) => {
            let mut scores = Vec::with_capacity(scorers.len());
            for scorer in scorers {
                scores.push(score_one(scorer.as_ref(), case, &output).await);
            }
            let passed = scores.iter().all(|s| s.passed);
            let latency_ms = output.latency_ms;
            let tokens = output.tokens;
            (
                TrialResult {
                    passed,
                    scores,
                    latency_ms,
                    tokens,
                    error: None,
                },
                Some(output),
            )
        }
        Err(err) => (
            TrialResult {
                passed: false,
                scores: Vec::new(),
                latency_ms: 0,
                tokens: None,
                error: Some(format!("{err:#}")),
            },
            None,
        ),
    }
}

/// Apply one scorer, converting a scorer `Err` into a failing score with a
/// reason (one bad scorer never aborts the run).
async fn score_one(scorer: &dyn Scorer, case: &TestCase, output: &TargetOutput) -> Score {
    scorer
        .score(case, output)
        .await
        .unwrap_or_else(|err| Score {
            scorer: scorer.name(),
            value: 0.0,
            passed: false,
            reason: Some(format!("scorer error: {err}")),
            tokens: None,
            cost_usd: None,
        })
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
                tokens: None,
                cost_usd: None,
            })
        }
    }

    /// A scorer that reports fixed token usage and a fixed USD cost, standing
    /// in for the priced LLM judge in engine-level accounting tests.
    struct PricedScorer {
        tokens: crate::types::TokenUsage,
        cost_usd: f64,
    }

    #[async_trait]
    impl Scorer for PricedScorer {
        fn name(&self) -> String {
            "priced".into()
        }

        async fn score(&self, _case: &TestCase, _output: &TargetOutput) -> anyhow::Result<Score> {
            Ok(Score {
                scorer: self.name(),
                value: 1.0,
                passed: true,
                reason: None,
                tokens: Some(self.tokens),
                cost_usd: Some(self.cost_usd),
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
                context: None,
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
    async fn on_progress_fires_exactly_once_per_case() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Arc;
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let ticks = Arc::new(AtomicUsize::new(0));
        let sink = {
            let ticks = Arc::clone(&ticks);
            ProgressSink::new(move || {
                ticks.fetch_add(1, Ordering::SeqCst);
            })
        };
        let opts = RunOptions {
            concurrency: 4,
            on_progress: Some(sink),
            ..Default::default()
        };
        // One case errors: it still completes, so it still ticks.
        let summary = run_suite(&Upper, cases(&["a", "explode", "c"]), &scorers, opts).await;
        assert_eq!(summary.total(), 3);
        assert_eq!(
            ticks.load(Ordering::SeqCst),
            3,
            "every completed case, pass or error, ticks progress exactly once"
        );
    }

    #[tokio::test]
    async fn threads_case_context_into_the_result() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let cases = vec![
            TestCase {
                id: "with-ctx".into(),
                input: "a".into(),
                expected: None,
                trace: None,
                context: Some(vec!["chunk one".into(), "chunk two".into()]),
            },
            TestCase {
                id: "no-ctx".into(),
                input: "b".into(),
                expected: None,
                trace: None,
                context: None,
            },
        ];
        let summary = run_suite(&Upper, cases, &scorers, options(2)).await;

        assert_eq!(
            summary.results[0].context.as_deref(),
            Some(["chunk one".to_string(), "chunk two".to_string()].as_slice()),
            "context rides from the case onto the result"
        );
        assert_eq!(summary.results[1].context, None);
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
            ..Default::default()
        };
        let summary = run_suite(&Upper, cases(&["a", "b"]), &scorers, opts).await;

        let expected_per_case = (10.0 * 1.0 + 5.0 * 2.0) / 1e6;
        assert_eq!(summary.results[0].cost_usd, Some(expected_per_case));
        assert_eq!(summary.total_cost_usd(), Some(expected_per_case * 2.0));
        let tokens = summary.total_tokens().unwrap();
        assert_eq!((tokens.input, tokens.output), (20, 10));
    }

    #[tokio::test]
    async fn multi_trial_total_tokens_sums_every_trial() {
        // gap #2: with trials > 1, token totals must reflect ALL trials, not
        // just the one surfaced output. `Upper` reports (10,5) tokens per call.
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let summary = run_suite(
            &Upper,
            cases(&["a"]),
            &scorers,
            trials_opts(3, TrialRequire::All),
        )
        .await;
        let tokens = summary.total_tokens().unwrap();
        assert_eq!(
            (tokens.input, tokens.output),
            (30, 15),
            "three trials of (10,5) target tokens sum to (30,15)"
        );
    }

    #[tokio::test]
    async fn priced_scorer_cost_and_tokens_fold_into_totals() {
        // The target is unpriced (no cost_rates); the judge-like scorer carries
        // the cost. It must still surface per case and in the run total, and its
        // tokens must add to the token totals alongside the target tokens.
        let tokens = crate::types::TokenUsage {
            input: 4,
            output: 6,
        };
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(PricedScorer {
            tokens,
            cost_usd: 0.000_01,
        })];
        let summary = run_suite(&Upper, cases(&["a", "b"]), &scorers, options(1)).await;

        assert_eq!(
            summary.results[0].cost_usd,
            Some(0.000_01),
            "the scorer's cost is the case cost when the target is unpriced"
        );
        assert_eq!(summary.total_cost_usd(), Some(0.000_02));
        // Target (10,5) + scorer (4,6) per case, over two cases.
        let total = summary.total_tokens().unwrap();
        assert_eq!((total.input, total.output), (28, 22));
    }

    #[tokio::test]
    async fn priced_scorer_cost_counts_against_the_budget() {
        // The budget accumulator reads `CaseResult.cost_usd`; a priced scorer's
        // cost must flow in and exhaust the budget even with an unpriced target.
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(PricedScorer {
            tokens: crate::types::TokenUsage::default(),
            cost_usd: 0.000_01,
        })];
        let opts = RunOptions {
            concurrency: 1,
            budget_usd: Some(0.000_005),
            cost_rates: None,
            ..Default::default()
        };
        let summary = run_suite(&Upper, cases(&["a", "b", "c"]), &scorers, opts).await;
        assert_eq!(
            summary.passed(),
            1,
            "the first case's scorer cost exhausts the budget for the rest"
        );
        let skipped = &summary.results[1];
        assert!(skipped.error.as_deref().unwrap().contains("budget"));
        assert!(skipped.output.is_none(), "over-budget cases do not invoke");
    }

    #[tokio::test]
    async fn multi_trial_priced_scorer_cost_sums_across_trials() {
        // Each trial runs the priced scorer once; the case cost is the sum over
        // all trials (target unpriced here).
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(PricedScorer {
            tokens: crate::types::TokenUsage {
                input: 1,
                output: 1,
            },
            cost_usd: 0.000_01,
        })];
        let summary = run_suite(
            &Upper,
            cases(&["a"]),
            &scorers,
            trials_opts(3, TrialRequire::All),
        )
        .await;
        let case_cost = summary.results[0].cost_usd.unwrap();
        assert!(
            (case_cost - 0.000_03).abs() < 1e-12,
            "three trials of scorer cost sum into the case cost; got {case_cost}"
        );
        // total_tokens: target (10,5) per trial + scorer (1,1) per trial, ×3.
        let total = summary.total_tokens().unwrap();
        assert_eq!((total.input, total.output), (33, 18));
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
            ..Default::default()
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

    /// A target whose output depends on the current trial index, so trials
    /// produce different verdicts deterministically: `"yes"` for trials below
    /// `pass_below`, else `"no"`. Latency is `trial + 1` (distinct per trial).
    struct Flaky {
        pass_below: u32,
    }

    #[async_trait]
    impl Target for Flaky {
        async fn invoke(&self, _case: &TestCase) -> anyhow::Result<TargetOutput> {
            let trial = current_trial();
            Ok(TargetOutput {
                text: if trial < self.pass_below { "yes" } else { "no" }.into(),
                latency_ms: trial as u64 + 1,
                tokens: None,
                trajectory: None,
            })
        }
    }

    /// A target that errors on exactly one trial index, else answers `"yes"`.
    struct ExplodeOnTrial {
        at: u32,
    }

    #[async_trait]
    impl Target for ExplodeOnTrial {
        async fn invoke(&self, _case: &TestCase) -> anyhow::Result<TargetOutput> {
            let trial = current_trial();
            if trial == self.at {
                anyhow::bail!("boom on trial {trial}");
            }
            Ok(TargetOutput {
                text: "yes".into(),
                latency_ms: 10,
                tokens: None,
                trajectory: None,
            })
        }
    }

    struct WantYes;

    #[async_trait]
    impl Scorer for WantYes {
        fn name(&self) -> String {
            "want-yes".into()
        }

        async fn score(&self, _case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
            let passed = output.text == "yes";
            Ok(Score {
                scorer: self.name(),
                value: if passed { 1.0 } else { 0.0 },
                passed,
                reason: (!passed).then(|| "wanted yes".into()),
                tokens: None,
                cost_usd: None,
            })
        }
    }

    fn trials_opts(count: u32, require: TrialRequire) -> RunOptions {
        RunOptions {
            concurrency: 1,
            trials: TrialsConfig { count, require },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn single_trial_leaves_no_trials_detail() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(NonEmpty)];
        let summary = run_suite(
            &Upper,
            cases(&["a"]),
            &scorers,
            trials_opts(1, TrialRequire::All),
        )
        .await;
        assert!(
            summary.results[0].trials.is_none(),
            "count == 1 keeps the trials detail None"
        );
    }

    #[tokio::test]
    async fn require_all_needs_every_trial() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(WantYes)];
        // 2 of 3 trials pass → require all FAILS the case.
        let summary = run_suite(
            &Flaky { pass_below: 2 },
            cases(&["q"]),
            &scorers,
            trials_opts(3, TrialRequire::All),
        )
        .await;
        assert_eq!(summary.passed(), 0);
        let result = &summary.results[0];
        let trials = result.trials.as_ref().unwrap();
        assert_eq!(trials.len(), 3);
        assert_eq!(trials.iter().filter(|t| t.passed).count(), 2);
        // Per-scorer case score is the mean across trials: [1, 1, 0] → 2/3.
        assert!((result.scores[0].value - 2.0 / 3.0).abs() < 1e-12);

        // 3 of 3 pass → require all PASSES.
        let summary = run_suite(
            &Flaky { pass_below: 3 },
            cases(&["q"]),
            &scorers,
            trials_opts(3, TrialRequire::All),
        )
        .await;
        assert_eq!(summary.passed(), 1);
    }

    #[tokio::test]
    async fn require_majority_needs_more_than_half() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(WantYes)];
        // 2/3 → majority PASS.
        assert_eq!(
            run_suite(
                &Flaky { pass_below: 2 },
                cases(&["q"]),
                &scorers,
                trials_opts(3, TrialRequire::Majority)
            )
            .await
            .passed(),
            1
        );
        // 1/3 → majority FAIL (1*2 is not > 3).
        assert_eq!(
            run_suite(
                &Flaky { pass_below: 1 },
                cases(&["q"]),
                &scorers,
                trials_opts(3, TrialRequire::Majority)
            )
            .await
            .passed(),
            0
        );
    }

    #[tokio::test]
    async fn require_any_needs_one_trial() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(WantYes)];
        // 1/3 → any PASS.
        assert_eq!(
            run_suite(
                &Flaky { pass_below: 1 },
                cases(&["q"]),
                &scorers,
                trials_opts(3, TrialRequire::Any)
            )
            .await
            .passed(),
            1
        );
        // 0/3 → any FAIL.
        assert_eq!(
            run_suite(
                &Flaky { pass_below: 0 },
                cases(&["q"]),
                &scorers,
                trials_opts(3, TrialRequire::Any)
            )
            .await
            .passed(),
            0
        );
    }

    #[tokio::test]
    async fn case_latency_is_mean_of_trial_latencies() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(WantYes)];
        let summary = run_suite(
            &Flaky { pass_below: 3 },
            cases(&["q"]),
            &scorers,
            trials_opts(3, TrialRequire::All),
        )
        .await;
        // Trial latencies 1, 2, 3 → case latency mean 2.
        assert_eq!(
            summary.results[0].output.as_ref().unwrap().latency_ms,
            2,
            "case latency is the mean of the trial latencies"
        );
        let trials = summary.results[0].trials.as_ref().unwrap();
        assert_eq!(
            trials.iter().map(|t| t.latency_ms).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "per-trial latencies are kept in trial-index order"
        );
    }

    #[tokio::test]
    async fn trial_target_error_is_a_failed_trial_with_reason() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(WantYes)];
        // Trial 1 errors; trials 0 and 2 succeed. Under `any` the case passes.
        let summary = run_suite(
            &ExplodeOnTrial { at: 1 },
            cases(&["q"]),
            &scorers,
            trials_opts(3, TrialRequire::Any),
        )
        .await;
        assert_eq!(
            summary.passed(),
            1,
            "two good trials pass the case under any"
        );
        let result = &summary.results[0];
        let trials = result.trials.as_ref().unwrap();
        assert!(trials[0].passed);
        assert!(!trials[1].passed, "the errored trial is a failed trial");
        assert!(trials[1].scores.is_empty(), "no scores on a target error");
        assert!(trials[1]
            .error
            .as_deref()
            .unwrap()
            .contains("boom on trial 1"));
        assert!(trials[2].passed);
        // The mean uses only the two successful trials: [1, 1] → 1.0.
        assert_eq!(result.scores[0].value, 1.0);
    }

    #[tokio::test]
    async fn all_trials_erroring_yields_a_case_error() {
        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(WantYes)];
        let summary = run_suite(
            &ExplodeOnTrial { at: 0 }, // errors only trial 0
            cases(&["q"]),
            &scorers,
            trials_opts(1, TrialRequire::All), // single trial errors → case errors
        )
        .await;
        // count == 1 uses the single-trial path: today's behavior, trials None.
        let result = &summary.results[0];
        assert!(!result.passed());
        assert!(result.error.as_deref().unwrap().contains("boom on trial 0"));
        assert!(result.trials.is_none());
    }

    #[tokio::test]
    async fn per_scorer_mean_feeds_a_mean_score_gate() {
        use crate::gates::evaluate_gates;
        use evalcore_config::GateConfig;

        let scorers: Vec<Box<dyn Scorer>> = vec![Box::new(WantYes)];
        // One case, 2/3 trials pass → want-yes mean value 2/3.
        let summary = run_suite(
            &Flaky { pass_below: 2 },
            cases(&["q"]),
            &scorers,
            trials_opts(3, TrialRequire::Majority),
        )
        .await;
        let pass = &evaluate_gates(
            &[GateConfig::MeanScore {
                scorer: Some("want-yes".into()),
                min: 0.6,
            }],
            &summary,
        )[0];
        assert!(pass.passed, "mean 0.667 clears a 0.6 floor");
        assert!((pass.actual - 2.0 / 3.0).abs() < 1e-12);

        let fail = &evaluate_gates(
            &[GateConfig::MeanScore {
                scorer: Some("want-yes".into()),
                min: 0.7,
            }],
            &summary,
        )[0];
        assert!(!fail.passed, "mean 0.667 is below a 0.7 floor");
    }
}
