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
    /// USD cost of this case's target call, when the target declares rates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
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
