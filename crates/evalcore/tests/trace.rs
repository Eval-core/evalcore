//! E2E: trace ingestion + trajectory assertions through the real binary.
//! The agent-trace example doubles as the fixture (native + OTel traces).

use assert_cmd::Command;
use predicates::prelude::*;

fn example() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/agent-trace/evals.yaml"
    )
}

#[test]
fn agent_trace_example_passes_for_native_and_otel() {
    Command::cargo_bin("evalcore")
        .unwrap()
        .args(["run", example()])
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS refund-flow-native"))
        .stdout(predicate::str::contains("PASS refund-flow-otel"))
        // OTel trace carries token usage; with cost rates declared on the
        // trace target, both totals must surface in the summary.
        .stdout(predicate::str::contains("268 tokens"))
        .stdout(predicate::str::contains("$0.0"));
}

#[test]
fn trajectory_violations_fail_with_named_rules() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        r#"
targets:
  agent: { type: trace }
datasets:
  - file: cases.jsonl
scorers:
  - type: trajectory
    rules:
      - must_not_call: issue_refund
        before: verify_identity
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        r#"{"id": "rogue-refund", "trace": "rogue.json"}"#,
    )
    .unwrap();
    // Refund issued without verifying identity first.
    std::fs::write(
        dir.path().join("rogue.json"),
        r#"{"steps": [
            {"tool": "issue_refund", "input": {"amount": 999}},
            {"tool": "verify_identity", "input": {}}
        ]}"#,
    )
    .unwrap();

    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL rogue-refund"))
        .stdout(predicate::str::contains("must_not_call"))
        .stdout(predicate::str::contains("issue_refund"));
}

#[test]
fn missing_trace_file_is_a_failed_case_with_the_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        r#"
targets:
  agent: { type: trace }
datasets:
  - file: cases.jsonl
scorers:
  - type: trajectory
    rules:
      - max_steps: 5
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        r#"{"id": "ghost", "trace": "does-not-exist.json"}"#,
    )
    .unwrap();

    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL ghost"))
        .stdout(predicate::str::contains("does-not-exist.json"));
}
