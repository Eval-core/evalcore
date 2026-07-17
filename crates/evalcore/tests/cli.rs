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
