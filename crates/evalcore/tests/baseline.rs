//! E2E tests for baseline regression gating: the exit contract flips from
//! "all passed" to "no regressions" when --baseline is given.

use assert_cmd::Command;
use predicates::prelude::*;

/// Suite with one always-passing and one always-failing case (echo target,
/// `contains: "ok"`).
fn write_suite(dir: &std::path::Path) {
    std::fs::write(
        dir.join("evals.yaml"),
        r#"
targets:
  echo: { type: shell, cmd: "cat" }
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "ok"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("cases.jsonl"),
        concat!(
            r#"{"id": "good", "input": "this is ok"}"#,
            "\n",
            r#"{"id": "known-bad", "input": "this fails"}"#,
            "\n",
        ),
    )
    .unwrap();
}

fn evalcore(dir: &std::path::Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.join("evals.yaml"))
        .args(extra)
        .assert()
}

#[test]
fn baseline_gate_tolerates_accepted_failures_and_catches_regressions() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path());

    // 1. Record the baseline. The run itself fails (known-bad), exit 1.
    evalcore(dir.path(), &["--save-baseline", "main"])
        .code(1)
        .stderr(predicate::str::contains(
            "saved baseline \"main\" (1/2 passed)",
        ));

    // 2. Same suite vs baseline: known-bad is an accepted failure → exit 0.
    evalcore(dir.path(), &["--baseline", "main"])
        .success()
        .stdout(predicate::str::contains("baseline gate: OK"));

    // 3. Break the previously-passing case → regression → exit 1 with names.
    std::fs::write(
        dir.path().join("cases.jsonl"),
        concat!(
            r#"{"id": "good", "input": "now failing"}"#,
            "\n",
            r#"{"id": "known-bad", "input": "this fails"}"#,
            "\n",
        ),
    )
    .unwrap();
    evalcore(dir.path(), &["--baseline", "main"])
        .code(1)
        .stdout(predicate::str::contains("REGRESSED good"))
        .stdout(predicate::str::contains("baseline gate: FAIL"));
}

#[test]
fn new_failing_cases_fail_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path());
    evalcore(dir.path(), &["--save-baseline", "main"]).code(1);

    let mut cases = std::fs::read_to_string(dir.path().join("cases.jsonl")).unwrap();
    cases.push_str("{\"id\": \"brand-new\", \"input\": \"also fails\"}\n");
    std::fs::write(dir.path().join("cases.jsonl"), cases).unwrap();

    evalcore(dir.path(), &["--baseline", "main"])
        .code(1)
        .stdout(predicate::str::contains("NEW FAIL"))
        .stdout(predicate::str::contains("brand-new"));
}

#[test]
fn missing_baseline_names_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path());

    evalcore(dir.path(), &["--baseline", "never-saved"])
        .failure()
        .stderr(predicate::str::contains("no baseline \"never-saved\""))
        .stderr(predicate::str::contains("--save-baseline never-saved"));
}

#[test]
fn rolling_baseline_compare_then_save_in_one_run() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path());
    evalcore(dir.path(), &["--save-baseline", "main"]).code(1);

    // Fix the failing case, compare AND re-save in the same run.
    std::fs::write(
        dir.path().join("cases.jsonl"),
        concat!(
            r#"{"id": "good", "input": "this is ok"}"#,
            "\n",
            r#"{"id": "known-bad", "input": "ok now"}"#,
            "\n",
        ),
    )
    .unwrap();
    evalcore(
        dir.path(),
        &["--baseline", "main", "--save-baseline", "main"],
    )
    .success()
    .stdout(predicate::str::contains("FIXED     known-bad"))
    .stderr(predicate::str::contains(
        "saved baseline \"main\" (2/2 passed)",
    ));

    // The rolled baseline now expects known-bad to pass: breaking it again is
    // a regression, not an accepted failure.
    write_suite(dir.path());
    evalcore(dir.path(), &["--baseline", "main"])
        .code(1)
        .stdout(predicate::str::contains("REGRESSED known-bad"));
}
