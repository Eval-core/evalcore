//! End-to-end CLI tests: spawn the real binary against real files. The
//! quickstart example doubles as a test fixture, so it can never rot.

use assert_cmd::Command;
use predicates::prelude::*;

fn quickstart() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/quickstart/evals.yaml"
    )
}

fn evalcore() -> Command {
    Command::cargo_bin("evalcore").unwrap()
}

#[test]
fn validate_accepts_the_quickstart_config() {
    evalcore()
        .args(["validate", quickstart()])
        .assert()
        .success()
        .stdout(predicate::str::contains("OK: 1 target(s)"));
}

#[test]
fn run_passes_the_quickstart_suite_with_exit_zero() {
    evalcore()
        .args(["run", quickstart()])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 passed, 0 failed, 2 total"))
        .stdout(predicate::str::contains("PASS refund-1"));
}

#[test]
fn run_reports_junit_to_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("junit.xml");
    evalcore()
        .args(["run", quickstart(), "--reporter", "junit", "--output"])
        .arg(&report)
        .assert()
        .success();

    let xml = std::fs::read_to_string(&report).unwrap();
    assert!(
        xml.contains("<testsuites tests=\"2\" failures=\"0\">"),
        "got: {xml}"
    );
}

#[test]
fn failing_suite_exits_one_with_reasons() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "xyzzy-never-present"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        r#"{"id": "will-fail", "input": "hello"}"#,
    )
    .unwrap();

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL will-fail"))
        .stdout(predicate::str::contains("xyzzy-never-present"));
}

#[test]
fn invalid_config_fails_validation_with_a_useful_message() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        "targets: {}\ndatasets: []\nscorers: []\n",
    )
    .unwrap();

    evalcore()
        .arg("validate")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("at least one target"));
}

#[test]
fn unknown_target_lists_available_ones() {
    evalcore()
        .args(["run", quickstart(), "--target", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("available: echo"));
}

/// Strip `(<n>ms)` latency stamps so two separate runs compare equal despite
/// the real shell target's timing jitter — latencies are never stable, so the
/// workspace convention is to assert on everything else.
fn redact_latencies(s: &[u8]) -> String {
    String::from_utf8_lossy(s)
        .lines()
        .map(|line| match line.rfind(" (") {
            Some(idx) if line.ends_with("ms)") => line[..idx].to_string(),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn html_report_is_written_alongside_terminal_without_changing_stdout() {
    // A run without --html, captured as the stdout baseline (latencies redacted).
    let plain = evalcore().args(["run", quickstart()]).assert().success();
    let plain_stdout = redact_latencies(&plain.get_output().stdout);

    // The same run with --html: stdout must match (modulo latency jitter), exit
    // unchanged, and the HTML file must exist carrying a known case id.
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("out.html");
    let with_html = evalcore()
        .args(["run", quickstart(), "--reporter", "terminal", "--html"])
        .arg(&report)
        .assert()
        .success();
    assert_eq!(
        redact_latencies(&with_html.get_output().stdout),
        plain_stdout,
        "--html must not perturb the terminal reporter's stdout"
    );

    let doc = std::fs::read_to_string(&report).unwrap();
    assert!(
        doc.starts_with("<!DOCTYPE html>"),
        "got: {}",
        &doc[..40.min(doc.len())]
    );
    assert!(doc.contains("refund-1"), "case id must be in the report");
}

#[test]
fn html_report_composes_with_json_reporter_keeping_stdout_pure_json() {
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("out.html");
    let assert = evalcore()
        .args(["run", quickstart(), "--reporter", "json", "--html"])
        .arg(&report)
        .assert()
        .success();

    // Stdout stays parseable JSON — the HTML went to the file, not stdout.
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be pure JSON, got err {e}: {stdout}"));
    assert!(parsed.get("results").is_some(), "JSON report on stdout");
    assert!(report.exists(), "HTML file written");
}

/// Write a one-case, always-passing suite (echo target, `contains: "ok"`) with
/// the given `run.gates` block appended.
fn write_gated_suite(dir: &std::path::Path, gates: &str) {
    std::fs::write(
        dir.join("evals.yaml"),
        format!(
            r#"
targets:
  echo: {{ type: shell, cmd: "cat" }}
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "ok"
run:
  gates:
{gates}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("cases.jsonl"),
        concat!(r#"{"id": "good", "input": "this is ok"}"#, "\n"),
    )
    .unwrap();
}

#[test]
fn passing_gate_reports_gate_pass_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    write_gated_suite(dir.path(), "    - type: pass_rate\n      min: 1.0");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed, 1 total"))
        .stdout(predicate::str::contains("GATE PASS pass_rate >= 1"));
}

#[test]
fn failing_gate_exits_one_even_when_every_case_passes() {
    // Isolates the ADDITIVE property: all cases pass (per-case contract → 0),
    // but a mean_score floor of 2.0 over `contains` scores (max 1.0) can never
    // be met, so the gate alone forces exit 1.
    let dir = tempfile::tempdir().unwrap();
    write_gated_suite(dir.path(), "    - type: mean_score\n      min: 2.0");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("1 passed, 0 failed, 1 total"))
        .stdout(predicate::str::contains("GATE FAIL mean_score >= 2"));
}

#[test]
fn gate_floor_fails_even_when_baseline_tolerates_the_case() {
    // A two-case suite: `good` passes, `known-bad` fails. The baseline accepts
    // known-bad, so the baseline gate is OK — but a pass_rate floor of 1.0 is
    // an absolute floor the accepted failure still violates, so exit stays 1.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "ok"
run:
  gates:
    - type: pass_rate
      min: 1.0
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        concat!(
            r#"{"id": "good", "input": "this is ok"}"#,
            "\n",
            r#"{"id": "known-bad", "input": "this fails"}"#,
            "\n",
        ),
    )
    .unwrap();

    // Record the baseline (the run itself fails known-bad → exit 1).
    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--save-baseline", "main"])
        .assert()
        .code(1);

    // Re-run against the baseline: known-bad is an accepted failure (baseline
    // gate OK), yet the pass_rate floor of 1.0 still fails the run.
    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--baseline", "main"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("baseline gate: OK"))
        .stdout(predicate::str::contains("GATE FAIL pass_rate >= 1"));
}

#[test]
fn saved_baseline_is_a_pure_snapshot_without_gates() {
    // A baseline is per-case history: gate results are run-scoped acceptance
    // criteria and must never be persisted into a stored baseline row.
    let dir = tempfile::tempdir().unwrap();
    write_gated_suite(dir.path(), "    - type: pass_rate\n      min: 1.0");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--save-baseline", "main"])
        .assert()
        .success();

    let store = evalcore_store::Store::open(&dir.path().join(".evalcore/cache.db")).unwrap();
    let baseline = store
        .load_baseline("main")
        .unwrap()
        .expect("baseline saved");
    assert!(
        baseline.gates.is_empty(),
        "stored baseline must carry no gate results, got {:?}",
        baseline.gates
    );
}

/// Write a labeled classification suite: a `cat` shell target echoes the input,
/// so each case's prediction is its input. Cases are labeled via `expected`, and
/// an always-passing `contains: ""` scorer keeps every per-case verdict green so
/// only the accuracy gate drives the exit code. Two of three cases match their
/// label (accuracy = 2/3 ≈ 0.667); the caller supplies the gate `min`.
fn write_classification_suite(dir: &std::path::Path, accuracy_min: &str) {
    std::fs::write(
        dir.join("evals.yaml"),
        format!(
            r#"
targets:
  echo: {{ type: shell, cmd: "cat" }}
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: ""
run:
  gates:
    - type: accuracy
      min: {accuracy_min}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("cases.jsonl"),
        concat!(
            r#"{"id": "a", "input": "cat", "expected": "cat"}"#,
            "\n",
            r#"{"id": "b", "input": "dog", "expected": "dog"}"#,
            "\n",
            r#"{"id": "c", "input": "fish", "expected": "bird"}"#,
            "\n",
        ),
    )
    .unwrap();
}

#[test]
fn accuracy_gate_met_reports_classification_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    write_classification_suite(dir.path(), "0.6");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "classification: accuracy 0.67 · macro-F1",
        ))
        .stdout(predicate::str::contains(
            "classification: accuracy 0.67 · macro-F1 0.67 (3 labeled, 0 unlabeled)",
        ))
        .stdout(predicate::str::contains("GATE PASS accuracy >= 0.6"));
}

#[test]
fn accuracy_gate_unmet_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    write_classification_suite(dir.path(), "0.9");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("GATE FAIL accuracy >= 0.9"))
        .stdout(predicate::str::contains("classification: accuracy 0.67"));
}

/// Write a two-target matrix suite: `echo` (`cat`) passes any case containing
/// "refund"; `upper` uppercases, so it never contains lowercase "refund". The
/// caller supplies the `scorers` block so a suite can be made all-pass.
fn write_matrix_suite(dir: &std::path::Path, scorers: &str) {
    std::fs::write(
        dir.join("evals.yaml"),
        format!(
            r#"
targets:
  echo: {{ type: shell, cmd: "cat" }}
  upper: {{ type: shell, cmd: "tr '[:lower:]' '[:upper:]'" }}
datasets:
  - file: cases.jsonl
scorers:
{scorers}
run:
  matrix: [echo, upper]
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("cases.jsonl"),
        concat!(
            r#"{"id": "refund-1", "input": "please process my refund"}"#,
            "\n",
            r#"{"id": "refund-2", "input": "another refund request"}"#,
            "\n",
        ),
    )
    .unwrap();
}

#[test]
fn matrix_run_renders_comparison_and_exits_one_when_an_arm_fails() {
    // echo passes both cases; upper fails both (uppercased, no lowercase
    // "refund"). Any failing arm fails the whole matrix contract → exit 1.
    let dir = tempfile::tempdir().unwrap();
    write_matrix_suite(
        dir.path(),
        "  - type: contains\n    value: \"refund\"\n    case_sensitive: true",
    );

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("== target: echo"))
        .stdout(predicate::str::contains("== target: upper"))
        .stdout(predicate::str::contains("== comparison"))
        // Arms run in list order; the header names them in that order.
        .stdout(predicate::str::contains("case"))
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("upper"))
        // echo uniquely wins refund-1; the wins footer tallies it.
        .stdout(predicate::str::contains("wins: echo 2 · upper 0 · ties 0"));
}

#[test]
fn matrix_run_all_arms_pass_exits_zero() {
    // An always-passing scorer (`contains: ""`) makes every arm satisfy the
    // contract, so the matrix exits 0.
    let dir = tempfile::tempdir().unwrap();
    write_matrix_suite(dir.path(), "  - type: contains\n    value: \"\"");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .success()
        .stdout(predicate::str::contains("== comparison"));
}

#[test]
fn matrix_unknown_name_errors_naming_available_targets() {
    let dir = tempfile::tempdir().unwrap();
    write_matrix_suite(dir.path(), "  - type: contains\n    value: \"\"");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--matrix", "echo,mystery"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "matrix target \"mystery\" is not defined; available: echo, upper",
        ));
}

#[test]
fn matrix_with_target_is_a_hard_error() {
    let dir = tempfile::tempdir().unwrap();
    write_matrix_suite(dir.path(), "  - type: contains\n    value: \"\"");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--matrix", "echo,upper", "--target", "echo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot combine --target with a matrix",
        ));
}

#[test]
fn matrix_with_baseline_is_a_hard_error() {
    let dir = tempfile::tempdir().unwrap();
    write_matrix_suite(dir.path(), "  - type: contains\n    value: \"\"");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--matrix", "echo,upper", "--baseline", "main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "baselines are per-run; run targets separately with --target",
        ));
}
