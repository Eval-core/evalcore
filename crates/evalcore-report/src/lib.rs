//! Report rendering. Every reporter is a pure function `&RunSummary -> String`
//! — no I/O, no clock, no global state — so outputs are snapshot-testable and
//! identical for identical runs.

use evalcore_core::{BaselineDiff, RunSummary, TargetOutput, Trajectory};

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
    // Suite-level gate outcomes, one line each. Absent when no gates are
    // configured, so gate-free runs render byte-identically to before.
    for gate in &summary.gates {
        let status = if gate.passed { "PASS" } else { "FAIL" };
        out.push_str(&format!(
            "GATE {status} {} (actual {:.2})\n",
            gate.gate, gate.actual
        ));
        if let Some(reason) = &gate.reason {
            out.push_str(&format!("     {reason}\n"));
        }
    }
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

/// Self-contained HTML report: the shareable "here's the eval report" artifact
/// a reviewer clicks in a PR. One document, entirely inline (no external
/// requests, no fonts, no images), zero JavaScript (`<details>` drives every
/// expander), and deterministic — identical `summary`/`diff` render
/// byte-identical bytes, so it snapshot-tests like every other reporter.
///
/// `diff`, when present, embeds the same baseline comparison the terminal diff
/// renderer prints. Everything user-derived (case ids, outputs, reasons, tool
/// names, JSON payloads) is HTML-escaped, so a hostile output renders inert.
pub fn html(summary: &RunSummary, diff: Option<&BaselineDiff>) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>EvalCore report</title>\n");
    out.push_str("<style>\n");
    out.push_str(HTML_STYLE);
    out.push_str("</style>\n</head>\n<body>\n");
    out.push_str("<main>\n");

    // Header: the same figures the terminal reporter's summary line shows.
    let overall = if summary.all_passed() { "pass" } else { "fail" };
    out.push_str("<header>\n");
    out.push_str("<h1>EvalCore report</h1>\n");
    out.push_str("<div class=\"stats\">\n");
    out.push_str(&format!(
        "<span class=\"stat overall-{overall}\">{}</span>\n",
        if summary.all_passed() {
            "all passed"
        } else {
            "failing"
        }
    ));
    out.push_str(&format!(
        "<span class=\"stat pass\">{} passed</span>\n",
        summary.passed()
    ));
    out.push_str(&format!(
        "<span class=\"stat fail\">{} failed</span>\n",
        summary.failed()
    ));
    out.push_str(&format!(
        "<span class=\"stat\">{} total</span>\n",
        summary.total()
    ));
    if let Some(tokens) = summary.total_tokens() {
        out.push_str(&format!(
            "<span class=\"stat\">{} tokens</span>\n",
            tokens.total()
        ));
    }
    if let Some(cost) = summary.total_cost_usd() {
        out.push_str(&format!("<span class=\"stat\">${cost:.4}</span>\n"));
    }
    out.push_str("</div>\n</header>\n");

    // Gates panel — omitted entirely when no gates are configured.
    if !summary.gates.is_empty() {
        out.push_str("<section class=\"gates\">\n<h2>Gates</h2>\n");
        out.push_str("<table>\n<thead><tr><th>Status</th><th>Gate</th><th>Actual</th><th>Reason</th></tr></thead>\n<tbody>\n");
        for gate in &summary.gates {
            let (cls, label) = if gate.passed {
                ("pass", "PASS")
            } else {
                ("fail", "FAIL")
            };
            let reason = gate.reason.as_deref().map(html_escape).unwrap_or_default();
            out.push_str(&format!(
                "<tr><td><span class=\"badge {cls}\">{label}</span></td><td>{}</td><td class=\"num\">{:.2}</td><td>{reason}</td></tr>\n",
                html_escape(&gate.gate),
                gate.actual,
            ));
        }
        out.push_str("</tbody>\n</table>\n</section>\n");
    }

    // Case table: one expandable row per case, in dataset order.
    out.push_str("<section class=\"cases\">\n<h2>Cases</h2>\n");
    out.push_str("<div class=\"row head\"><span class=\"c-status\">Status</span><span class=\"c-id\">Case</span><span class=\"c-latency\">Latency</span><span class=\"c-cost\">Cost</span></div>\n");
    for result in &summary.results {
        push_case(&mut out, result);
    }
    out.push_str("</section>\n");

    // Baseline diff — same data the terminal diff renderer shows.
    if let Some(diff) = diff {
        push_baseline(&mut out, diff);
    }

    out.push_str("</main>\n</body>\n</html>\n");
    out
}

/// Render one case as an expandable `<details>` "row".
fn push_case(out: &mut String, result: &evalcore_core::CaseResult) {
    let (cls, label) = if result.passed() {
        ("pass", "PASS")
    } else {
        ("fail", "FAIL")
    };
    let latency = result.output.as_ref().map_or(0, |o| o.latency_ms);
    let cost = result
        .cost_usd
        .map(|c| format!("${c:.4}"))
        .unwrap_or_default();
    out.push_str("<details class=\"case\">\n");
    out.push_str(&format!(
        "<summary class=\"row\"><span class=\"c-status\"><span class=\"badge {cls}\">{label}</span></span><span class=\"c-id\">{}</span><span class=\"c-latency num\">{latency}ms</span><span class=\"c-cost num\">{cost}</span></summary>\n",
        html_escape(&result.case_id),
    ));
    out.push_str("<div class=\"case-body\">\n");

    if let Some(error) = &result.error {
        out.push_str(&format!(
            "<p class=\"error\">target error: {}</p>\n",
            html_escape(error)
        ));
    }

    // RAG context, when the case carried it: numbered chunks, each escaped.
    // Rendered before the output so a reviewer reads the retrieved evidence
    // first. Uses no dedicated CSS rule, so context-free reports (which never
    // emit this block) stay byte-identical.
    if let Some(context) = &result.context {
        out.push_str("<h3>Context</h3>\n<ol class=\"context\">\n");
        for chunk in context {
            out.push_str(&format!("<li>{}</li>\n", html_escape(chunk)));
        }
        out.push_str("</ol>\n");
    }

    if let Some(output) = &result.output {
        out.push_str("<h3>Output</h3>\n");
        out.push_str(&format!("<pre>{}</pre>\n", html_escape(&output.text)));
    }

    if !result.scores.is_empty() {
        out.push_str("<h3>Scores</h3>\n");
        out.push_str("<table>\n<thead><tr><th>Scorer</th><th>Value</th><th>Passed</th><th>Reason</th></tr></thead>\n<tbody>\n");
        for score in &result.scores {
            let (scls, slabel) = if score.passed {
                ("pass", "yes")
            } else {
                ("fail", "no")
            };
            let reason = score.reason.as_deref().map(html_escape).unwrap_or_default();
            out.push_str(&format!(
                "<tr><td>{}</td><td class=\"num\">{}</td><td><span class=\"badge {scls}\">{slabel}</span></td><td>{reason}</td></tr>\n",
                html_escape(&score.scorer),
                score.value,
            ));
        }
        out.push_str("</tbody>\n</table>\n");
    }

    if let Some(TargetOutput {
        trajectory: Some(trajectory),
        ..
    }) = &result.output
    {
        push_trajectory(out, trajectory);
    }

    out.push_str("</div>\n</details>\n");
}

/// Render an agent trajectory: one nested `<details>` per step.
fn push_trajectory(out: &mut String, trajectory: &Trajectory) {
    out.push_str("<h3>Trajectory</h3>\n");
    if trajectory.steps.is_empty() {
        out.push_str("<p class=\"muted\">no steps</p>\n");
        return;
    }
    out.push_str("<ol class=\"trajectory\">\n");
    for step in &trajectory.steps {
        out.push_str(&format!(
            "<li><span class=\"tool\">{}</span>\n",
            html_escape(&step.tool)
        ));
        out.push_str("<details class=\"payload\"><summary>input</summary>\n");
        out.push_str(&format!(
            "<pre>{}</pre></details>\n",
            html_escape(&pretty(&step.input))
        ));
        if let Some(output) = &step.output {
            out.push_str("<details class=\"payload\"><summary>output</summary>\n");
            out.push_str(&format!(
                "<pre>{}</pre></details>\n",
                html_escape(&pretty(output))
            ));
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ol>\n");
}

/// Render the baseline diff section.
fn push_baseline(out: &mut String, diff: &BaselineDiff) {
    out.push_str("<section class=\"baseline\">\n<h2>Baseline diff</h2>\n");
    out.push_str(&format!(
        "<p class=\"muted\">baseline {}/{} passed &rarr; current {}/{} passed</p>\n",
        diff.baseline_passed, diff.baseline_total, diff.current_passed, diff.current_total
    ));
    let gate = if diff.gate_failed() {
        format!(
            "<p class=\"gate fail\">baseline gate: FAIL ({} regressed, {} new failing)</p>\n",
            diff.regressions.len(),
            diff.new_failing.len()
        )
    } else {
        "<p class=\"gate pass\">baseline gate: OK — no regressions</p>\n".into()
    };
    out.push_str(&gate);

    push_regression_group(out, "Regressed", "fail", &diff.regressions);
    push_regression_group(out, "New failing", "fail", &diff.new_failing);
    push_id_group(out, "Fixed", "pass", &diff.fixed);
    push_id_group(out, "Removed", "muted", &diff.removed);
    out.push_str("</section>\n");
}

fn push_regression_group(
    out: &mut String,
    title: &str,
    cls: &str,
    cases: &[evalcore_core::CaseRegression],
) {
    if cases.is_empty() {
        return;
    }
    out.push_str(&format!("<h3 class=\"{cls}\">{title}</h3>\n<ul>\n"));
    for case in cases {
        out.push_str(&format!("<li><code>{}</code>", html_escape(&case.case_id)));
        if !case.reasons.is_empty() {
            out.push_str("<ul class=\"reasons\">\n");
            for reason in &case.reasons {
                out.push_str(&format!("<li>{}</li>\n", html_escape(reason)));
            }
            out.push_str("</ul>");
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
}

fn push_id_group(out: &mut String, title: &str, cls: &str, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    out.push_str(&format!("<h3 class=\"{cls}\">{title}</h3>\n<ul>\n"));
    for id in ids {
        out.push_str(&format!("<li><code>{}</code></li>\n", html_escape(id)));
    }
    out.push_str("</ul>\n");
}

/// Deterministic pretty JSON for a payload value (serde_json's `Map` is a
/// `BTreeMap` here — `preserve_order` is banned workspace-wide — so keys sort
/// stably).
fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Minimal, theme-aware stylesheet, inlined into every report. Light by
/// default, dark via `prefers-color-scheme`. No external resources.
const HTML_STYLE: &str = "\
:root{--bg:#ffffff;--fg:#1c1e21;--muted:#6b7280;--border:#e5e7eb;--panel:#f9fafb;\
--pass:#137333;--pass-bg:#e6f4ea;--fail:#c5221f;--fail-bg:#fce8e6;--code:#f3f4f6;}\
@media (prefers-color-scheme:dark){:root{--bg:#16181c;--fg:#e6e6e6;--muted:#9aa0a6;\
--border:#2c2f36;--panel:#1e2127;--pass:#81c995;--pass-bg:#1e3a28;--fail:#f28b82;\
--fail-bg:#3a1f1e;--code:#232733;}}\
*{box-sizing:border-box;}\
body{margin:0;background:var(--bg);color:var(--fg);\
font:14px/1.5 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;}\
main{max-width:960px;margin:0 auto;padding:24px 16px 64px;}\
h1{font-size:20px;margin:0 0 12px;}\
h2{font-size:16px;margin:28px 0 10px;border-bottom:1px solid var(--border);padding-bottom:4px;}\
h3{font-size:13px;margin:14px 0 6px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);}\
h3.pass{color:var(--pass);}h3.fail{color:var(--fail);}h3.muted{color:var(--muted);}\
.stats{display:flex;flex-wrap:wrap;gap:8px;}\
.stat{background:var(--panel);border:1px solid var(--border);border-radius:6px;padding:3px 10px;font-variant-numeric:tabular-nums;}\
.stat.pass{color:var(--pass);}.stat.fail{color:var(--fail);}\
.overall-pass{background:var(--pass-bg);color:var(--pass);border-color:transparent;font-weight:600;}\
.overall-fail{background:var(--fail-bg);color:var(--fail);border-color:transparent;font-weight:600;}\
table{width:100%;border-collapse:collapse;margin:4px 0;}\
th,td{text-align:left;padding:5px 8px;border-bottom:1px solid var(--border);vertical-align:top;}\
th{font-size:11px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);font-weight:600;}\
.num{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap;}\
.badge{display:inline-block;min-width:34px;text-align:center;border-radius:4px;padding:1px 6px;font-size:11px;font-weight:600;}\
.badge.pass{background:var(--pass-bg);color:var(--pass);}\
.badge.fail{background:var(--fail-bg);color:var(--fail);}\
.row{display:grid;grid-template-columns:70px 1fr 90px 80px;gap:8px;align-items:center;padding:6px 8px;}\
.row.head{color:var(--muted);font-size:11px;text-transform:uppercase;letter-spacing:.04em;border-bottom:1px solid var(--border);}\
.c-id{overflow-wrap:anywhere;}\
details.case{border-bottom:1px solid var(--border);}\
details.case>summary{cursor:pointer;list-style:none;}\
details.case>summary::-webkit-details-marker{display:none;}\
details.case[open]>summary{background:var(--panel);}\
.case-body{padding:6px 12px 14px;}\
pre{background:var(--code);border:1px solid var(--border);border-radius:6px;padding:8px 10px;\
overflow-x:auto;white-space:pre-wrap;overflow-wrap:anywhere;margin:4px 0;font-size:12.5px;}\
code{background:var(--code);border-radius:4px;padding:1px 5px;font-size:12.5px;}\
.error{color:var(--fail);font-weight:600;}\
.muted{color:var(--muted);}\
ol.trajectory{margin:4px 0;padding-left:20px;}\
ol.trajectory>li{margin:6px 0;}\
.tool{font-weight:600;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;}\
details.payload{margin:4px 0;}\
details.payload>summary{cursor:pointer;color:var(--muted);font-size:12px;}\
ul.reasons{color:var(--muted);}\
.gate.pass{color:var(--pass);font-weight:600;}\
.gate.fail{color:var(--fail);font-weight:600;}\
";

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// HTML-escape text destined for element content or an attribute value. Mirrors
/// `xml_escape` but emits `&#39;` for the apostrophe (universally valid in HTML,
/// unlike `&apos;`). Every user-derived string in the HTML report goes through
/// this, so a case output of `<script>alert(1)</script>` renders as inert text.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use evalcore_core::{
        CaseResult, GateResult, Score, TargetOutput, TokenUsage, TraceStep, Trajectory,
    };

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
                    context: None,
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
                    context: None,
                },
                CaseResult {
                    case_id: "boom".into(),
                    output: None,
                    // The engine stores errors UNPREFIXED; `target error: ` is
                    // added by the renderers/`failure_reasons()`, so the fixture
                    // must not double it.
                    error: Some("connection refused".into()),
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                },
            ],
            gates: Vec::new(),
        }
    }

    /// The fixture plus a passing and a failing gate, for the gates section.
    fn fixture_with_gates() -> RunSummary {
        let mut summary = fixture();
        summary.gates = vec![
            GateResult {
                gate: "pass_rate >= 0.3".into(),
                actual: 0.333_333_333_333_333_3,
                passed: true,
                reason: None,
            },
            GateResult {
                gate: "mean_score(contains) >= 0.8".into(),
                actual: 0.5,
                passed: false,
                reason: None,
            },
            // A gate that could not be measured carries a reason line, pinning
            // the indented reason rendering.
            GateResult {
                gate: "mean_score(judge) >= 0.7".into(),
                actual: 0.0,
                passed: false,
                reason: Some("no scores from scorer \"judge\"".into()),
            },
        ];
        summary
    }

    #[test]
    fn terminal_report_snapshot() {
        insta::assert_snapshot!(terminal(&fixture()));
    }

    #[test]
    fn terminal_report_with_gates_snapshot() {
        insta::assert_snapshot!(terminal(&fixture_with_gates()));
    }

    #[test]
    fn json_includes_gates_when_present() {
        let rendered = json(&fixture_with_gates()).unwrap();
        assert!(
            rendered.contains("\"gates\""),
            "gates array must ride along"
        );
        let parsed: RunSummary = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed.gates.len(), 3);
        assert_eq!(parsed.gates[0].gate, "pass_rate >= 0.3");
        assert!(parsed.gates[0].passed);
        assert!(!parsed.gates[1].passed);
        assert_eq!(
            parsed.gates[2].reason.as_deref(),
            Some("no scores from scorer \"judge\"")
        );
    }

    #[test]
    fn json_omits_gates_when_empty() {
        // Byte-level guarantee: a gate-free summary must not gain a "gates" key.
        let rendered = json(&fixture()).unwrap();
        assert!(
            !rendered.contains("\"gates\""),
            "empty gates must be omitted"
        );
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
                    context: None,
                },
                CaseResult {
                    case_id: "refund-2".into(),
                    output: None,
                    error: None,
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                },
                CaseResult {
                    case_id: "retired".into(),
                    output: None,
                    error: Some("was failing".into()),
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                },
            ],
            gates: Vec::new(),
        };
        let diff = evalcore_core::compare(&baseline_run, &fixture());
        insta::assert_snapshot!(baseline(&diff, "main"));
    }

    /// A trajectory-bearing case, for the HTML report's trajectory section.
    fn trajectory_case() -> CaseResult {
        CaseResult {
            case_id: "agent-1".into(),
            output: Some(TargetOutput {
                text: "Refunds take 30 days.".into(),
                latency_ms: 4400,
                tokens: Some(TokenUsage {
                    input: 200,
                    output: 68,
                }),
                trajectory: Some(Trajectory {
                    steps: vec![
                        TraceStep {
                            tool: "search_kb".into(),
                            input: serde_json::json!({"query": "refund policy"}),
                            output: Some(serde_json::json!("30 day window")),
                        },
                        TraceStep {
                            tool: "reply".into(),
                            input: serde_json::json!({"text": "Refunds take 30 days."}),
                            output: None,
                        },
                    ],
                }),
            }),
            error: None,
            scores: vec![Score {
                scorer: "trajectory".into(),
                value: 1.0,
                passed: true,
                reason: None,
            }],
            cost_usd: Some(0.000_19),
            context: None,
        }
    }

    /// A passing case carrying RAG context, for the HTML report's Context
    /// block. Passes and is absent from the baseline, so it lands in none of the
    /// baseline-diff groups (which track failures) — keeping those stable.
    fn context_case() -> CaseResult {
        CaseResult {
            case_id: "rag-1".into(),
            output: Some(TargetOutput {
                text: "Refunds are issued within 30 days.".into(),
                latency_ms: 18,
                tokens: Some(TokenUsage {
                    input: 90,
                    output: 12,
                }),
                trajectory: None,
            }),
            error: None,
            scores: vec![Score {
                scorer: "judge".into(),
                value: 1.0,
                passed: true,
                reason: None,
            }],
            cost_usd: Some(0.0003),
            context: Some(vec![
                "Refunds are processed within 30 days of the request.".into(),
                "Refunds require an order number & the original receipt.".into(),
            ]),
        }
    }

    /// The gated fixture plus a trajectory case and a context case: exercises
    /// every HTML section.
    fn fixture_full() -> RunSummary {
        let mut summary = fixture_with_gates();
        summary.results.push(trajectory_case());
        summary.results.push(context_case());
        summary
    }

    #[test]
    fn html_full_report_snapshot() {
        // Baseline: refund-1 was failing, refund-2 passed, and boom/agent-1
        // didn't exist. The current run fixes refund-1 (Fixed), regresses
        // refund-2 (Regressed), adds failing boom (New failing), and drops
        // `retired` (Removed) — every baseline-diff group is exercised.
        let baseline_run = RunSummary {
            results: vec![
                CaseResult {
                    case_id: "refund-1".into(),
                    output: None,
                    error: Some("was flaky".into()),
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                },
                CaseResult {
                    case_id: "refund-2".into(),
                    output: None,
                    error: None,
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                },
                CaseResult {
                    case_id: "retired".into(),
                    output: None,
                    error: Some("was failing".into()),
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                },
            ],
            gates: Vec::new(),
        };
        let summary = fixture_full();
        let diff = evalcore_core::compare(&baseline_run, &summary);
        insta::assert_snapshot!(html(&summary, Some(&diff)));
    }

    #[test]
    fn html_minimal_report_snapshot() {
        // No gates, no trajectory, no baseline diff.
        insta::assert_snapshot!(html(&fixture(), None));
    }

    #[test]
    fn html_escapes_user_derived_content() {
        // A hostile case id, output, and scorer reason must all render inert.
        let summary = RunSummary {
            results: vec![CaseResult {
                case_id: "<script>alert('id')</script>".into(),
                output: Some(TargetOutput {
                    text: "<script>alert(\"xss\")</script> & \"done\"".into(),
                    latency_ms: 5,
                    tokens: None,
                    trajectory: None,
                }),
                error: None,
                scores: vec![Score {
                    scorer: "contains".into(),
                    value: 0.0,
                    passed: false,
                    reason: Some("wanted <b>x</b> & 'y' but got \"z\"".into()),
                }],
                cost_usd: None,
                context: None,
            }],
            gates: Vec::new(),
        };
        let rendered = html(&summary, None);

        assert!(
            rendered.contains("&lt;script&gt;"),
            "angle brackets escaped"
        );
        assert!(rendered.contains("&amp;"), "ampersand escaped");
        assert!(rendered.contains("&quot;"), "double quote escaped");
        assert!(rendered.contains("&#39;"), "apostrophe escaped");
        assert!(
            !rendered.contains("<script>"),
            "no live script tag survives"
        );
    }

    #[test]
    fn html_escapes_hostile_context_chunks() {
        // A malicious retrieved chunk must render inert in the Context block.
        let summary = RunSummary {
            results: vec![CaseResult {
                case_id: "rag".into(),
                output: Some(TargetOutput {
                    text: "answer".into(),
                    latency_ms: 5,
                    tokens: None,
                    trajectory: None,
                }),
                error: None,
                scores: vec![],
                cost_usd: None,
                context: Some(vec!["<script>alert(1)</script> & \"quoted\" 'chunk'".into()]),
            }],
            gates: Vec::new(),
        };
        let rendered = html(&summary, None);

        assert!(
            rendered.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "context script tag escaped"
        );
        assert!(rendered.contains("&amp;"), "ampersand escaped in context");
        assert!(
            rendered.contains("&quot;"),
            "double quote escaped in context"
        );
        assert!(rendered.contains("&#39;"), "apostrophe escaped in context");
        assert!(
            !rendered.contains("<script>"),
            "no live script tag survives from a context chunk"
        );
    }

    #[test]
    fn html_is_deterministic() {
        // Pins the no-clock, no-random rule: identical inputs, identical bytes.
        let summary = fixture_full();
        let baseline_run = RunSummary {
            results: vec![CaseResult {
                case_id: "refund-2".into(),
                output: None,
                error: None,
                scores: vec![],
                cost_usd: None,
                context: None,
            }],
            gates: Vec::new(),
        };
        let diff = evalcore_core::compare(&baseline_run, &summary);
        assert_eq!(html(&summary, Some(&diff)), html(&summary, Some(&diff)));
    }

    #[test]
    fn json_report_round_trips() {
        let rendered = json(&fixture()).unwrap();
        let parsed: RunSummary = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed.total(), 3);
        assert_eq!(parsed.failed(), 2);
    }
}
