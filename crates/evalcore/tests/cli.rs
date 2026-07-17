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
