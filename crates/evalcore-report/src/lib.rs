//! Report rendering. Every reporter is a pure function `&RunSummary -> String`
//! — no I/O, no clock, no global state — so outputs are snapshot-testable and
//! identical for identical runs.

use evalcore_core::RunSummary;

/// Human-readable report for terminals and logs.
pub fn terminal(summary: &RunSummary) -> String {
    let mut out = String::new();
    for result in &summary.results {
        if result.passed() {
            let latency = result.output.as_ref().map_or(0, |o| o.latency_ms);
            out.push_str(&format!("PASS {} ({latency}ms)\n", result.case_id));
        } else {
            out.push_str(&format!("FAIL {}\n", result.case_id));
            for reason in result.failure_reasons() {
                out.push_str(&format!("     {reason}\n"));
            }
        }
    }
    let mut totals = String::new();
    if let Some(tokens) = summary.total_tokens() {
        totals.push_str(&format!(" · {} tokens", tokens.total()));
    }
    if let Some(cost) = summary.total_cost_usd() {
        totals.push_str(&format!(" · ${cost:.4}"));
    }
    out.push_str(&format!(
        "\n{} passed, {} failed, {} total{totals}\n",
        summary.passed(),
        summary.failed(),
        summary.total()
    ));
    out
}

/// Machine-readable report: the full `RunSummary` as pretty JSON.
pub fn json(summary: &RunSummary) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(summary)?)
}

/// JUnit XML for CI systems (GitHub Actions, GitLab, Jenkins all ingest it).
pub fn junit(summary: &RunSummary) -> String {
    let mut cases = String::new();
    for result in &summary.results {
        let name = xml_escape(&result.case_id);
        let time = result.output.as_ref().map_or(0, |o| o.latency_ms) as f64 / 1000.0;
        if result.passed() {
            cases.push_str(&format!(
                r#"    <testcase name="{name}" time="{time:.3}"/>"#
            ));
            cases.push('\n');
        } else {
            let message = xml_escape(&result.failure_reasons().join("; "));
            cases.push_str(&format!(
                "    <testcase name=\"{name}\" time=\"{time:.3}\">\n      <failure message=\"{message}\"/>\n    </testcase>\n"
            ));
        }
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites tests=\"{total}\" failures=\"{failed}\">\n  <testsuite name=\"evalcore\" tests=\"{total}\" failures=\"{failed}\">\n{cases}  </testsuite>\n</testsuites>\n",
        total = summary.total(),
        failed = summary.failed(),
    )
}

/// Baseline comparison section, appended after the main report (stdout for
/// terminal runs, stderr when a machine reporter owns stdout).
pub fn baseline(diff: &evalcore_core::BaselineDiff, label: &str) -> String {
    let mut out = format!(
        "\nbaseline {label:?}: {}/{} passed -> current: {}/{} passed\n",
        diff.baseline_passed, diff.baseline_total, diff.current_passed, diff.current_total
    );
    for regression in &diff.regressions {
        out.push_str(&format!("REGRESSED {}\n", regression.case_id));
        for reason in &regression.reasons {
            out.push_str(&format!("     {reason}\n"));
        }
    }
    for new_failing in &diff.new_failing {
        out.push_str(&format!("NEW FAIL  {}\n", new_failing.case_id));
        for reason in &new_failing.reasons {
            out.push_str(&format!("     {reason}\n"));
        }
    }
    for fixed in &diff.fixed {
        out.push_str(&format!("FIXED     {fixed}\n"));
    }
    for removed in &diff.removed {
        out.push_str(&format!("REMOVED   {removed}\n"));
    }
    if diff.gate_failed() {
        out.push_str(&format!(
            "baseline gate: FAIL ({} regressed, {} new failing)\n",
            diff.regressions.len(),
            diff.new_failing.len()
        ));
    } else {
        out.push_str("baseline gate: OK — no regressions\n");
    }
    out
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use evalcore_core::{CaseResult, Score, TargetOutput, TokenUsage};

    /// Fixture with fixed latencies/tokens so every reporter output is
    /// deterministic.
    fn fixture() -> RunSummary {
        RunSummary {
            results: vec![
                CaseResult {
                    case_id: "refund-1".into(),
                    output: Some(TargetOutput {
                        text: "refund issued".into(),
                        latency_ms: 12,
                        tokens: Some(TokenUsage {
                            input: 100,
                            output: 20,
                        }),
                        trajectory: None,
                    }),
                    error: None,
                    scores: vec![Score {
                        scorer: "contains".into(),
                        value: 1.0,
                        passed: true,
                        reason: None,
                    }],
                    cost_usd: Some(0.0012),
                },
                CaseResult {
                    case_id: "refund-2".into(),
                    output: Some(TargetOutput {
                        text: "cannot help".into(),
                        latency_ms: 40,
                        tokens: Some(TokenUsage {
                            input: 80,
                            output: 10,
                        }),
                        trajectory: None,
                    }),
                    error: None,
                    scores: vec![Score {
                        scorer: "contains".into(),
                        value: 0.0,
                        passed: false,
                        reason: Some("expected output to contain \"refund\" & <more>".into()),
                    }],
                    cost_usd: Some(0.0008),
                },
                CaseResult {
                    case_id: "boom".into(),
                    output: None,
                    error: Some("target error: connection refused".into()),
                    scores: vec![],
                    cost_usd: None,
                },
            ],
        }
    }

    #[test]
    fn terminal_report_snapshot() {
        insta::assert_snapshot!(terminal(&fixture()));
    }

    #[test]
    fn junit_report_snapshot() {
        insta::assert_snapshot!(junit(&fixture()));
    }

    #[test]
    fn junit_escapes_xml_metacharacters() {
        let xml = junit(&fixture());
        assert!(xml.contains("&amp;"), "raw & must be escaped");
        assert!(
            xml.contains("&lt;more&gt;"),
            "angle brackets must be escaped"
        );
        assert!(!xml.contains("<more>"), "no unescaped payload in XML");
    }

    #[test]
    fn baseline_section_snapshot() {
        // Baseline: refund-2 passed and boom didn't exist; current run (the
        // fixture) fails refund-2 (regression) and adds failing boom.
        let baseline_run = RunSummary {
            results: vec![
                CaseResult {
                    case_id: "refund-1".into(),
                    output: None,
                    error: None,
                    scores: vec![],
                    cost_usd: None,
                },
                CaseResult {
                    case_id: "refund-2".into(),
                    output: None,
                    error: None,
                    scores: vec![],
                    cost_usd: None,
                },
                CaseResult {
                    case_id: "retired".into(),
                    output: None,
                    error: Some("was failing".into()),
                    scores: vec![],
                    cost_usd: None,
                },
            ],
        };
        let diff = evalcore_core::compare(&baseline_run, &fixture());
        insta::assert_snapshot!(baseline(&diff, "main"));
    }

    #[test]
    fn json_report_round_trips() {
        let rendered = json(&fixture()).unwrap();
        let parsed: RunSummary = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed.total(), 3);
        assert_eq!(parsed.failed(), 2);
    }
}
