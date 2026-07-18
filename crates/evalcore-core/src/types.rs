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
    pub fn total_tokens(&self) -> Option<TokenUsage> {
        let mut any = false;
        let mut total = TokenUsage::default();
        for tokens in self
            .results
            .iter()
            .filter_map(|r| r.output.as_ref().and_then(|o| o.tokens))
        {
            any = true;
            total.input += tokens.input;
            total.output += tokens.output;
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
                    }],
                    latency_ms: 5,
                    error: None,
                },
                TrialResult {
                    passed: false,
                    scores: vec![],
                    latency_ms: 0,
                    error: Some("boom".into()),
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
        // Baseline rows persist RunSummary JSON and predate the `gates` field:
        // old rows must deserialize (→ empty vec), and an empty `gates` must
        // serialize to the SAME bytes as before the field existed — otherwise
        // stored baselines would silently stop round-tripping.
        let old_shape = r#"{"results":[]}"#;
        let summary: RunSummary = serde_json::from_str(old_shape).unwrap();
        assert!(
            summary.gates.is_empty(),
            "absent field deserializes to empty"
        );

        let reserialized = serde_json::to_string(&summary).unwrap();
        assert_eq!(
            reserialized, old_shape,
            "empty gates must not add bytes to the recorded shape"
        );
    }
}
