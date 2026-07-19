//! Core domain types shared across the workspace.

use serde::{Deserialize, Serialize};

use crate::gates::GateResult;
use crate::trace::Trajectory;

/// One test case from a dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub id: String,
    /// The prompt/input sent to the target. Empty for trace cases.
    #[serde(default)]
    pub input: String,
    /// Optional expectation, interpreted per-scorer (e.g. `exact` compares
    /// against it when no inline value is configured).
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
    /// For `trace` targets: path to the recorded trace file, resolved by the
    /// dataset loader relative to the dataset file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<std::path::PathBuf>,
    /// Retrieved context chunks for RAG evaluation. Scorers see it (the judge
    /// grades against it, subprocess scorers receive it); targets never do — a
    /// RAG app does its own retrieval, and cache keys hash only identity +
    /// `input`, so context never reaches the target/cache path. The dataset
    /// loader accepts a single string or an array of strings and normalizes an
    /// empty array to `None`. Omitted from serialization when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<String>>,
}

/// Token counts reported by the provider for one call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input + self.output
    }
}

/// USD prices per 1M tokens (mirrors the config's `cost` block).
#[derive(Debug, Clone, Copy)]
pub struct CostRates {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

impl CostRates {
    pub fn cost_of(&self, tokens: TokenUsage) -> f64 {
        (tokens.input as f64 * self.input_per_1m + tokens.output as f64 * self.output_per_1m)
            / 1_000_000.0
    }
}

/// What a target produced for one case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetOutput {
    pub text: String,
    pub latency_ms: u64,
    /// Provider-reported usage, when available. Cached recordings replay the
    /// recorded usage, so cost accounting stays consistent offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    /// Structured agent trajectory, when the target produced one (`trace`
    /// targets always do). Lets the `trajectory` scorer assert on the steps
    /// while judge/text scorers grade `text` (the final answer) — the answer
    /// and the path scored on the same case. Absent (`None`) for every other
    /// target. `None` is omitted on serialization so pre-existing LLM
    /// cassettes — where trajectory is always `None`, since trace targets are
    /// never cached — keep their exact bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trajectory: Option<Trajectory>,
}

/// One scorer's verdict on one output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Score {
    /// Scorer name, e.g. `contains`.
    pub scorer: String,
    /// 0.0..=1.0; deterministic scorers emit exactly 0.0 or 1.0.
    pub value: f64,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Provider-reported tokens the scorer itself consumed, when it calls an
    /// LLM (only the `judge` scorer does today). `None` for deterministic
    /// scorers; omitted on serialization so their `Score` bytes are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    /// USD cost of this scorer's own LLM call, when the scorer was configured
    /// with pricing. `None` when unpriced or deterministic; omitted on
    /// serialization so pre-existing `Score` bytes are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// One trial's outcome within a multi-trial case (`run.trials` count > 1).
/// Carried in `CaseResult.trials`; a single-trial case has no `TrialResult`s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialResult {
    /// True iff every scorer passed for this trial. A trial whose target
    /// errored is a failed trial (`passed: false`, empty `scores`) — failures
    /// are data.
    pub passed: bool,
    /// This trial's per-scorer scores, in scorer order. Empty when the target
    /// errored (scorers do not run on a target error).
    pub scores: Vec<Score>,
    /// This trial's target latency in milliseconds; `0` when the target errored.
    pub latency_ms: u64,
    /// This trial's provider-reported TARGET tokens, when available. Held
    /// per-trial so `RunSummary::total_tokens()` can sum every trial (the
    /// case-level `output.tokens` surfaces only one trial). `None` when the
    /// target reported no usage or errored; omitted on serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    /// The target error reason for this trial, if the target failed. Omitted on
    /// serialization when the trial succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Everything that happened for one case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub case_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<TargetOutput>,
    /// Set when the target itself failed; scorers do not run in that case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub scores: Vec<Score>,
    /// USD cost of this case's target call, when the target declares rates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// The case's RAG context chunks, threaded from the dataset so the HTML
    /// reporter can display them. Absent (`None`) for cases without context;
    /// omitted on serialization so pre-existing baseline rows keep their exact
    /// bytes (a shape-pinning test guards this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<String>>,
    /// Per-trial breakdown when the case ran more than one trial
    /// (`run.trials` count > 1). `None` for single-trial cases, and omitted on
    /// serialization, so single-trial `CaseResult` JSON is byte-identical to
    /// before trials existed (a shape-pinning test guards this). When present,
    /// the case-level `output.latency_ms` is the MEAN of the trial latencies
    /// and each per-scorer `Score.value` above is the MEAN of that scorer's
    /// value across trials; the individual trial latencies and scores live here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trials: Option<Vec<TrialResult>>,
}

impl CaseResult {
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.scores.iter().all(|s| s.passed)
    }

    /// Human-readable reasons this case failed (empty when it passed).
    pub fn failure_reasons(&self) -> Vec<String> {
        if let Some(err) = &self.error {
            return vec![format!("target error: {err}")];
        }
        self.scores
            .iter()
            .filter(|s| !s.passed)
            .map(|s| {
                let reason = s.reason.as_deref().unwrap_or("failed");
                format!("{}: {reason}", s.scorer)
            })
            .collect()
    }
}

/// Precision/recall/F1 for one class in a [`ClassificationSummary`]. The class
/// set is the observed *expected* labels only, so a hallucinated prediction —
/// one that matches no expected label — is a false negative for its true class
/// and never forms a class of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassMetrics {
    /// The class label (a trimmed `expected` value).
    pub label: String,
    /// `correct / predicted-as-this-label`; `0.0` when nothing was predicted as
    /// this label (0/0 guard).
    pub precision: f64,
    /// `correct / cases-with-this-label`; `0.0` when the label has no support
    /// (0/0 guard).
    pub recall: f64,
    /// Harmonic mean of precision and recall; `0.0` when both are `0.0`.
    pub f1: f64,
    /// Number of labeled cases whose true label is this class.
    pub support: usize,
}

/// Classification aggregates over a run's labeled cases (those with `expected`).
/// A case is predicted correctly when its trimmed output equals its trimmed
/// `expected`. Populated only when `run.classification` is set or an
/// `accuracy`/`macro_f1` gate is configured; otherwise `None` and omitted from
/// serialization, so a run without classification is byte-identical to before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassificationSummary {
    /// Cases carrying an `expected` label (the metric denominator).
    pub labeled_cases: usize,
    /// Cases without `expected`, excluded from every metric.
    pub unlabeled_cases: usize,
    /// Fraction of labeled cases predicted correctly; `0.0` when none are
    /// labeled.
    pub accuracy: f64,
    /// Mean per-class F1 over the expected-label set; `0.0` when none are
    /// labeled.
    pub macro_f1: f64,
    /// Per-class metrics, sorted by label (determinism).
    pub per_class: Vec<ClassMetrics>,
}

/// The result of a whole run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub results: Vec<CaseResult>,
    /// Suite-level gate outcomes, populated by the CLI before reporting.
    /// Empty (and omitted from JSON) for runs without gates, so pre-existing
    /// baseline rows — which persist `RunSummary` JSON — still deserialize
    /// (→ empty) and re-serialize byte-identically. Baseline comparison
    /// ignores this field; it compares per-case results only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gates: Vec<GateResult>,
    /// Classification aggregates, attached by the CLI when `run.classification`
    /// is set or an `accuracy`/`macro_f1` gate is present. `None` (and omitted
    /// from JSON) otherwise, so a run without classification serializes exactly
    /// as it did before the field existed (a shape-pinning test guards this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<ClassificationSummary>,
}

impl RunSummary {
    pub fn total(&self) -> usize {
        self.results.len()
    }

    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed()).count()
    }

    pub fn failed(&self) -> usize {
        self.total() - self.passed()
    }

    pub fn all_passed(&self) -> bool {
        self.failed() == 0
    }

    /// Sum of provider-reported tokens across cases; `None` when no case
    /// reported usage (e.g. shell targets).
    ///
    /// Trial-aware and scorer-aware: a case's tokens are its TARGET tokens —
    /// summed over every trial when the case ran multiple trials, else the
    /// single surfaced `output.tokens` — PLUS the tokens consumed by any
    /// token-bearing scorer (the LLM judge). For a single-trial case the
    /// scorer tokens come from its `scores`; for a multi-trial case they come
    /// from every trial's `scores`. Everything is additive and `None`
    /// contributes nothing, so a single-trial run with only deterministic
    /// scorers yields exactly the total it did before scorers reported usage.
    pub fn total_tokens(&self) -> Option<TokenUsage> {
        let mut any = false;
        let mut total = TokenUsage::default();
        let mut add = |tokens: TokenUsage| {
            any = true;
            total.input += tokens.input;
            total.output += tokens.output;
        };
        for result in &self.results {
            match &result.trials {
                // Multi-trial: target tokens per trial, plus each trial's
                // scorer tokens (the case-level mean scores carry none).
                Some(trials) if !trials.is_empty() => {
                    for trial in trials {
                        if let Some(tokens) = trial.tokens {
                            add(tokens);
                        }
                        for score in &trial.scores {
                            if let Some(tokens) = score.tokens {
                                add(tokens);
                            }
                        }
                    }
                }
                // Single-trial: the surfaced target tokens plus the case's
                // scorer tokens.
                _ => {
                    if let Some(tokens) = result.output.as_ref().and_then(|o| o.tokens) {
                        add(tokens);
                    }
                    for score in &result.scores {
                        if let Some(tokens) = score.tokens {
                            add(tokens);
                        }
                    }
                }
            }
        }
        any.then_some(total)
    }

    /// Sum of per-case costs; `None` when no case was costed.
    pub fn total_cost_usd(&self) -> Option<f64> {
        let costs: Vec<f64> = self.results.iter().filter_map(|r| r.cost_usd).collect();
        (!costs.is_empty()).then(|| costs.iter().sum())
    }
}

/// Judges one output. Implementations live in `evalcore-scorers`.
///
/// Async because some scorers do real work (LLM judges call an endpoint,
/// subprocess scorers spawn commands); deterministic checks just return
/// immediately.
///
/// Contract: deterministic given `(case, output)` — LLM judges achieve this
/// through the record/replay cache — and never panics on malformed input:
/// return `Err` (the engine converts it into a failing score) or a failing
/// `Score` with a `reason`.
#[async_trait::async_trait]
pub trait Scorer: Send + Sync {
    /// The config tag, e.g. `contains`.
    fn name(&self) -> String;

    async fn score(&self, case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_output_trajectory_is_cassette_backcompat() {
        // Old cached JSON predates the `trajectory` field: it must deserialize
        // (→ None), and a None trajectory must serialize to the SAME bytes as
        // before the field existed — otherwise every committed LLM cassette
        // would be invalidated.
        let old_shape = r#"{"text":"hi","latency_ms":7}"#;
        let output: TargetOutput = serde_json::from_str(old_shape).unwrap();
        assert!(
            output.trajectory.is_none(),
            "absent field deserializes to None"
        );

        let reserialized = serde_json::to_string(&output).unwrap();
        assert_eq!(
            reserialized, old_shape,
            "None trajectory must not add bytes to the recorded shape"
        );
    }

    #[test]
    fn case_result_context_is_baseline_backcompat() {
        // Baseline rows persist CaseResult JSON and predate the `context` and
        // `trials` fields: old rows must deserialize (→ None), and a None
        // context/trials must serialize to the SAME bytes as before the fields
        // existed.
        let old_shape = r#"{"case_id":"c","scores":[]}"#;
        let result: CaseResult = serde_json::from_str(old_shape).unwrap();
        assert!(
            result.context.is_none(),
            "absent field deserializes to None"
        );
        assert!(
            result.trials.is_none(),
            "absent trials deserializes to None"
        );

        let reserialized = serde_json::to_string(&result).unwrap();
        assert_eq!(
            reserialized, old_shape,
            "None context/trials must not add bytes to the recorded shape"
        );

        // The other direction: a present context rides along in the JSON.
        let with_context = CaseResult {
            context: Some(vec!["chunk a".into(), "chunk b".into()]),
            ..result
        };
        let json = serde_json::to_string(&with_context).unwrap();
        assert!(
            json.contains(r#""context":["chunk a","chunk b"]"#),
            "present context serializes; got: {json}"
        );
        let round_tripped: CaseResult = serde_json::from_str(&json).unwrap();
        assert_eq!(
            round_tripped.context.as_deref(),
            Some(["chunk a".to_string(), "chunk b".to_string()].as_slice())
        );
    }

    #[test]
    fn single_trial_case_result_json_is_byte_identical() {
        // A `trials: 1` run (or any run with no trials configured) must produce
        // exactly the CaseResult bytes it produced before trials existed: the
        // `trials` field is None and omitted, adding nothing to the shape.
        let result = CaseResult {
            case_id: "refund-1".into(),
            output: Some(TargetOutput {
                text: "ok".into(),
                latency_ms: 12,
                tokens: None,
                trajectory: None,
            }),
            error: None,
            scores: vec![Score {
                scorer: "contains".into(),
                value: 1.0,
                passed: true,
                reason: None,
                tokens: None,
                cost_usd: None,
            }],
            cost_usd: None,
            context: None,
            trials: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json,
            r#"{"case_id":"refund-1","output":{"text":"ok","latency_ms":12},"scores":[{"scorer":"contains","value":1.0,"passed":true}]}"#,
            "None trials must not add bytes to the single-trial shape"
        );
    }

    #[test]
    fn multi_trial_case_result_serializes_trials_detail() {
        // A present `trials` vec rides along in the JSON and round-trips.
        let result = CaseResult {
            case_id: "c".into(),
            output: None,
            error: None,
            scores: vec![],
            cost_usd: None,
            context: None,
            trials: Some(vec![
                TrialResult {
                    passed: true,
                    scores: vec![Score {
                        scorer: "contains".into(),
                        value: 1.0,
                        passed: true,
                        reason: None,
                        tokens: None,
                        cost_usd: None,
                    }],
                    latency_ms: 5,
                    error: None,
                    tokens: None,
                },
                TrialResult {
                    passed: false,
                    scores: vec![],
                    latency_ms: 0,
                    error: Some("boom".into()),
                    tokens: None,
                },
            ]),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(r#""trials":[{"passed":true"#), "got: {json}");
        assert!(json.contains(r#""error":"boom""#), "got: {json}");
        let round_tripped: CaseResult = serde_json::from_str(&json).unwrap();
        let trials = round_tripped.trials.unwrap();
        assert_eq!(trials.len(), 2);
        assert!(trials[0].error.is_none(), "successful trial omits error");
        assert_eq!(trials[1].error.as_deref(), Some("boom"));
    }

    #[test]
    fn run_summary_gates_is_baseline_backcompat() {
        // Baseline rows persist RunSummary JSON and predate the `gates` and
        // `classification` fields: old rows must deserialize (→ empty/None), and
        // absent gates/classification must serialize to the SAME bytes as before
        // the fields existed — otherwise stored baselines would silently stop
        // round-tripping.
        let old_shape = r#"{"results":[]}"#;
        let summary: RunSummary = serde_json::from_str(old_shape).unwrap();
        assert!(
            summary.gates.is_empty(),
            "absent field deserializes to empty"
        );
        assert!(
            summary.classification.is_none(),
            "absent classification deserializes to None"
        );

        let reserialized = serde_json::to_string(&summary).unwrap();
        assert_eq!(
            reserialized, old_shape,
            "empty gates and absent classification must not add bytes to the recorded shape"
        );
    }

    #[test]
    fn run_summary_classification_rides_along_when_present() {
        // A present classification summary serializes and round-trips.
        let summary = RunSummary {
            results: vec![],
            gates: Vec::new(),
            classification: Some(ClassificationSummary {
                labeled_cases: 2,
                unlabeled_cases: 1,
                accuracy: 0.5,
                macro_f1: 0.5,
                per_class: vec![ClassMetrics {
                    label: "refund".into(),
                    precision: 1.0,
                    recall: 0.5,
                    f1: 0.6666666666666666,
                    support: 2,
                }],
            }),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(
            json.contains(r#""classification":{"labeled_cases":2"#),
            "got: {json}"
        );
        let round_tripped: RunSummary = serde_json::from_str(&json).unwrap();
        let classification = round_tripped.classification.unwrap();
        assert_eq!(classification.labeled_cases, 2);
        assert_eq!(classification.per_class[0].label, "refund");
    }
}
