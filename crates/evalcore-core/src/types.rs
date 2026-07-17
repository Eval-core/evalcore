//! Core domain types shared across the workspace.

use serde::{Deserialize, Serialize};

/// One test case from a dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub id: String,
    /// The prompt/input sent to the target.
    pub input: String,
    /// Optional expectation, interpreted per-scorer (e.g. `exact` compares
    /// against it when no inline value is configured).
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
}

/// What a target produced for one case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetOutput {
    pub text: String,
    pub latency_ms: u64,
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
