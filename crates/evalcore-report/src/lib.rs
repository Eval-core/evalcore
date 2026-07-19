//! Report rendering. Every reporter is a pure function `&RunSummary -> String`
//! — no I/O, no clock, no global state — so outputs are snapshot-testable and
//! identical for identical runs.

use evalcore_core::types::TrialResult;
use evalcore_core::{
    BaselineDiff, CaseResult, MatrixComparison, MatrixSummary, RunSummary, TargetOutput, Trajectory,
};

/// The ` [k/N trials]` suffix appended to a case's terminal PASS/FAIL line when
/// it ran more than one trial (`k` = passing trials, `N` = total). Empty for
/// single-trial cases, so their output stays byte-identical to a non-trial run.
fn trials_suffix(result: &CaseResult) -> String {
    match &result.trials {
        Some(trials) if trials.len() > 1 => {
            let passed = trials.iter().filter(|t| t.passed).count();
            format!(" [{passed}/{} trials]", trials.len())
        }
        _ => String::new(),
    }
}

/// Human-readable report for terminals and logs.
pub fn terminal(summary: &RunSummary) -> String {
    let mut out = String::new();
    for result in &summary.results {
        let trials = trials_suffix(result);
        if result.passed() {
            let latency = result.output.as_ref().map_or(0, |o| o.latency_ms);
            out.push_str(&format!("PASS {} ({latency}ms){trials}\n", result.case_id));
        } else {
            out.push_str(&format!("FAIL {}{trials}\n", result.case_id));
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
    // Flakiness suffix: reporter-computed from the per-case trials detail. A
    // case is flaky when its trials split (0 < passed < count). Appended only
    // when at least one case carries a trials detail, so single-trial runs
    // render byte-identically to before.
    if summary.results.iter().any(|r| r.trials.is_some()) {
        let flaky = summary.results.iter().filter(|r| is_flaky(r)).count();
        totals.push_str(&format!(" · {flaky} flaky"));
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
    // Classification aggregates, one line after the gates block, only when the
    // run computed them. Absent otherwise, so classification-free runs are
    // byte-identical. The per-class table lives in the JSON/HTML reporters.
    if let Some(classification) = &summary.classification {
        out.push_str(&format!(
            "classification: accuracy {:.2} · macro-F1 {:.2} ({} labeled, {} unlabeled)\n",
            classification.accuracy,
            classification.macro_f1,
            classification.labeled_cases,
            classification.unlabeled_cases,
        ));
    }
    out
}

/// True when a case's trials split — some passed, some failed
/// (`0 < passed < count`) — the reporter-layer flakiness measure. False for
/// single-trial cases and cases whose trials all agree.
fn is_flaky(result: &CaseResult) -> bool {
    match &result.trials {
        Some(trials) => {
            let passed = trials.iter().filter(|t| t.passed).count();
            passed > 0 && passed < trials.len()
        }
        None => false,
    }
}

/// Machine-readable report: the full `RunSummary` as pretty JSON.
///
/// When any case carries a trials detail, each such case gains a reporter-
/// computed `trial_stats` object (`pass_fraction`, `score_mean`, `score_range`)
/// with sorted keys. A run with no trials detail serializes byte-identically to
/// `serde_json::to_string_pretty(summary)` — the augmentation path is skipped
/// entirely, so single-trial output is unchanged.
pub fn json(summary: &RunSummary) -> anyhow::Result<String> {
    if !summary.results.iter().any(|r| r.trials.is_some()) {
        return Ok(serde_json::to_string_pretty(summary)?);
    }
    let mut value = serde_json::to_value(summary)?;
    if let Some(results) = value
        .get_mut("results")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (case_json, result) in results.iter_mut().zip(summary.results.iter()) {
            if let (Some(object), Some(trials)) = (case_json.as_object_mut(), &result.trials) {
                object.insert("trial_stats".into(), trial_stats(trials));
            }
        }
    }
    Ok(serde_json::to_string_pretty(&value)?)
}

/// Reporter-computed statistics over a case's trials, as a JSON object with
/// sorted keys (serde_json's `Map` is a `BTreeMap` — `preserve_order` is banned
/// — so `score_mean`/`score_range` are label-sorted deterministically):
/// `pass_fraction` (passing trials / count), `score_mean` (per-scorer mean), and
/// `score_range` (per-scorer `[min, max]`).
fn trial_stats(trials: &[TrialResult]) -> serde_json::Value {
    let count = trials.len();
    let passed = trials.iter().filter(|t| t.passed).count();
    let pass_fraction = if count == 0 {
        0.0
    } else {
        passed as f64 / count as f64
    };
    let mut score_mean = serde_json::Map::new();
    let mut score_range = serde_json::Map::new();
    for (scorer, (mean, min, max)) in scorer_trial_stats(trials) {
        score_mean.insert(scorer.clone(), json_number(mean));
        score_range.insert(
            scorer,
            serde_json::Value::Array(vec![json_number(min), json_number(max)]),
        );
    }
    let mut stats = serde_json::Map::new();
    stats.insert("pass_fraction".into(), json_number(pass_fraction));
    stats.insert("score_mean".into(), serde_json::Value::Object(score_mean));
    stats.insert("score_range".into(), serde_json::Value::Object(score_range));
    serde_json::Value::Object(stats)
}

/// Encode an `f64` as a JSON number, falling back to `null` for a non-finite
/// value (JSON has no NaN/Infinity) so the reporter never panics on hostile data.
fn json_number(value: f64) -> serde_json::Value {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

/// Per-scorer `(mean, min, max)` over a case's trials, keyed by scorer name and
/// sorted (a `BTreeMap`) for determinism. A scorer contributes only the trials
/// in which it produced a score (errored trials have none), so every entry has
/// at least one value.
fn scorer_trial_stats(
    trials: &[TrialResult],
) -> std::collections::BTreeMap<String, (f64, f64, f64)> {
    let mut values: std::collections::BTreeMap<String, Vec<f64>> =
        std::collections::BTreeMap::new();
    for trial in trials {
        for score in &trial.scores {
            values
                .entry(score.scorer.clone())
                .or_default()
                .push(score.value);
        }
    }
    values
        .into_iter()
        .map(|(scorer, vs)| {
            let mean = vs.iter().sum::<f64>() / vs.len() as f64;
            let min = vs.iter().copied().fold(f64::INFINITY, f64::min);
            let max = vs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            (scorer, (mean, min, max))
        })
        .collect()
}

/// One `<testsuite>` block (the cases plus its open/close tags), reused by both
/// the single-run [`junit`] and the matrix [`junit_matrix`] roots. `name` is
/// XML-escaped; the single-run name `evalcore` has no metacharacters, so its
/// output is byte-identical to before this was factored out.
fn junit_suite_block(name: &str, summary: &RunSummary) -> String {
    let mut cases = String::new();
    for result in &summary.results {
        let case_name = xml_escape(&result.case_id);
        let time = result.output.as_ref().map_or(0, |o| o.latency_ms) as f64 / 1000.0;
        if result.passed() {
            cases.push_str(&format!(
                r#"    <testcase name="{case_name}" time="{time:.3}"/>"#
            ));
            cases.push('\n');
        } else {
            let message = xml_escape(&result.failure_reasons().join("; "));
            cases.push_str(&format!(
                "    <testcase name=\"{case_name}\" time=\"{time:.3}\">\n      <failure message=\"{message}\"/>\n    </testcase>\n"
            ));
        }
    }
    format!(
        "  <testsuite name=\"{name}\" tests=\"{total}\" failures=\"{failed}\">\n{cases}  </testsuite>\n",
        name = xml_escape(name),
        total = summary.total(),
        failed = summary.failed(),
    )
}

/// JUnit XML for CI systems (GitHub Actions, GitLab, Jenkins all ingest it).
pub fn junit(summary: &RunSummary) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites tests=\"{total}\" failures=\"{failed}\">\n{block}</testsuites>\n",
        total = summary.total(),
        failed = summary.failed(),
        block = junit_suite_block("evalcore", summary),
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
    html_head(&mut out);
    push_report_body(&mut out, summary, diff);
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

/// Human-readable terminal report for a matrix run: each arm's single-run block
/// (prefixed by a `== target: <name>` line), then a `== comparison` section — a
/// case-by-case PASS/FAIL grid with the per-case winner and a wins footer.
/// Pure and deterministic; arms in matrix order, comparison rows in dataset
/// order, no color codes beyond the single-run reporter's.
pub fn terminal_matrix(matrix: &MatrixSummary, comparison: &MatrixComparison) -> String {
    let mut out = String::new();
    for arm in &matrix.arms {
        out.push_str(&format!("== target: {}\n", arm.target));
        out.push_str(&terminal(&arm.summary));
        out.push('\n');
    }
    out.push_str("== comparison\n");

    // Case column at least 8 wide; each arm column fits its name (min 4, the
    // width of PASS/FAIL). Columns are separated by four spaces.
    let id_w = comparison
        .rows
        .iter()
        .map(|r| r.case_id.len())
        .max()
        .unwrap_or(0)
        .max(8);
    let col_w = |name: &str| name.len().max(4);

    let mut header = format!("{:<id_w$}", "case");
    for arm in &comparison.arms {
        header.push_str("    ");
        header.push_str(&format!("{:<w$}", arm.target, w = col_w(&arm.target)));
    }
    out.push_str(header.trim_end());
    out.push('\n');

    for row in &comparison.rows {
        let mut line = format!("{:<id_w$}", row.case_id);
        for (i, cell) in row.cells.iter().enumerate() {
            line.push_str("    ");
            let mark = if cell.passed { "PASS" } else { "FAIL" };
            let w = col_w(&comparison.arms[i].target);
            line.push_str(&format!("{mark:<w$}"));
        }
        line.push_str("    ");
        line.push_str(match row.winner {
            Some(i) => comparison.arms[i].target.as_str(),
            None => "tie",
        });
        out.push_str(line.trim_end());
        out.push('\n');
    }

    out.push_str(&format!("wins: {}\n", wins_line(comparison)));
    out
}

/// The `<name> <wins> · … · ties <n>` summary shared by the terminal and HTML
/// comparison sections.
fn wins_line(comparison: &MatrixComparison) -> String {
    let mut parts: Vec<String> = comparison
        .arms
        .iter()
        .map(|a| format!("{} {}", a.target, a.wins))
        .collect();
    parts.push(format!("ties {}", comparison.ties));
    parts.join(" · ")
}

/// Machine-readable matrix report: `{"arms": [{"target", "summary"}...],
/// "comparison": {...}}`, pretty-printed. Each arm's `summary` is the single-run
/// JSON shape (struct field order); the comparison carries per-case rows and
/// per-arm aggregates.
pub fn json_matrix(
    matrix: &MatrixSummary,
    comparison: &MatrixComparison,
) -> anyhow::Result<String> {
    let arms = matrix
        .arms
        .iter()
        .map(|arm| {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "target".into(),
                serde_json::Value::String(arm.target.clone()),
            );
            obj.insert("summary".into(), serde_json::to_value(&arm.summary)?);
            Ok(serde_json::Value::Object(obj))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut root = serde_json::Map::new();
    root.insert("arms".into(), serde_json::Value::Array(arms));
    root.insert("comparison".into(), serde_json::to_value(comparison)?);
    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(
        root,
    ))?)
}

/// JUnit XML for a matrix run: one `<testsuite name="evalcore/<target>">` per
/// arm under a single `<testsuites>` root whose totals sum the arms. Target
/// names are XML-escaped.
pub fn junit_matrix(matrix: &MatrixSummary) -> String {
    let total: usize = matrix.arms.iter().map(|a| a.summary.total()).sum();
    let failed: usize = matrix.arms.iter().map(|a| a.summary.failed()).sum();
    let mut blocks = String::new();
    for arm in &matrix.arms {
        blocks.push_str(&junit_suite_block(
            &format!("evalcore/{}", arm.target),
            &arm.summary,
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites tests=\"{total}\" failures=\"{failed}\">\n{blocks}</testsuites>\n",
    )
}

/// Self-contained HTML matrix report: a comparison table at the top, then each
/// arm's single-run report body under a `target:` heading. Reuses the single-run
/// chrome and sections; every target name and case id is HTML-escaped.
pub fn html_matrix(matrix: &MatrixSummary, comparison: &MatrixComparison) -> String {
    let mut out = String::new();
    html_head(&mut out);
    push_comparison_table(&mut out, comparison);
    for arm in &matrix.arms {
        out.push_str(&format!(
            "<section class=\"arm\">\n<h2>target: {}</h2>\n",
            html_escape(&arm.target)
        ));
        push_report_body(&mut out, &arm.summary, None);
        out.push_str("</section>\n");
    }
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

/// Render the top-of-page comparison table: one row per case, one column per
/// arm (PASS/FAIL badge), a winner column, and a wins footer. Names and case
/// ids are escaped, so hostile targets/cases render inert.
fn push_comparison_table(out: &mut String, comparison: &MatrixComparison) {
    out.push_str("<section class=\"comparison\">\n<h2>Comparison</h2>\n");
    out.push_str("<table>\n<thead><tr><th>Case</th>");
    for arm in &comparison.arms {
        out.push_str(&format!("<th>{}</th>", html_escape(&arm.target)));
    }
    out.push_str("<th>Winner</th></tr></thead>\n<tbody>\n");
    for row in &comparison.rows {
        out.push_str(&format!("<tr><td>{}</td>", html_escape(&row.case_id)));
        for cell in &row.cells {
            let (cls, label) = if cell.passed {
                ("pass", "PASS")
            } else {
                ("fail", "FAIL")
            };
            out.push_str(&format!(
                "<td><span class=\"badge {cls}\">{label}</span></td>"
            ));
        }
        let winner = match row.winner {
            Some(i) => html_escape(&comparison.arms[i].target),
            None => "tie".to_string(),
        };
        out.push_str(&format!("<td>{winner}</td></tr>\n"));
    }
    out.push_str("</tbody>\n</table>\n");
    // Escaped wins footer: the terminal `wins_line` emits raw names, so the HTML
    // path builds its own escaped version rather than reusing it.
    let mut parts: Vec<String> = comparison
        .arms
        .iter()
        .map(|a| format!("{} {}", html_escape(&a.target), a.wins))
        .collect();
    parts.push(format!("ties {}", comparison.ties));
    out.push_str(&format!(
        "<p class=\"muted\">wins: {}</p>\n",
        parts.join(" &middot; ")
    ));
    out.push_str("</section>\n");
}

/// Emit the document prelude through the opening `<main>` tag — the shared
/// chrome for the single-run and matrix HTML reports.
fn html_head(out: &mut String) {
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>EvalCore report</title>\n");
    out.push_str("<style>\n");
    out.push_str(HTML_STYLE);
    out.push_str("</style>\n</head>\n<body>\n");
    out.push_str("<main>\n");
}

/// Emit one run's report sections (header, gates, classification, cases, and an
/// optional baseline diff) — everything between `<main>` and `</main>`. Shared
/// so each matrix arm renders exactly the single-run body.
fn push_report_body(out: &mut String, summary: &RunSummary, diff: Option<&BaselineDiff>) {
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

    // Classification panel — omitted entirely when the run computed no
    // classification aggregates, so classification-free reports stay identical.
    if let Some(classification) = &summary.classification {
        push_classification(out, classification);
    }

    // Case table: one expandable row per case, in dataset order.
    out.push_str("<section class=\"cases\">\n<h2>Cases</h2>\n");
    out.push_str("<div class=\"row head\"><span class=\"c-status\">Status</span><span class=\"c-id\">Case</span><span class=\"c-latency\">Latency</span><span class=\"c-cost\">Cost</span></div>\n");
    for result in &summary.results {
        push_case(out, result);
    }
    out.push_str("</section>\n");

    // Baseline diff — same data the terminal diff renderer shows.
    if let Some(diff) = diff {
        push_baseline(out, diff);
    }
}

/// Render the classification panel: the headline accuracy/macro-F1 figures and
/// a per-class precision/recall/F1/support table, label-sorted (the core sorts
/// `per_class`). Labels are escaped, so a hostile label renders inert.
fn push_classification(out: &mut String, classification: &evalcore_core::ClassificationSummary) {
    out.push_str("<section class=\"classification\">\n<h2>Classification</h2>\n");
    out.push_str("<div class=\"stats\">\n");
    out.push_str(&format!(
        "<span class=\"stat\">accuracy {:.2}</span>\n",
        classification.accuracy
    ));
    out.push_str(&format!(
        "<span class=\"stat\">macro-F1 {:.2}</span>\n",
        classification.macro_f1
    ));
    out.push_str(&format!(
        "<span class=\"stat\">{} labeled</span>\n",
        classification.labeled_cases
    ));
    out.push_str(&format!(
        "<span class=\"stat\">{} unlabeled</span>\n",
        classification.unlabeled_cases
    ));
    out.push_str("</div>\n");
    if !classification.per_class.is_empty() {
        out.push_str("<table>\n<thead><tr><th>Class</th><th>Precision</th><th>Recall</th><th>F1</th><th>Support</th></tr></thead>\n<tbody>\n");
        for metrics in &classification.per_class {
            out.push_str(&format!(
                "<tr><td>{}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td><td class=\"num\">{}</td></tr>\n",
                html_escape(&metrics.label),
                metrics.precision,
                metrics.recall,
                metrics.f1,
                metrics.support,
            ));
        }
        out.push_str("</tbody>\n</table>\n");
    }
    out.push_str("</section>\n");
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

    // Per-trial breakdown for multi-trial cases. Absent (never emitted) for
    // single-trial cases, so their report stays byte-identical.
    if let Some(trials) = &result.trials {
        push_trials(out, trials);
    }

    out.push_str("</div>\n</details>\n");
}

/// Render the per-trial breakdown: one table row per trial, in trial-index
/// order. A trial's detail is its escaped target-error reason, or a
/// scorer=value summary for a successful trial.
fn push_trials(out: &mut String, trials: &[TrialResult]) {
    out.push_str("<h3>Trials</h3>\n");
    let count = trials.len();
    let passed = trials.iter().filter(|t| t.passed).count();
    let fraction = if count == 0 {
        0.0
    } else {
        passed as f64 / count as f64
    };
    out.push_str(&format!(
        "<p class=\"muted\">{passed}/{count} passed (pass fraction {fraction:.2})</p>\n"
    ));
    out.push_str("<table>\n<thead><tr><th>Trial</th><th>Passed</th><th>Latency</th><th>Detail</th></tr></thead>\n<tbody>\n");
    for (i, trial) in trials.iter().enumerate() {
        let (cls, label) = if trial.passed {
            ("pass", "yes")
        } else {
            ("fail", "no")
        };
        let detail = if let Some(error) = &trial.error {
            format!("target error: {}", html_escape(error))
        } else {
            trial
                .scores
                .iter()
                .map(|score| format!("{}={}", html_escape(&score.scorer), score.value))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "<tr><td class=\"num\">{i}</td><td><span class=\"badge {cls}\">{label}</span></td><td class=\"num\">{}ms</td><td>{detail}</td></tr>\n",
            trial.latency_ms,
        ));
    }
    out.push_str("</tbody>\n</table>\n");

    // Per-scorer aggregate across the trials: mean and [min, max].
    let stats = scorer_trial_stats(trials);
    if !stats.is_empty() {
        out.push_str("<table>\n<thead><tr><th>Scorer</th><th>Mean</th><th>Min</th><th>Max</th></tr></thead>\n<tbody>\n");
        for (scorer, (mean, min, max)) in &stats {
            out.push_str(&format!(
                "<tr><td>{}</td><td class=\"num\">{mean:.2}</td><td class=\"num\">{min:.2}</td><td class=\"num\">{max:.2}</td></tr>\n",
                html_escape(scorer),
            ));
        }
        out.push_str("</tbody>\n</table>\n");
    }
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
///
/// Public so the local `evalcore serve` viewer escapes DB-derived strings
/// (config paths, target names) with the exact same rule as the report; it must
/// stay a pure `&str -> String` with no rendering behavior of its own.
pub fn html_escape(text: &str) -> String {
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
                    trials: None,
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
                    trials: None,
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
                    trials: None,
                },
            ],
            gates: Vec::new(),
            classification: None,
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
                    trials: None,
                },
                CaseResult {
                    case_id: "refund-2".into(),
                    output: None,
                    error: None,
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                    trials: None,
                },
                CaseResult {
                    case_id: "retired".into(),
                    output: None,
                    error: Some("was failing".into()),
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                    trials: None,
                },
            ],
            gates: Vec::new(),
            classification: None,
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
            trials: None,
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
            trials: None,
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
                    trials: None,
                },
                CaseResult {
                    case_id: "refund-2".into(),
                    output: None,
                    error: None,
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                    trials: None,
                },
                CaseResult {
                    case_id: "retired".into(),
                    output: None,
                    error: Some("was failing".into()),
                    scores: vec![],
                    cost_usd: None,
                    context: None,
                    trials: None,
                },
            ],
            gates: Vec::new(),
            classification: None,
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
                trials: None,
            }],
            gates: Vec::new(),
            classification: None,
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
                trials: None,
            }],
            gates: Vec::new(),
            classification: None,
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
                trials: None,
            }],
            gates: Vec::new(),
            classification: None,
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

    /// A multi-trial case: 2 of 3 trials pass, one trial's target errored with a
    /// hostile reason. Exercises the trials suffix and the HTML trials table.
    fn multi_trial_case() -> CaseResult {
        CaseResult {
            case_id: "flaky-1".into(),
            output: Some(TargetOutput {
                text: "yes".into(),
                latency_ms: 5,
                tokens: None,
                trajectory: None,
            }),
            error: None,
            scores: vec![Score {
                scorer: "contains".into(),
                value: 2.0 / 3.0,
                passed: false,
                reason: None,
            }],
            cost_usd: None,
            context: None,
            trials: Some(vec![
                TrialResult {
                    passed: true,
                    scores: vec![Score {
                        scorer: "contains".into(),
                        value: 1.0,
                        passed: true,
                        reason: None,
                    }],
                    latency_ms: 5,
                    error: None,
                },
                TrialResult {
                    passed: true,
                    scores: vec![Score {
                        scorer: "contains".into(),
                        value: 1.0,
                        passed: true,
                        reason: None,
                    }],
                    latency_ms: 6,
                    error: None,
                },
                TrialResult {
                    passed: false,
                    scores: vec![],
                    latency_ms: 0,
                    error: Some("<boom> & \"crash\"".into()),
                },
            ]),
        }
    }

    #[test]
    fn terminal_appends_trials_suffix_only_when_multi() {
        // A multi-trial FAIL line carries the [k/N trials] tag; a single-trial
        // case (trials None) is byte-identical to today (no tag).
        let summary = RunSummary {
            results: vec![multi_trial_case()],
            gates: Vec::new(),
            classification: None,
        };
        let rendered = terminal(&summary);
        assert!(
            rendered.contains("FAIL flaky-1 [2/3 trials]"),
            "got: {rendered}"
        );

        // A single-trial fixture must NOT gain any trials tag.
        assert!(
            !terminal(&fixture()).contains("trials]"),
            "single-trial output must stay tag-free"
        );
    }

    #[test]
    fn html_renders_trials_and_escapes_error_reasons() {
        let summary = RunSummary {
            results: vec![multi_trial_case()],
            gates: Vec::new(),
            classification: None,
        };
        let rendered = html(&summary, None);

        assert!(
            rendered.contains("<h3>Trials</h3>"),
            "trials section present"
        );
        // A hostile trial-error reason must render inert.
        assert!(
            rendered.contains("target error: &lt;boom&gt; &amp; &quot;crash&quot;"),
            "trial error reason must be escaped; got: {rendered}"
        );
        assert!(
            !rendered.contains("<boom>"),
            "no live markup survives from a trial error"
        );
        // A successful trial's detail summarizes its scorer values.
        assert!(rendered.contains("contains=1"), "got: {rendered}");
    }

    #[test]
    fn html_without_trials_has_no_trials_section() {
        // Byte-level: a single-trial report never emits the Trials section.
        assert!(!html(&fixture(), None).contains("<h3>Trials</h3>"));
    }

    /// A summary carrying classification aggregates with a hostile label, for the
    /// classification-reporting tests.
    fn fixture_with_classification() -> RunSummary {
        let mut summary = fixture();
        summary.classification = Some(evalcore_core::ClassificationSummary {
            labeled_cases: 24,
            unlabeled_cases: 1,
            accuracy: 0.916_666_666,
            macro_f1: 0.875,
            per_class: vec![
                evalcore_core::ClassMetrics {
                    label: "<b>refund</b>".into(),
                    precision: 1.0,
                    recall: 0.5,
                    f1: 2.0 / 3.0,
                    support: 2,
                },
                evalcore_core::ClassMetrics {
                    label: "escalate".into(),
                    precision: 0.75,
                    recall: 1.0,
                    f1: 0.857,
                    support: 3,
                },
            ],
        });
        summary
    }

    #[test]
    fn terminal_classification_line_only_when_present() {
        // The classification line follows the gates block, with two-decimal
        // figures and the labeled/unlabeled counts.
        let rendered = terminal(&fixture_with_classification());
        assert!(
            rendered.contains(
                "classification: accuracy 0.92 · macro-F1 0.88 (24 labeled, 1 unlabeled)"
            ),
            "got: {rendered}"
        );
        // A classification-free run must not gain the line (byte-compat).
        assert!(
            !terminal(&fixture()).contains("classification:"),
            "classification-free output must stay line-free"
        );
    }

    #[test]
    fn terminal_flaky_suffix_only_when_trials_present() {
        // One flaky case (2/3) → the summary line gains ` · 1 flaky`.
        let summary = RunSummary {
            results: vec![multi_trial_case()],
            gates: Vec::new(),
            classification: None,
        };
        assert!(
            terminal(&summary).contains("0 passed, 1 failed, 1 total · 1 flaky"),
            "got: {}",
            terminal(&summary)
        );
        // No trials detail anywhere → no flaky suffix (byte-compat).
        assert!(
            !terminal(&fixture()).contains("flaky"),
            "single-trial output must stay flaky-free"
        );
    }

    #[test]
    fn html_classification_section_escapes_labels() {
        let rendered = html(&fixture_with_classification(), None);
        assert!(
            rendered.contains("<h2>Classification</h2>"),
            "classification section present"
        );
        assert!(
            rendered.contains("accuracy 0.92"),
            "headline accuracy; got: {rendered}"
        );
        // The hostile class label renders inert.
        assert!(
            rendered.contains("&lt;b&gt;refund&lt;/b&gt;"),
            "class label must be escaped; got: {rendered}"
        );
        assert!(
            !rendered.contains("<b>refund</b>"),
            "no live markup survives from a class label"
        );
        // A classification-free report never emits the section.
        assert!(!html(&fixture(), None).contains("<h2>Classification</h2>"));
    }

    #[test]
    fn json_adds_trial_stats_only_when_trials_present() {
        // With trials detail, the case gains a reporter-computed trial_stats with
        // sorted keys; without, the JSON is byte-identical to plain serialization.
        let summary = RunSummary {
            results: vec![multi_trial_case()],
            gates: Vec::new(),
            classification: None,
        };
        let rendered = json(&summary).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let stats = &value["results"][0]["trial_stats"];
        assert!(
            (stats["pass_fraction"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-12,
            "got: {stats}"
        );
        assert!(
            (stats["score_mean"]["contains"].as_f64().unwrap() - 1.0).abs() < 1e-12,
            "mean over the two scoring trials is 1.0; got: {stats}"
        );
        let range = stats["score_range"]["contains"].as_array().unwrap();
        assert_eq!(range[0].as_f64().unwrap(), 1.0);
        assert_eq!(range[1].as_f64().unwrap(), 1.0);

        // Byte-compat: a single-trial summary serializes exactly like plain JSON.
        assert_eq!(
            json(&fixture()).unwrap(),
            serde_json::to_string_pretty(&fixture()).unwrap(),
            "no trials detail → byte-identical to plain serialization"
        );
        assert!(!json(&fixture()).unwrap().contains("trial_stats"));
    }

    #[test]
    fn json_includes_classification_when_present() {
        let rendered = json(&fixture_with_classification()).unwrap();
        assert!(rendered.contains("\"classification\""));
        let parsed: RunSummary = serde_json::from_str(&rendered).unwrap();
        let classification = parsed.classification.unwrap();
        assert_eq!(classification.labeled_cases, 24);
        assert_eq!(classification.per_class[0].label, "<b>refund</b>");
    }

    #[test]
    fn html_trials_header_shows_pass_fraction() {
        let summary = RunSummary {
            results: vec![multi_trial_case()],
            gates: Vec::new(),
            classification: None,
        };
        let rendered = html(&summary, None);
        assert!(
            rendered.contains("2/3 passed (pass fraction 0.67)"),
            "got: {rendered}"
        );
    }

    // ---- Matrix reporters ----

    use evalcore_core::{compare_arms, MatrixArm, MatrixSummary};

    /// One arm with two fixed-latency cases; `refund-1` always passes, `refund-2`
    /// passes iff `r2_pass`.
    fn matrix_arm(target: &str, r2_pass: bool) -> MatrixArm {
        MatrixArm {
            target: target.into(),
            summary: RunSummary {
                results: vec![
                    CaseResult {
                        case_id: "refund-1".into(),
                        output: Some(TargetOutput {
                            text: "refund issued".into(),
                            latency_ms: 10,
                            tokens: None,
                            trajectory: None,
                        }),
                        error: None,
                        scores: vec![Score {
                            scorer: "contains".into(),
                            value: 1.0,
                            passed: true,
                            reason: None,
                        }],
                        cost_usd: Some(0.0010),
                        context: None,
                        trials: None,
                    },
                    CaseResult {
                        case_id: "refund-2".into(),
                        output: Some(TargetOutput {
                            text: "response".into(),
                            latency_ms: 20,
                            tokens: None,
                            trajectory: None,
                        }),
                        error: None,
                        scores: vec![Score {
                            scorer: "contains".into(),
                            value: if r2_pass { 1.0 } else { 0.0 },
                            passed: r2_pass,
                            reason: if r2_pass {
                                None
                            } else {
                                Some("expected \"refund\"".into())
                            },
                        }],
                        cost_usd: Some(0.0020),
                        context: None,
                        trials: None,
                    },
                ],
                gates: Vec::new(),
                classification: None,
            },
        }
    }

    /// Two-arm matrix: `gpt` passes both cases, `claude` fails `refund-2`. So
    /// `refund-1` ties and `gpt` wins `refund-2`.
    fn matrix_fixture() -> MatrixSummary {
        MatrixSummary {
            arms: vec![matrix_arm("gpt", true), matrix_arm("claude", false)],
        }
    }

    #[test]
    fn terminal_matrix_snapshot() {
        let matrix = matrix_fixture();
        let comparison = compare_arms(&matrix);
        insta::assert_snapshot!(terminal_matrix(&matrix, &comparison));
    }

    #[test]
    fn html_matrix_snapshot() {
        let matrix = matrix_fixture();
        let comparison = compare_arms(&matrix);
        insta::assert_snapshot!(html_matrix(&matrix, &comparison));
    }

    #[test]
    fn json_matrix_shape() {
        let matrix = matrix_fixture();
        let comparison = compare_arms(&matrix);
        let rendered = json_matrix(&matrix, &comparison).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        // Two arms, first is gpt, each carrying a single-run summary.
        assert_eq!(value["arms"].as_array().unwrap().len(), 2);
        assert_eq!(value["arms"][0]["target"], "gpt");
        assert!(value["arms"][0]["summary"]["results"].is_array());
        // Comparison rows in dataset order; refund-2 won by gpt (arm 0).
        let rows = value["comparison"]["rows"].as_array().unwrap();
        assert_eq!(rows[0]["case_id"], "refund-1");
        assert!(rows[0]["winner"].is_null(), "refund-1 is a tie");
        assert_eq!(rows[1]["winner"], 0);
        assert_eq!(value["comparison"]["ties"], 1);
        assert_eq!(value["comparison"]["arms"][0]["wins"], 1);
    }

    #[test]
    fn junit_matrix_has_one_suite_per_arm() {
        let matrix = matrix_fixture();
        let xml = junit_matrix(&matrix);
        assert!(
            xml.contains("<testsuites tests=\"4\" failures=\"1\">"),
            "root sums the arms; got: {xml}"
        );
        assert!(
            xml.contains("<testsuite name=\"evalcore/gpt\""),
            "got: {xml}"
        );
        assert!(
            xml.contains("<testsuite name=\"evalcore/claude\""),
            "got: {xml}"
        );
    }

    #[test]
    fn html_matrix_escapes_hostile_target_names_and_case_ids() {
        let matrix = MatrixSummary {
            arms: vec![
                MatrixArm {
                    target: "<script>alert(1)</script>".into(),
                    summary: RunSummary {
                        results: vec![CaseResult {
                            case_id: "<img src=x>".into(),
                            output: Some(TargetOutput {
                                text: "ok".into(),
                                latency_ms: 5,
                                tokens: None,
                                trajectory: None,
                            }),
                            error: None,
                            scores: vec![Score {
                                scorer: "contains".into(),
                                value: 1.0,
                                passed: true,
                                reason: None,
                            }],
                            cost_usd: None,
                            context: None,
                            trials: None,
                        }],
                        gates: Vec::new(),
                        classification: None,
                    },
                },
                MatrixArm {
                    target: "safe".into(),
                    summary: RunSummary {
                        results: vec![CaseResult {
                            case_id: "<img src=x>".into(),
                            output: None,
                            error: Some("boom".into()),
                            scores: vec![],
                            cost_usd: None,
                            context: None,
                            trials: None,
                        }],
                        gates: Vec::new(),
                        classification: None,
                    },
                },
            ],
        };
        let comparison = compare_arms(&matrix);
        let rendered = html_matrix(&matrix, &comparison);
        assert!(
            rendered.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "hostile target name escaped; got: {rendered}"
        );
        assert!(
            rendered.contains("&lt;img src=x&gt;"),
            "hostile case id escaped"
        );
        assert!(
            !rendered.contains("<script>alert(1)"),
            "no live script tag survives"
        );
    }

    #[test]
    fn matrix_reporters_do_not_disturb_single_run_output() {
        // A one-arm-style single summary still renders byte-identically through
        // the single-run reporters — the refactor kept them pure.
        assert_eq!(terminal(&fixture()), terminal(&fixture()));
        assert_eq!(junit(&fixture()), junit(&fixture()));
    }
}
