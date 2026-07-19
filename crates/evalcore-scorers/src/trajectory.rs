//! Trajectory assertions over normalized agent traces: which tools ran, with
//! what arguments, in what order, within what budget.
//!
//! Operates on the canonical trajectory JSON produced by `trace` targets
//! (see `evalcore-core::trace` and docs/trajectory-spec.md). Rule semantics
//! are user-facing contract:
//!
//! - `must_call: T` — at least one call of `T` (with matching `with:`
//!   arguments if given; only counting calls after the first call of
//!   `after:` if given — if the `after` tool never runs, the rule fails).
//! - `must_not_call: T` — no call of `T` at all; with `before: U`, no call
//!   of `T` before the first call of `U` (if `U` never runs, ANY call of `T`
//!   fails — the guard it was waiting for never happened).
//! - `max_steps: N` — at most `N` tool calls in the whole trajectory.

use async_trait::async_trait;
use evalcore_config::{FieldMatcher, TrajectoryRule};
use evalcore_core::trace::{TraceStep, Trajectory};
use evalcore_core::{parse_trajectory, Score, Scorer, TargetOutput, TestCase};

pub struct TrajectoryScorer {
    rules: Vec<TrajectoryRule>,
}

impl TrajectoryScorer {
    pub fn new(rules: Vec<TrajectoryRule>) -> Self {
        Self { rules }
    }
}

#[async_trait]
impl Scorer for TrajectoryScorer {
    fn name(&self) -> String {
        "trajectory".into()
    }

    async fn score(&self, _case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        // Prefer the structured trajectory (`trace` targets attach it, so the
        // steps are graded even when `text` holds the final answer). Fall back
        // to parsing `text` for trajectory JSON arriving through any other
        // target (e.g. a shell target emitting the native format).
        let parsed;
        let trajectory = match &output.trajectory {
            Some(trajectory) => trajectory,
            None => {
                parsed = parse_trajectory(&output.text)?;
                &parsed
            }
        };
        let failures: Vec<String> = self
            .rules
            .iter()
            .filter_map(|rule| check_rule(rule, trajectory).err())
            .collect();

        let passed = failures.is_empty();
        Ok(Score {
            scorer: self.name(),
            value: if passed { 1.0 } else { 0.0 },
            passed,
            reason: (!passed).then(|| failures.join("; ")),
            tokens: None,
            cost_usd: None,
        })
    }
}

/// Ok(()) when the rule holds; Err(reason) when it doesn't.
fn check_rule(rule: &TrajectoryRule, trajectory: &Trajectory) -> Result<(), String> {
    let steps = &trajectory.steps;
    match rule {
        TrajectoryRule::MustCall {
            must_call,
            with,
            after,
        } => {
            let start = match after {
                Some(gate) => match first_index(steps, gate) {
                    Some(index) => index + 1,
                    None => {
                        return Err(format!(
                            "must_call {must_call:?} after {gate:?}: {gate:?} was never called"
                        ))
                    }
                },
                None => 0,
            };
            let mut saw_tool = false;
            for step in &steps[start.min(steps.len())..] {
                if step.tool != *must_call {
                    continue;
                }
                saw_tool = true;
                if with
                    .iter()
                    .all(|(field, matcher)| field_matches(&step.input, field, matcher))
                {
                    return Ok(());
                }
            }
            if saw_tool {
                Err(format!(
                    "must_call {must_call:?}: called, but no call matched `with` constraints"
                ))
            } else if after.is_none() {
                Err(format!("must_call {must_call:?}: never called"))
            } else {
                Err(format!(
                    "must_call {must_call:?} after {:?}: not called after it",
                    after.as_deref().unwrap_or_default()
                ))
            }
        }
        TrajectoryRule::MustNotCall {
            must_not_call,
            before,
        } => {
            let limit = match before {
                Some(gate) => first_index(steps, gate).unwrap_or(steps.len()),
                None => steps.len(),
            };
            match steps[..limit].iter().position(|s| s.tool == *must_not_call) {
                Some(index) => Err(match before {
                    Some(gate) => format!(
                        "must_not_call {must_not_call:?} before {gate:?}: called at step {}",
                        index + 1
                    ),
                    None => format!(
                        "must_not_call {must_not_call:?}: called at step {}",
                        index + 1
                    ),
                }),
                None => Ok(()),
            }
        }
        TrajectoryRule::MaxSteps { max_steps } => {
            if steps.len() > *max_steps {
                Err(format!(
                    "max_steps {max_steps}: trajectory has {} tool calls",
                    steps.len()
                ))
            } else {
                Ok(())
            }
        }
    }
}

fn first_index(steps: &[TraceStep], tool: &str) -> Option<usize> {
    steps.iter().position(|s| s.tool == tool)
}

fn field_matches(input: &serde_json::Value, field: &str, matcher: &FieldMatcher) -> bool {
    let Some(value) = input.get(field) else {
        return false;
    };
    if let Some(needle) = &matcher.contains {
        let rendered = match value.as_str() {
            Some(s) => s.to_string(),
            None => value.to_string(),
        };
        if !rendered.contains(needle.as_str()) {
            return false;
        }
    }
    if let Some(expected) = &matcher.equals {
        if value != expected {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output_of(trajectory: serde_json::Value) -> TargetOutput {
        TargetOutput {
            text: trajectory.to_string(),
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
            context: None,
        }
    }

    /// verify_identity → search_kb(query: refund policy) → issue_refund
    fn refund_flow() -> TargetOutput {
        output_of(serde_json::json!({"steps": [
            {"tool": "verify_identity", "input": {"user": "u1"}},
            {"tool": "search_kb", "input": {"query": "refund policy limits"}},
            {"tool": "issue_refund", "input": {"amount": 42}},
        ]}))
    }

    async fn run(rules: serde_json::Value, output: &TargetOutput) -> Score {
        let rules: Vec<TrajectoryRule> = serde_json::from_value(rules).unwrap();
        TrajectoryScorer::new(rules)
            .score(&case(), output)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn happy_flow_passes_all_rule_kinds() {
        let score = run(
            serde_json::json!([
                {"must_call": "search_kb", "with": {"query": {"contains": "refund"}}},
                {"must_call": "issue_refund", "after": "verify_identity"},
                {"must_not_call": "escalate_to_human"},
                {"must_not_call": "issue_refund", "before": "verify_identity"},
                {"max_steps": 3},
            ]),
            &refund_flow(),
        )
        .await;
        assert!(score.passed, "got: {:?}", score.reason);
    }

    #[tokio::test]
    async fn missing_call_and_unmatched_args_fail_with_reasons() {
        let score = run(
            serde_json::json!([
                {"must_call": "escalate_to_human"},
                {"must_call": "search_kb", "with": {"query": {"contains": "warranty"}}},
            ]),
            &refund_flow(),
        )
        .await;
        assert!(!score.passed);
        let reason = score.reason.unwrap();
        assert!(
            reason.contains("\"escalate_to_human\": never called"),
            "got: {reason}"
        );
        assert!(reason.contains("`with` constraints"), "got: {reason}");
    }

    #[tokio::test]
    async fn ordering_rules_catch_violations() {
        // issue_refund BEFORE verify_identity.
        let bad_order = output_of(serde_json::json!({"steps": [
            {"tool": "issue_refund", "input": {"amount": 42}},
            {"tool": "verify_identity", "input": {"user": "u1"}},
        ]}));
        let score = run(
            serde_json::json!([
                {"must_not_call": "issue_refund", "before": "verify_identity"},
                {"must_call": "issue_refund", "after": "verify_identity"},
            ]),
            &bad_order,
        )
        .await;
        assert!(!score.passed);
        let reason = score.reason.unwrap();
        assert!(reason.contains("called at step 1"), "got: {reason}");
        assert!(reason.contains("not called after it"), "got: {reason}");
    }

    #[tokio::test]
    async fn missing_gate_tool_fails_conservatively() {
        // `before: verify_identity` where verify_identity never runs: any
        // issue_refund call must fail the rule.
        let no_gate = output_of(serde_json::json!({"steps": [
            {"tool": "issue_refund", "input": {}},
        ]}));
        let score = run(
            serde_json::json!([
                {"must_not_call": "issue_refund", "before": "verify_identity"},
            ]),
            &no_gate,
        )
        .await;
        assert!(!score.passed, "gate never ran → call is a violation");
    }

    #[tokio::test]
    async fn max_steps_and_equals_matcher() {
        let score = run(
            serde_json::json!([
                {"max_steps": 2},
                {"must_call": "issue_refund", "with": {"amount": {"equals": 42}}},
            ]),
            &refund_flow(),
        )
        .await;
        assert!(!score.passed);
        assert!(score.reason.unwrap().contains("max_steps 2"), "3 calls > 2");

        let score = run(
            serde_json::json!([
                {"must_call": "issue_refund", "with": {"amount": {"equals": 42}}},
            ]),
            &refund_flow(),
        )
        .await;
        assert!(score.passed, "equals matcher on number");
    }

    #[tokio::test]
    async fn scores_from_structured_trajectory_ignoring_text() {
        // `text` is a plain final answer (not trajectory JSON); the scorer must
        // read the structured trajectory and never try to parse `text`.
        let output = TargetOutput {
            text: "Refunds are honored within 30 days.".into(),
            latency_ms: 0,
            tokens: None,
            trajectory: Some(Trajectory {
                steps: vec![
                    TraceStep {
                        tool: "search_kb".into(),
                        input: serde_json::json!({"query": "refund policy"}),
                        output: None,
                    },
                    TraceStep {
                        tool: "reply".into(),
                        input: serde_json::json!({}),
                        output: None,
                    },
                ],
            }),
        };
        let score = run(
            serde_json::json!([
                {"must_call": "search_kb", "with": {"query": {"contains": "refund"}}},
                {"max_steps": 2},
            ]),
            &output,
        )
        .await;
        assert!(
            score.passed,
            "structured trajectory graded; got: {:?}",
            score.reason
        );
    }

    #[tokio::test]
    async fn falls_back_to_parsing_text_when_no_structured_trajectory() {
        // trajectory: None (e.g. a shell target emitting native JSON) → the
        // scorer parses `text`, preserving today's behavior.
        let output = refund_flow(); // output_of sets trajectory: None
        assert!(output.trajectory.is_none());
        let score = run(serde_json::json!([{"must_call": "search_kb"}]), &output).await;
        assert!(score.passed, "fallback text parse; got: {:?}", score.reason);
    }

    #[tokio::test]
    async fn non_trajectory_output_is_a_scorer_error() {
        let scorer = TrajectoryScorer::new(vec![]);
        let output = TargetOutput {
            text: "just some model text".into(),
            latency_ms: 0,
            tokens: None,
            trajectory: None,
        };
        assert!(scorer.score(&case(), &output).await.is_err());
    }
}
