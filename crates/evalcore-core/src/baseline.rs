//! Baseline comparison: the "did anything get worse?" half of snapshot
//! testing. Pure functions over two `RunSummary` values — storage lives in
//! `evalcore-store`, rendering in `evalcore-report`.

use std::collections::BTreeMap;

use crate::types::RunSummary;

/// One case that fails now but is not an accepted failure in the baseline.
#[derive(Debug, Clone)]
pub struct CaseRegression {
    pub case_id: String,
    pub reasons: Vec<String>,
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
    let baseline_by_id: BTreeMap<&str, bool> = baseline
        .results
        .iter()
        .map(|r| (r.case_id.as_str(), r.passed()))
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
            Some(true) if !now_passes => diff.regressions.push(CaseRegression {
                case_id: result.case_id.clone(),
                reasons: result.failure_reasons(),
            }),
            Some(false) if now_passes => diff.fixed.push(result.case_id.clone()),
            None if !now_passes => diff.new_failing.push(CaseRegression {
                case_id: result.case_id.clone(),
                reasons: result.failure_reasons(),
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
    use crate::types::{CaseResult, Score};

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
}
