//! Suite-level aggregate gates: absolute floors on a whole run (pass rate,
//! mean score), the enterprise CI acceptance criteria that sit alongside the
//! per-case and baseline contracts.
//!
//! `evaluate_gates` is a pure, deterministic function over a `RunSummary`:
//! identical summaries yield identical results, in config order. Wiring
//! (populating `RunSummary.gates`, folding the outcome into the exit code)
//! lives in the CLI; nothing here reads the clock or the environment.

use serde::{Deserialize, Serialize};

use evalcore_config::GateConfig;

use crate::types::RunSummary;

/// Absolute tolerance applied to every gate's floor comparison, so a run that
/// exactly meets its floor is not failed by floating-point rounding (summing
/// then dividing rarely lands on the mathematically exact value).
const GATE_TOLERANCE: f64 = 1e-9;

/// The outcome of evaluating one gate against a run. `gate` is a human label
/// like `pass_rate >= 0.95` or `mean_score(judge) >= 0.8`; `actual` is the
/// measured value compared against the floor; `reason` is set only when the
/// gate could not be measured (no cases, or no matching scores).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub gate: String,
    pub actual: f64,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Evaluate every gate against the run, in config order. Deterministic.
pub fn evaluate_gates(gates: &[GateConfig], summary: &RunSummary) -> Vec<GateResult> {
    gates
        .iter()
        .map(|gate| evaluate_gate(gate, summary))
        .collect()
}

fn evaluate_gate(gate: &GateConfig, summary: &RunSummary) -> GateResult {
    match gate {
        GateConfig::PassRate { min } => {
            let label = format!("pass_rate >= {min}");
            let total = summary.total();
            // Target-error cases count in the denominator — failures are data.
            if total == 0 {
                return GateResult {
                    gate: label,
                    actual: 0.0,
                    passed: false,
                    reason: Some("no cases".into()),
                };
            }
            let actual = summary.passed() as f64 / total as f64;
            GateResult {
                gate: label,
                actual,
                // Compare with an absolute tolerance so a run that exactly
                // meets its floor isn't sunk by float rounding (e.g. three
                // 0.95 scores average to 0.9499999999999998).
                passed: actual >= *min - GATE_TOLERANCE,
                reason: None,
            }
        }
        GateConfig::MeanScore { scorer, min } => {
            let label = match scorer {
                Some(name) => format!("mean_score({name}) >= {min}"),
                None => format!("mean_score >= {min}"),
            };
            // Cases whose target errored carry no scores, so they never enter
            // the mean (pair with a pass_rate gate to catch that).
            let values: Vec<f64> = summary
                .results
                .iter()
                .flat_map(|result| &result.scores)
                .filter(|score| scorer.as_deref().map_or(true, |name| score.scorer == name))
                .map(|score| score.value)
                .collect();
            if values.is_empty() {
                let reason = match scorer {
                    Some(name) => format!("no scores from scorer {name:?}"),
                    None => "no scores".into(),
                };
                return GateResult {
                    gate: label,
                    actual: 0.0,
                    passed: false,
                    reason: Some(reason),
                };
            }
            let actual = values.iter().sum::<f64>() / values.len() as f64;
            GateResult {
                gate: label,
                actual,
                // Compare with an absolute tolerance so a run that exactly
                // meets its floor isn't sunk by float rounding (e.g. three
                // 0.95 scores average to 0.9499999999999998).
                passed: actual >= *min - GATE_TOLERANCE,
                reason: None,
            }
        }
        GateConfig::Accuracy { min } => {
            classification_gate(format!("accuracy >= {min}"), *min, summary, |c| c.accuracy)
        }
        GateConfig::MacroF1 { min } => {
            classification_gate(format!("macro_f1 >= {min}"), *min, summary, |c| c.macro_f1)
        }
    }
}

/// Evaluate a classification gate (`accuracy`/`macro_f1`) against the run's
/// computed [`ClassificationSummary`](crate::types::ClassificationSummary).
/// `metric` selects the measured value. A run with zero labeled cases — or no
/// classification computed at all — scores `0.0` and fails with a "no labeled
/// cases" reason, so the gate fails loudly rather than passing vacuously.
fn classification_gate(
    label: String,
    min: f64,
    summary: &RunSummary,
    metric: impl Fn(&crate::types::ClassificationSummary) -> f64,
) -> GateResult {
    match summary.classification.as_ref() {
        Some(classification) if classification.labeled_cases > 0 => {
            let actual = metric(classification);
            GateResult {
                gate: label,
                actual,
                passed: actual >= min - GATE_TOLERANCE,
                reason: None,
            }
        }
        _ => GateResult {
            gate: label,
            actual: 0.0,
            passed: false,
            reason: Some("no labeled cases".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CaseResult, Score};

    fn score(scorer: &str, value: f64, passed: bool) -> Score {
        Score {
            scorer: scorer.into(),
            value,
            passed,
            reason: None,
            tokens: None,
            cost_usd: None,
        }
    }

    fn case(case_id: &str, error: Option<&str>, scores: Vec<Score>) -> CaseResult {
        CaseResult {
            case_id: case_id.into(),
            output: None,
            error: error.map(Into::into),
            scores,
            cost_usd: None,
            context: None,
            trials: None,
        }
    }

    #[test]
    fn pass_rate_happy_failing_and_zero_cases() {
        // 3 of 4 pass → 0.75.
        let summary = RunSummary {
            results: vec![
                case("a", None, vec![score("contains", 1.0, true)]),
                case("b", None, vec![score("contains", 1.0, true)]),
                case("c", None, vec![score("contains", 1.0, true)]),
                case("d", None, vec![score("contains", 0.0, false)]),
            ],
            gates: Vec::new(),
            classification: None,
        };
        let pass = &evaluate_gates(&[GateConfig::PassRate { min: 0.7 }], &summary)[0];
        assert!(pass.passed);
        assert_eq!(pass.actual, 0.75);
        assert_eq!(pass.gate, "pass_rate >= 0.7");

        let fail = &evaluate_gates(&[GateConfig::PassRate { min: 0.9 }], &summary)[0];
        assert!(!fail.passed);

        // Zero cases → the gate fails with a reason, never divides by zero.
        let empty = RunSummary {
            results: Vec::new(),
            gates: Vec::new(),
            classification: None,
        };
        let zero = &evaluate_gates(&[GateConfig::PassRate { min: 0.0 }], &empty)[0];
        assert!(!zero.passed);
        assert_eq!(zero.reason.as_deref(), Some("no cases"));
    }

    #[test]
    fn target_error_cases_count_in_the_pass_rate_denominator() {
        // 1 pass, 1 target error → 0.5, not 1.0.
        let summary = RunSummary {
            results: vec![
                case("ok", None, vec![score("contains", 1.0, true)]),
                case("boom", Some("connection refused"), vec![]),
            ],
            gates: Vec::new(),
            classification: None,
        };
        let result = &evaluate_gates(&[GateConfig::PassRate { min: 0.6 }], &summary)[0];
        assert_eq!(result.actual, 0.5);
        assert!(!result.passed);
    }

    #[test]
    fn mean_score_all_scores_vs_filtered() {
        let summary = RunSummary {
            results: vec![
                case(
                    "a",
                    None,
                    vec![score("contains", 1.0, true), score("judge", 0.6, true)],
                ),
                case(
                    "b",
                    None,
                    vec![score("contains", 1.0, true), score("judge", 0.4, false)],
                ),
            ],
            gates: Vec::new(),
            classification: None,
        };
        // All scores: (1.0 + 0.6 + 1.0 + 0.4) / 4 = 0.75.
        let all = &evaluate_gates(
            &[GateConfig::MeanScore {
                scorer: None,
                min: 0.0,
            }],
            &summary,
        )[0];
        assert_eq!(all.actual, 0.75);
        assert_eq!(all.gate, "mean_score >= 0");

        // Filtered to judge: (0.6 + 0.4) / 2 = 0.5.
        let judge = &evaluate_gates(
            &[GateConfig::MeanScore {
                scorer: Some("judge".into()),
                min: 0.8,
            }],
            &summary,
        )[0];
        assert_eq!(judge.actual, 0.5);
        assert!(!judge.passed);
        assert_eq!(judge.gate, "mean_score(judge) >= 0.8");
    }

    #[test]
    fn mean_score_with_no_matching_scores_fails_naming_the_scorer() {
        // Every case errored, so the judge never ran: the mean has no inputs.
        let summary = RunSummary {
            results: vec![case("boom", Some("timeout"), vec![])],
            gates: Vec::new(),
            classification: None,
        };
        let result = &evaluate_gates(
            &[GateConfig::MeanScore {
                scorer: Some("judge".into()),
                min: 0.8,
            }],
            &summary,
        )[0];
        assert!(!result.passed);
        assert!(
            result
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("judge"),
            "reason names the scorer, got {:?}",
            result.reason
        );
    }

    #[test]
    fn exact_floor_passes_despite_float_rounding() {
        // Three 0.95 scores sum-then-divide to 0.9499999999999998, which is
        // < 0.95 in raw float comparison; the tolerance must let it pass.
        let summary = RunSummary {
            results: vec![
                case("a", None, vec![score("judge", 0.95, true)]),
                case("b", None, vec![score("judge", 0.95, true)]),
                case("c", None, vec![score("judge", 0.95, true)]),
            ],
            gates: Vec::new(),
            classification: None,
        };
        let result = &evaluate_gates(
            &[GateConfig::MeanScore {
                scorer: Some("judge".into()),
                min: 0.95,
            }],
            &summary,
        )[0];
        assert!(
            result.passed,
            "a run exactly meeting its floor must pass, got actual {}",
            result.actual
        );
    }

    #[test]
    fn nan_score_fails_safe() {
        // A NaN mean can never satisfy a floor: NaN >= anything is false.
        let summary = RunSummary {
            results: vec![case("a", None, vec![score("judge", f64::NAN, true)])],
            gates: Vec::new(),
            classification: None,
        };
        let result = &evaluate_gates(
            &[GateConfig::MeanScore {
                scorer: None,
                min: 0.0,
            }],
            &summary,
        )[0];
        assert!(!result.passed, "a NaN actual must fail the gate");
    }

    #[test]
    fn results_follow_config_order() {
        let summary = RunSummary {
            results: vec![case("a", None, vec![score("contains", 1.0, true)])],
            gates: Vec::new(),
            classification: None,
        };
        let results = evaluate_gates(
            &[
                GateConfig::MeanScore {
                    scorer: None,
                    min: 0.0,
                },
                GateConfig::PassRate { min: 0.5 },
            ],
            &summary,
        );
        assert!(results[0].gate.starts_with("mean_score"));
        assert!(results[1].gate.starts_with("pass_rate"));
    }

    fn summary_with_classification(
        accuracy: f64,
        macro_f1: f64,
        labeled_cases: usize,
    ) -> RunSummary {
        RunSummary {
            results: vec![case("a", None, vec![score("contains", 1.0, true)])],
            gates: Vec::new(),
            classification: Some(crate::types::ClassificationSummary {
                labeled_cases,
                unlabeled_cases: 0,
                accuracy,
                macro_f1,
                per_class: Vec::new(),
            }),
        }
    }

    #[test]
    fn accuracy_and_macro_f1_gates_pass_and_fail() {
        let summary = summary_with_classification(0.92, 0.88, 24);

        let acc = &evaluate_gates(&[GateConfig::Accuracy { min: 0.9 }], &summary)[0];
        assert!(acc.passed);
        assert_eq!(acc.actual, 0.92);
        assert_eq!(acc.gate, "accuracy >= 0.9");
        assert!(acc.reason.is_none());

        let acc_fail = &evaluate_gates(&[GateConfig::Accuracy { min: 0.95 }], &summary)[0];
        assert!(!acc_fail.passed);

        let f1 = &evaluate_gates(&[GateConfig::MacroF1 { min: 0.9 }], &summary)[0];
        assert!(!f1.passed);
        assert_eq!(f1.actual, 0.88);
        assert_eq!(f1.gate, "macro_f1 >= 0.9");
    }

    #[test]
    fn classification_gate_exact_floor_passes_despite_tolerance() {
        // A gate whose floor exactly equals the measured accuracy must pass.
        let summary = summary_with_classification(0.9, 0.9, 10);
        let acc = &evaluate_gates(&[GateConfig::Accuracy { min: 0.9 }], &summary)[0];
        assert!(acc.passed, "exact floor must pass, got {}", acc.actual);
    }

    #[test]
    fn classification_gate_zero_labeled_cases_fails_loudly() {
        // Zero labeled cases: metric 0.0 and a "no labeled cases" reason, for
        // both an empty classification summary and a missing one.
        let summary = summary_with_classification(0.0, 0.0, 0);
        for gate in [
            GateConfig::Accuracy { min: 0.0 },
            GateConfig::MacroF1 { min: 0.0 },
        ] {
            let result = &evaluate_gates(&[gate], &summary)[0];
            assert!(!result.passed, "zero labeled cases must fail even at min 0");
            assert_eq!(result.actual, 0.0);
            assert_eq!(result.reason.as_deref(), Some("no labeled cases"));
        }

        // No classification computed at all → same "no labeled cases" outcome.
        let no_classification = RunSummary {
            results: vec![case("a", None, vec![])],
            gates: Vec::new(),
            classification: None,
        };
        let result = &evaluate_gates(&[GateConfig::Accuracy { min: 0.5 }], &no_classification)[0];
        assert!(!result.passed);
        assert_eq!(result.reason.as_deref(), Some("no labeled cases"));
    }
}
