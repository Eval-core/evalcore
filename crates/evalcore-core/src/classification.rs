//! Classification aggregates: accuracy and macro-F1 over a run's labeled cases.
//!
//! [`compute_classification`] is a pure, deterministic function of the cases and
//! their results — identical inputs yield identical output, with `per_class`
//! sorted by label. Wiring (deciding when to compute it and attaching it to the
//! [`RunSummary`](crate::types::RunSummary)) lives in the CLI; nothing here reads
//! the clock or the environment.
//!
//! Semantics (v1, locked):
//! - A case is *labeled* iff it has `expected`. Its label is `expected.trim()`
//!   (the string value verbatim, other JSON scalars via their display form); its
//!   prediction is the case-level output text `.trim()`. Matching is
//!   case-sensitive exact — no normalization beyond trim.
//! - A target-error case with `expected` is labeled-and-wrong: it has no output,
//!   so it matches no label (an error storm sinks accuracy).
//! - The class set is the observed *expected* labels only. Precision of class
//!   `c` = correct(`c`) / predicted-as-`c` among labeled cases; a prediction
//!   that matches no expected label enters no precision denominator (it only
//!   lowers its true class's recall). Every 0/0 guard yields `0.0`.
//! - Multi-trial runs: the prediction is the case-level surfaced output.

use std::collections::BTreeSet;

use crate::types::{CaseResult, ClassMetrics, ClassificationSummary, TestCase};

/// The classification label of a case: its trimmed `expected` value, or `None`
/// when the case is unlabeled. Mirrors the `exact` scorer's `expected`
/// interpretation (string verbatim, other scalars via display) before trimming.
fn expected_label(case: &TestCase) -> Option<String> {
    let raw = match case.expected.as_ref()? {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    Some(raw.trim().to_string())
}

/// Compute accuracy and macro-F1 over the labeled cases. `cases` and `results`
/// are paired positionally — the engine preserves dataset order, so
/// `results[i]` is the outcome of `cases[i]`.
pub fn compute_classification(cases: &[TestCase], results: &[CaseResult]) -> ClassificationSummary {
    // (expected label, predicted label) for each labeled case; the prediction is
    // `None` when the target errored (no output), which matches no class.
    let mut labeled: Vec<(String, Option<String>)> = Vec::new();
    let mut unlabeled_cases = 0usize;
    for (case, result) in cases.iter().zip(results.iter()) {
        match expected_label(case) {
            Some(expected) => {
                let prediction = result
                    .output
                    .as_ref()
                    .map(|output| output.text.trim().to_string());
                labeled.push((expected, prediction));
            }
            None => unlabeled_cases += 1,
        }
    }

    let labeled_cases = labeled.len();
    if labeled_cases == 0 {
        return ClassificationSummary {
            labeled_cases: 0,
            unlabeled_cases,
            accuracy: 0.0,
            macro_f1: 0.0,
            per_class: Vec::new(),
        };
    }

    let correct = labeled
        .iter()
        .filter(|(expected, prediction)| prediction.as_deref() == Some(expected.as_str()))
        .count();
    let accuracy = correct as f64 / labeled_cases as f64;

    // Class set = expected labels only; the BTreeSet keeps `per_class` sorted.
    let label_set: BTreeSet<&str> = labeled
        .iter()
        .map(|(expected, _)| expected.as_str())
        .collect();
    let per_class: Vec<ClassMetrics> = label_set
        .into_iter()
        .map(|label| {
            let support = labeled
                .iter()
                .filter(|(expected, _)| expected.as_str() == label)
                .count();
            let predicted_as = labeled
                .iter()
                .filter(|(_, prediction)| prediction.as_deref() == Some(label))
                .count();
            let correct_c = labeled
                .iter()
                .filter(|(expected, prediction)| {
                    expected.as_str() == label && prediction.as_deref() == Some(label)
                })
                .count();
            let precision = if predicted_as == 0 {
                0.0
            } else {
                correct_c as f64 / predicted_as as f64
            };
            let recall = if support == 0 {
                0.0
            } else {
                correct_c as f64 / support as f64
            };
            let f1 = if precision + recall == 0.0 {
                0.0
            } else {
                2.0 * precision * recall / (precision + recall)
            };
            ClassMetrics {
                label: label.to_string(),
                precision,
                recall,
                f1,
                support,
            }
        })
        .collect();

    let macro_f1 = per_class.iter().map(|m| m.f1).sum::<f64>() / per_class.len() as f64;

    ClassificationSummary {
        labeled_cases,
        unlabeled_cases,
        accuracy,
        macro_f1,
        per_class,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TargetOutput;

    fn case(id: &str, expected: Option<&str>) -> TestCase {
        TestCase {
            id: id.into(),
            input: String::new(),
            expected: expected.map(|e| serde_json::Value::String(e.into())),
            trace: None,
            context: None,
        }
    }

    /// A result whose output text is `prediction`; `None` means the target
    /// errored (no output).
    fn result(id: &str, prediction: Option<&str>) -> CaseResult {
        CaseResult {
            case_id: id.into(),
            output: prediction.map(|text| TargetOutput {
                text: text.into(),
                latency_ms: 1,
                tokens: None,
                trajectory: None,
            }),
            error: prediction.is_none().then(|| "boom".into()),
            scores: vec![],
            cost_usd: None,
            context: None,
            trials: None,
        }
    }

    #[test]
    fn multi_class_precision_recall_f1_hand_computed() {
        // Three classes: cat, dog, bird. Predictions include a correct hit per
        // class plus a cat->dog confusion and a dog->"fish" hallucination.
        //
        //  case  expected  predicted
        //   1      cat       cat       (correct)
        //   2      cat       dog       (cat miss; dog false positive)
        //   3      dog       dog       (correct)
        //   4      dog       fish      (dog miss; fish is not a class)
        //   5      bird      bird      (correct)
        //
        // Accuracy = 3/5 = 0.6.
        // cat:  support 2, predicted-as-cat 1, correct 1 -> P 1.0, R 0.5,  F1 0.6667
        // dog:  support 2, predicted-as-dog 2, correct 1 -> P 0.5, R 0.5,  F1 0.5
        // bird: support 1, predicted-as-bird 1, correct 1 -> P 1.0, R 1.0, F1 1.0
        // macro-F1 = (0.6667 + 0.5 + 1.0) / 3 = 0.7222.
        let cases = vec![
            case("1", Some("cat")),
            case("2", Some("cat")),
            case("3", Some("dog")),
            case("4", Some("dog")),
            case("5", Some("bird")),
        ];
        let results = vec![
            result("1", Some("cat")),
            result("2", Some("dog")),
            result("3", Some("dog")),
            result("4", Some("fish")),
            result("5", Some("bird")),
        ];
        let summary = compute_classification(&cases, &results);

        assert_eq!(summary.labeled_cases, 5);
        assert_eq!(summary.unlabeled_cases, 0);
        assert!((summary.accuracy - 0.6).abs() < 1e-12);

        // per_class is sorted by label: bird, cat, dog.
        let labels: Vec<&str> = summary.per_class.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["bird", "cat", "dog"]);

        let bird = &summary.per_class[0];
        assert_eq!(bird.support, 1);
        assert!((bird.precision - 1.0).abs() < 1e-12);
        assert!((bird.recall - 1.0).abs() < 1e-12);
        assert!((bird.f1 - 1.0).abs() < 1e-12);

        let cat = &summary.per_class[1];
        assert_eq!(cat.support, 2);
        assert!((cat.precision - 1.0).abs() < 1e-12);
        assert!((cat.recall - 0.5).abs() < 1e-12);
        assert!((cat.f1 - 2.0 / 3.0).abs() < 1e-12);

        let dog = &summary.per_class[2];
        assert_eq!(dog.support, 2);
        assert!((dog.precision - 0.5).abs() < 1e-12);
        assert!((dog.recall - 0.5).abs() < 1e-12);
        assert!((dog.f1 - 0.5).abs() < 1e-12);

        let expected_macro = (2.0 / 3.0 + 0.5 + 1.0) / 3.0;
        assert!((summary.macro_f1 - expected_macro).abs() < 1e-12);
    }

    #[test]
    fn error_case_with_expected_is_labeled_and_wrong() {
        // The dog case errored (no output): it is labeled but predicted nothing,
        // so accuracy is 1/2 and dog's recall is 0.
        let cases = vec![case("1", Some("cat")), case("2", Some("dog"))];
        let results = vec![result("1", Some("cat")), result("2", None)];
        let summary = compute_classification(&cases, &results);

        assert_eq!(summary.labeled_cases, 2);
        assert!((summary.accuracy - 0.5).abs() < 1e-12);
        let dog = summary.per_class.iter().find(|m| m.label == "dog").unwrap();
        assert_eq!(dog.support, 1);
        assert_eq!(dog.recall, 0.0, "an errored case never matches its class");
        assert_eq!(dog.precision, 0.0, "nothing was predicted as dog");
        assert_eq!(dog.f1, 0.0);
    }

    #[test]
    fn zero_over_zero_guards_yield_zero() {
        // A single class predicted as a hallucination: precision 0/0 -> 0.0,
        // recall 0/1 -> 0.0, F1 -> 0.0.
        let cases = vec![case("1", Some("yes"))];
        let results = vec![result("1", Some("maybe"))];
        let summary = compute_classification(&cases, &results);
        let yes = &summary.per_class[0];
        assert_eq!(yes.precision, 0.0);
        assert_eq!(yes.recall, 0.0);
        assert_eq!(yes.f1, 0.0);
        assert_eq!(summary.accuracy, 0.0);
        assert_eq!(summary.macro_f1, 0.0);
    }

    #[test]
    fn unlabeled_cases_are_counted_and_excluded() {
        // Two labeled, one unlabeled: the unlabeled case never enters a metric.
        let cases = vec![
            case("1", Some("cat")),
            case("2", None),
            case("3", Some("cat")),
        ];
        let results = vec![
            result("1", Some("cat")),
            result("2", Some("cat")),
            result("3", Some("dog")),
        ];
        let summary = compute_classification(&cases, &results);
        assert_eq!(summary.labeled_cases, 2);
        assert_eq!(summary.unlabeled_cases, 1);
        assert!((summary.accuracy - 0.5).abs() < 1e-12);
        assert_eq!(summary.per_class.len(), 1, "only cat is an expected label");
    }

    #[test]
    fn all_unlabeled_yields_zero_metrics() {
        let cases = vec![case("1", None), case("2", None)];
        let results = vec![result("1", Some("x")), result("2", Some("y"))];
        let summary = compute_classification(&cases, &results);
        assert_eq!(summary.labeled_cases, 0);
        assert_eq!(summary.unlabeled_cases, 2);
        assert_eq!(summary.accuracy, 0.0);
        assert_eq!(summary.macro_f1, 0.0);
        assert!(summary.per_class.is_empty());
    }

    #[test]
    fn labels_are_trimmed() {
        // Surrounding whitespace on expected and prediction is trimmed before
        // matching, so "  cat " and "cat" are the same class.
        let cases = vec![case("1", Some("  cat "))];
        let results = vec![result("1", Some("cat\n"))];
        let summary = compute_classification(&cases, &results);
        assert_eq!(summary.per_class[0].label, "cat");
        assert!((summary.accuracy - 1.0).abs() < 1e-12);
    }
}
