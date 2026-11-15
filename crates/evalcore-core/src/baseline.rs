//! Baseline comparison: the "did anything get worse?" half of snapshot
//! testing. Pure functions over two `RunSummary` values — storage lives in
//! `evalcore-store`, rendering in `evalcore-report`.

use std::collections::BTreeMap;

use llm_diff::{DiffOp, TextDiff};

use crate::types::{CaseResult, RunSummary};

/// One case that fails now but is not an accepted failure in the baseline.
#[derive(Debug, Clone)]
pub struct CaseRegression {
    pub case_id: String,
    pub reasons: Vec<String>,
    /// How the output moved away from the baseline's. Set only for
    /// regressions where both runs recorded output text and the texts differ.
    pub output_change: Option<OutputChange>,
}

/// Word-level diff between a case's baseline output and its current output.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputChange {
    /// Order-aware word similarity, `2 * matched / total` words, in
    /// `0.0..=1.0` (1.0 = same words in the same order).
    pub ratio: f64,
    /// Changed runs in output order: `-` prefixes baseline text, `+` current
    /// text. Adjacent changed words (and the whitespace between them) form one
    /// run.
    pub changes: Vec<String>,
}

impl OutputChange {
    /// Word-diff `baseline` against `current`; `None` when they are equal.
    pub fn between(baseline: &str, current: &str) -> Option<Self> {
        if baseline == current {
            return None;
        }
        let diff = TextDiff::compute_words(baseline, current);
        let mut changes = Vec::new();
        let (mut deleted, mut inserted) = (String::new(), String::new());
        // Unchanged whitespace seen since each side's last changed word.
        let (mut deleted_gap, mut inserted_gap) = (String::new(), String::new());
        for op in diff.ops() {
            match op {
                DiffOp::Delete(text) => {
                    if !deleted.is_empty() {
                        deleted.push_str(&deleted_gap);
                    }
                    deleted_gap.clear();
                    deleted.push_str(text);
                }
                DiffOp::Insert(text) => {
                    if !inserted.is_empty() {
                        inserted.push_str(&inserted_gap);
                    }
                    inserted_gap.clear();
                    inserted.push_str(text);
                }
                // Whitespace between two changed words keeps the run open.
                _ if op.text().trim().is_empty()
                    && !(deleted.is_empty() && inserted.is_empty()) =>
                {
                    deleted_gap.push_str(op.text());
                    inserted_gap.push_str(op.text());
                }
                _ => {
                    flush_run(&mut changes, &mut deleted, &mut inserted);
                    deleted_gap.clear();
                    inserted_gap.clear();
                }
            }
        }
        flush_run(&mut changes, &mut deleted, &mut inserted);
        Some(Self {
            ratio: diff.ratio(),
            changes,
        })
    }

    /// One-line rendering for reports, e.g. `89% similar: -2.1 +2.2`.
    /// Truncated to ~200 chars so a rewritten answer doesn't flood the report.
    pub fn summary(&self) -> String {
        const MAX: usize = 200;
        let line = format!(
            "{:.0}% similar: {}",
            self.ratio * 100.0,
            self.changes.join(" ")
        );
        if line.chars().count() <= MAX {
            line
        } else {
            let cut: String = line.chars().take(MAX).collect();
            format!("{cut}…")
        }
    }
}

fn flush_run(changes: &mut Vec<String>, deleted: &mut String, inserted: &mut String) {
    for (sign, run) in [('-', deleted), ('+', inserted)] {
        if !run.is_empty() {
            // Trim edge whitespace, unless whitespace is the whole change.
            let text = match run.trim() {
                "" => run.as_str(),
                trimmed => trimmed,
            };
            changes.push(format!("{sign}{text}"));
            run.clear();
        }
    }
}

fn output_change(baseline: &CaseResult, current: &CaseResult) -> Option<OutputChange> {
    OutputChange::between(
        &baseline.output.as_ref()?.text,
        &current.output.as_ref()?.text,
    )
}

/// The difference between a stored baseline run and the current run.
///
/// Gate semantics: `gate_failed()` is true when any case regressed
/// (passed → failed) or any case unknown to the baseline is failing.
/// Failures already present in the baseline are *accepted* — that's the
/// point of a baseline — and fixed or removed cases never fail the gate.
#[derive(Debug, Clone, Default)]
pub struct BaselineDiff {
    /// Passed in the baseline, fails now.
    pub regressions: Vec<CaseRegression>,
    /// Not present in the baseline, fails now.
    pub new_failing: Vec<CaseRegression>,
    /// Failed in the baseline, passes now.
    pub fixed: Vec<String>,
    /// Present in the baseline, absent now.
    pub removed: Vec<String>,
    pub baseline_passed: usize,
    pub baseline_total: usize,
    pub current_passed: usize,
    pub current_total: usize,
}

impl BaselineDiff {
    pub fn gate_failed(&self) -> bool {
        !self.regressions.is_empty() || !self.new_failing.is_empty()
    }
}

/// Compare `current` against `baseline`, matching cases by id. Output vectors
/// follow `current`'s dataset order (and `baseline`'s for `removed`), so the
/// diff is deterministic.
pub fn compare(baseline: &RunSummary, current: &RunSummary) -> BaselineDiff {
    let baseline_by_id: BTreeMap<&str, &CaseResult> = baseline
        .results
        .iter()
        .map(|r| (r.case_id.as_str(), r))
        .collect();
    let current_ids: BTreeMap<&str, ()> = current
        .results
        .iter()
        .map(|r| (r.case_id.as_str(), ()))
        .collect();

    let mut diff = BaselineDiff {
        baseline_passed: baseline.passed(),
        baseline_total: baseline.total(),
        current_passed: current.passed(),
        current_total: current.total(),
        ..Default::default()
    };

    for result in &current.results {
        let now_passes = result.passed();
        match baseline_by_id.get(result.case_id.as_str()) {
            Some(before) if before.passed() && !now_passes => {
                diff.regressions.push(CaseRegression {
                    case_id: result.case_id.clone(),
                    reasons: result.failure_reasons(),
                    output_change: output_change(before, result),
                })
            }
            Some(before) if !before.passed() && now_passes => {
                diff.fixed.push(result.case_id.clone())
            }
            None if !now_passes => diff.new_failing.push(CaseRegression {
                case_id: result.case_id.clone(),
                reasons: result.failure_reasons(),
                output_change: None,
            }),
            _ => {}
        }
    }

    for result in &baseline.results {
        if !current_ids.contains_key(result.case_id.as_str()) {
            diff.removed.push(result.case_id.clone());
        }
    }

    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Score, TargetOutput};

    fn result(case_id: &str, passed: bool) -> CaseResult {
        CaseResult {
            case_id: case_id.into(),
            output: None,
            error: (!passed).then(|| "it broke".into()),
            scores: vec![Score {
                scorer: "test".into(),
                value: if passed { 1.0 } else { 0.0 },
                passed,
                reason: None,
                tokens: None,
                cost_usd: None,
            }],
            cost_usd: None,
            context: None,
            trials: None,
        }
    }

    fn summary(cases: &[(&str, bool)]) -> RunSummary {
        RunSummary {
            results: cases.iter().map(|(id, p)| result(id, *p)).collect(),
            gates: Vec::new(),
            classification: None,
        }
    }

    #[test]
    fn regression_fails_the_gate_with_reasons() {
        let baseline = summary(&[("a", true), ("b", true)]);
        let current = summary(&[("a", true), ("b", false)]);
        let diff = compare(&baseline, &current);

        assert!(diff.gate_failed());
        assert_eq!(diff.regressions.len(), 1);
        assert_eq!(diff.regressions[0].case_id, "b");
        assert!(diff.regressions[0].reasons[0].contains("it broke"));
    }

    #[test]
    fn accepted_failures_pass_the_gate() {
        let baseline = summary(&[("a", true), ("b", false)]);
        let current = summary(&[("a", true), ("b", false)]);
        let diff = compare(&baseline, &current);

        assert!(!diff.gate_failed(), "known failure must be tolerated");
        assert!(diff.regressions.is_empty());
    }

    #[test]
    fn new_failing_cases_fail_the_gate_but_new_passing_do_not() {
        let baseline = summary(&[("a", true)]);
        let current = summary(&[("a", true), ("new-bad", false), ("new-good", true)]);
        let diff = compare(&baseline, &current);

        assert!(diff.gate_failed());
        assert_eq!(diff.new_failing.len(), 1);
        assert_eq!(diff.new_failing[0].case_id, "new-bad");
    }

    #[test]
    fn fixes_and_removals_are_informational() {
        let baseline = summary(&[("a", false), ("gone", true)]);
        let current = summary(&[("a", true)]);
        let diff = compare(&baseline, &current);

        assert!(!diff.gate_failed());
        assert_eq!(diff.fixed, ["a"]);
        assert_eq!(diff.removed, ["gone"]);
        assert_eq!((diff.baseline_passed, diff.baseline_total), (1, 2));
        assert_eq!((diff.current_passed, diff.current_total), (1, 1));
    }

    fn with_output(mut result: CaseResult, text: &str) -> CaseResult {
        result.output = Some(TargetOutput {
            text: text.into(),
            latency_ms: 0,
            tokens: None,
            trajectory: None,
        });
        result
    }

    #[test]
    fn regression_carries_word_diff_of_the_output() {
        let baseline = RunSummary {
            results: vec![with_output(
                result("pop", true),
                "Paris has 2.1 million people.",
            )],
            gates: Vec::new(),
            classification: None,
        };
        let current = RunSummary {
            results: vec![with_output(
                result("pop", false),
                "Paris has 2.2 million people.",
            )],
            gates: Vec::new(),
            classification: None,
        };
        let diff = compare(&baseline, &current);

        let change = diff.regressions[0]
            .output_change
            .as_ref()
            .expect("both runs have output");
        assert_eq!(change.changes, ["-2.1", "+2.2"]);
        assert!(change.ratio > 0.8 && change.ratio < 1.0, "{}", change.ratio);
        assert_eq!(change.summary(), "89% similar: -2.1 +2.2");
    }

    #[test]
    fn output_change_is_absent_without_two_differing_outputs() {
        // Baseline has no output recorded.
        let diff = compare(
            &summary(&[("a", true)]),
            &RunSummary {
                results: vec![with_output(result("a", false), "now")],
                gates: Vec::new(),
                classification: None,
            },
        );
        assert!(diff.regressions[0].output_change.is_none());

        // Identical text that a scorer now rejects: nothing to diff.
        assert!(OutputChange::between("same", "same").is_none());
    }

    #[test]
    fn adjacent_changed_words_form_one_run() {
        let change =
            OutputChange::between("refunds take 30 days", "refunds are not available").unwrap();
        assert_eq!(change.changes, ["-take 30 days", "+are not available"]);
    }
}
