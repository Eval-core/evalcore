//! Matrix comparison: the pure model behind a matrix run (one suite run once
//! per target). [`compare_arms`] turns a [`MatrixSummary`] — the per-arm
//! [`RunSummary`]s, in the user's list order — into a [`MatrixComparison`] of
//! per-case rows and per-arm aggregates. Deterministic: identical arms yield
//! identical comparisons, rows stay in dataset order, and the winner is the
//! unique-max mean case score. Rendering lives in `evalcore-report`; wiring
//! (running the arms, folding the exit code) lives in the CLI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::types::RunSummary;

/// Absolute tolerance for the per-case winner comparison, so two arms whose
/// mean case scores differ only by floating-point rounding are treated as tied
/// rather than crowning a spurious winner.
const WINNER_TOLERANCE: f64 = 1e-9;

/// The result of running one suite against several targets: one arm per target,
/// in the matrix's list order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixSummary {
    pub arms: Vec<MatrixArm>,
}

/// One arm of a matrix run: a target name paired with the suite's `RunSummary`
/// against that target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixArm {
    pub target: String,
    pub summary: RunSummary,
}

/// A side-by-side comparison of a matrix run's arms: per-case rows in dataset
/// order plus per-arm aggregates, both in matrix (arm) order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixComparison {
    /// One row per case, in dataset order (the arms' shared case order).
    pub rows: Vec<ComparisonRow>,
    /// Per-arm aggregates, in matrix order.
    pub arms: Vec<ArmStats>,
    /// Cases with no unique winner (all-tie, or no arm produced a score).
    pub ties: usize,
}

/// One case compared across every arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonRow {
    pub case_id: String,
    /// Per-arm cell, in matrix order.
    pub cells: Vec<ComparisonCell>,
    /// Index (into `cells`/`arms`) of the arm with the strictly highest mean
    /// case score; `None` when the case is a tie (multiple arms share the max
    /// within [`WINNER_TOLERANCE`], or no arm produced any score).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winner: Option<usize>,
}

/// One arm's outcome on one case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonCell {
    pub passed: bool,
    /// Mean of this case's `Score.value`s for this arm; `None` when the arm
    /// produced no scores (e.g. a target error), which contributes to no winner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_score: Option<f64>,
}

/// Per-arm aggregates across the whole suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmStats {
    pub target: String,
    pub passed: usize,
    pub failed: usize,
    /// Sum of per-case costs for this arm; `None` when no case was costed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    /// Cases this arm won outright (unique max mean score).
    pub wins: usize,
}

/// Mean of a case result's scorer values; `None` when it has no scores (a
/// target error, or a budget-skipped case), which never wins a comparison.
fn mean_score(result: &crate::types::CaseResult) -> Option<f64> {
    if result.scores.is_empty() {
        return None;
    }
    let sum: f64 = result.scores.iter().map(|s| s.value).sum();
    Some(sum / result.scores.len() as f64)
}

/// The winning arm index for a row: the unique arm whose mean score is the max,
/// within [`WINNER_TOLERANCE`]. `None` when two or more arms share the max
/// (all-tie) or no arm produced a score.
fn pick_winner(cells: &[ComparisonCell]) -> Option<usize> {
    let best = cells
        .iter()
        .filter_map(|c| c.mean_score)
        .fold(None, |acc: Option<f64>, m| {
            Some(acc.map_or(m, |b| if m > b { m } else { b }))
        })?;
    let mut at_max = cells.iter().enumerate().filter(|(_, c)| {
        c.mean_score
            .map(|m| (best - m).abs() <= WINNER_TOLERANCE)
            .unwrap_or(false)
    });
    let first = at_max.next()?;
    // A second arm within tolerance of the max means the case is tied.
    if at_max.next().is_some() {
        None
    } else {
        Some(first.0)
    }
}

/// Compare a matrix run's arms into per-case rows and per-arm aggregates.
///
/// Pure and deterministic. Rows follow the first arm's dataset order; each
/// arm's cell for a case is matched by case id (a case missing from an arm — an
/// arm that ran a different suite — yields a failing, score-less cell). The
/// winner is the unique-max mean case score with a [`WINNER_TOLERANCE`] tie
/// tolerance, applied for any number of arms.
pub fn compare_arms(matrix: &MatrixSummary) -> MatrixComparison {
    // Index each arm's results by case id so cells match by id, not position.
    let indexed: Vec<BTreeMap<&str, &crate::types::CaseResult>> = matrix
        .arms
        .iter()
        .map(|arm| {
            arm.summary
                .results
                .iter()
                .map(|r| (r.case_id.as_str(), r))
                .collect()
        })
        .collect();

    let mut wins = vec![0usize; matrix.arms.len()];
    let mut ties = 0usize;
    let mut rows = Vec::new();
    // The first arm defines the dataset order for the comparison rows.
    if let Some(first) = matrix.arms.first() {
        for case in &first.summary.results {
            let cells: Vec<ComparisonCell> = indexed
                .iter()
                .map(|arm| match arm.get(case.case_id.as_str()) {
                    Some(result) => ComparisonCell {
                        passed: result.passed(),
                        mean_score: mean_score(result),
                    },
                    None => ComparisonCell {
                        passed: false,
                        mean_score: None,
                    },
                })
                .collect();
            let winner = pick_winner(&cells);
            match winner {
                Some(i) => wins[i] += 1,
                None => ties += 1,
            }
            rows.push(ComparisonRow {
                case_id: case.case_id.clone(),
                cells,
                winner,
            });
        }
    }

    let arms = matrix
        .arms
        .iter()
        .enumerate()
        .map(|(i, arm)| ArmStats {
            target: arm.target.clone(),
            passed: arm.summary.passed(),
            failed: arm.summary.failed(),
            total_cost_usd: arm.summary.total_cost_usd(),
            wins: wins[i],
        })
        .collect();

    MatrixComparison { rows, arms, ties }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CaseResult, Score, TargetOutput};

    /// A costless case with the given id and a single `contains` score.
    fn scored(case_id: &str, value: f64, passed: bool) -> CaseResult {
        CaseResult {
            case_id: case_id.into(),
            output: Some(TargetOutput {
                text: "x".into(),
                latency_ms: 1,
                tokens: None,
                trajectory: None,
            }),
            error: None,
            scores: vec![Score {
                scorer: "contains".into(),
                value,
                passed,
                reason: None,
            }],
            cost_usd: None,
            context: None,
            trials: None,
        }
    }

    /// A target-error case: no scores, so it never wins.
    fn errored(case_id: &str) -> CaseResult {
        CaseResult {
            case_id: case_id.into(),
            output: None,
            error: Some("boom".into()),
            scores: vec![],
            cost_usd: None,
            context: None,
            trials: None,
        }
    }

    fn arm(target: &str, results: Vec<CaseResult>) -> MatrixArm {
        MatrixArm {
            target: target.into(),
            summary: RunSummary {
                results,
                gates: Vec::new(),
                classification: None,
            },
        }
    }

    #[test]
    fn two_arm_unique_winner_and_dataset_order() {
        let matrix = MatrixSummary {
            arms: vec![
                arm(
                    "gpt",
                    vec![scored("refund-1", 1.0, true), scored("refund-2", 1.0, true)],
                ),
                arm(
                    "claude",
                    vec![
                        scored("refund-1", 1.0, true),
                        scored("refund-2", 0.0, false),
                    ],
                ),
            ],
        };
        let cmp = compare_arms(&matrix);
        // Rows in dataset order.
        assert_eq!(cmp.rows[0].case_id, "refund-1");
        assert_eq!(cmp.rows[1].case_id, "refund-2");
        // refund-1: both 1.0 → tie; refund-2: gpt uniquely max → gpt (arm 0).
        assert_eq!(cmp.rows[0].winner, None);
        assert_eq!(cmp.rows[1].winner, Some(0));
        assert_eq!(cmp.ties, 1);
        assert_eq!(cmp.arms[0].wins, 1);
        assert_eq!(cmp.arms[1].wins, 0);
        assert_eq!(cmp.arms[0].passed, 2);
        assert_eq!(cmp.arms[1].failed, 1);
    }

    #[test]
    fn three_arm_unique_max_wins() {
        let matrix = MatrixSummary {
            arms: vec![
                arm("a", vec![scored("c1", 0.5, false)]),
                arm("b", vec![scored("c1", 0.9, true)]),
                arm("c", vec![scored("c1", 0.7, false)]),
            ],
        };
        let cmp = compare_arms(&matrix);
        assert_eq!(cmp.rows[0].winner, Some(1));
        assert_eq!(cmp.arms[1].wins, 1);
        assert_eq!(cmp.ties, 0);
    }

    #[test]
    fn tie_within_tolerance_is_a_tie() {
        let matrix = MatrixSummary {
            arms: vec![
                arm("a", vec![scored("c1", 1.0, true)]),
                arm("b", vec![scored("c1", 1.0 + 5e-10, true)]),
            ],
        };
        let cmp = compare_arms(&matrix);
        assert_eq!(
            cmp.rows[0].winner, None,
            "a 5e-10 gap is within 1e-9 tolerance → tie"
        );
        assert_eq!(cmp.ties, 1);
    }

    #[test]
    fn no_scores_case_is_a_tie() {
        let matrix = MatrixSummary {
            arms: vec![arm("a", vec![errored("c1")]), arm("b", vec![errored("c1")])],
        };
        let cmp = compare_arms(&matrix);
        assert_eq!(cmp.rows[0].winner, None);
        assert_eq!(cmp.rows[0].cells[0].mean_score, None);
        assert_eq!(cmp.ties, 1);
    }

    #[test]
    fn one_arm_scored_others_errored_wins() {
        // Only arm b produced a score; it is the unique max → winner.
        let matrix = MatrixSummary {
            arms: vec![
                arm("a", vec![errored("c1")]),
                arm("b", vec![scored("c1", 0.3, false)]),
            ],
        };
        let cmp = compare_arms(&matrix);
        assert_eq!(cmp.rows[0].winner, Some(1));
        assert_eq!(cmp.arms[1].wins, 1);
    }
}
