//! CLI-level `run.trials` tests: spawn the real binary against a shell target
//! (deterministic, no network) and assert the `[k/N trials]` terminal tag and
//! the exit-code contract.

use assert_cmd::Command;
use predicates::prelude::*;

fn evalcore() -> Command {
    Command::cargo_bin("evalcore").unwrap()
}

/// Write a one-case suite: an echo (`cat`) shell target, a `contains` scorer,
/// and `run.trials: 3`. The shell target is deterministic, so all three trials
/// agree — the tag is `[3/3 trials]` when the scorer passes, `[0/3 trials]`
/// when it fails.
fn write_trials_suite(dir: &std::path::Path, contains: &str, input: &str) {
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
    value: "{contains}"
run:
  trials: 3
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("cases.jsonl"),
        format!("{{\"id\": \"c1\", \"input\": \"{input}\"}}\n"),
    )
    .unwrap();
}

#[test]
fn passing_trials_show_the_tag_and_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    write_trials_suite(dir.path(), "ok", "this is ok");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS c1"))
        .stdout(predicate::str::contains("[3/3 trials]"));
}

#[test]
fn failing_trials_show_the_tag_and_exit_one() {
    let dir = tempfile::tempdir().unwrap();
    write_trials_suite(dir.path(), "xyzzy-never-present", "hello");

    evalcore()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL c1 [0/3 trials]"));
}
