//! E2E tests for run-history recording: `evalcore run` appends one row per
//! executed run (matrix: one per arm) to the store, on by default, and the
//! recording is a pure side-effect — its failure never changes the exit code.

use assert_cmd::Command;
use evalcore_store::Store;
use predicates::prelude::*;

/// A single always-passing case (echo target, `contains: "ok"`), so the run
/// exits 0 and history-recording effects are isolated from the eval verdict.
fn write_passing_suite(dir: &std::path::Path) {
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
        concat!(r#"{"id": "good", "input": "this is ok"}"#, "\n"),
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

/// Open the store the run would have written and return its history rows.
fn history(dir: &std::path::Path) -> Vec<evalcore_store::RunMeta> {
    Store::open(&dir.join(".evalcore/cache.db"))
        .unwrap()
        .list_runs()
        .unwrap()
}

#[test]
fn default_run_records_exactly_one_history_row() {
    let dir = tempfile::tempdir().unwrap();
    write_passing_suite(dir.path());

    evalcore(dir.path(), &[]).success();

    let runs = history(dir.path());
    assert_eq!(runs.len(), 1, "one row per run");
    assert_eq!(runs[0].target, "echo");
    // The config path is stored as the user gave it (the absolute path here).
    assert!(
        runs[0].config.ends_with("evals.yaml"),
        "got: {}",
        runs[0].config
    );
    let summary = runs[0].summary.as_ref().unwrap();
    assert_eq!(summary.passed(), 1);
    assert_eq!(summary.total(), 1);
}

#[test]
fn no_history_flag_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write_passing_suite(dir.path());

    evalcore(dir.path(), &["--no-history"]).success();

    // A shell-only run with history off never opens the store, so no file.
    assert!(
        !dir.path().join(".evalcore/cache.db").exists(),
        "--no-history must not create a store"
    );
}

#[test]
fn history_false_in_config_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write_passing_suite(dir.path());
    // Append the run.history: false toggle to the suite.
    let mut yaml = std::fs::read_to_string(dir.path().join("evals.yaml")).unwrap();
    yaml.push_str("run:\n  history: false\n");
    std::fs::write(dir.path().join("evals.yaml"), yaml).unwrap();

    evalcore(dir.path(), &[]).success();

    assert!(
        !dir.path().join(".evalcore/cache.db").exists(),
        "run.history: false must not create a store"
    );
}

#[test]
fn matrix_records_one_row_per_arm() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        r#"
targets:
  a: { type: shell, cmd: "cat" }
  b: { type: shell, cmd: "cat" }
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "ok"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        concat!(r#"{"id": "good", "input": "this is ok"}"#, "\n"),
    )
    .unwrap();

    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--matrix", "a,b"])
        .assert()
        .success();

    let runs = history(dir.path());
    assert_eq!(runs.len(), 2, "one row per arm");
    // Newest first: arm b (recorded second) precedes arm a.
    let targets: Vec<&str> = runs.iter().map(|r| r.target.as_str()).collect();
    assert_eq!(targets, vec!["b", "a"], "arm target names, newest first");
}

#[test]
fn history_write_failure_does_not_change_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    write_passing_suite(dir.path());
    // Make the store path un-openable: a directory where the db file goes. The
    // run still exits 0 (all cases pass); recording just warns.
    std::fs::create_dir_all(dir.path().join(".evalcore/cache.db")).unwrap();

    evalcore(dir.path(), &[])
        .success()
        .stderr(predicate::str::contains("could not record run history"));
}
