//! Offline protocol tests for the shipped RAG-metric shims (`shims/`).
//!
//! These exercise each shim's `--check` self-test path: pipe a payload into
//! `python3 <shim> --check` and assert protocol behavior. `--check` never
//! imports Ragas/DeepEval and never calls an LLM, so these tests run fully
//! offline with no pip packages installed. If `python3` is absent the tests
//! skip gracefully rather than fail.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The four shipped shim scripts, relative to the workspace root.
const SHIMS: &[&str] = &[
    "ragas/faithfulness.py",
    "ragas/context_recall.py",
    "deepeval/faithfulness.py",
    "deepeval/contextual_recall.py",
];

/// `shims/` lives at the workspace root, two levels up from this crate.
fn shims_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../shims")
        .canonicalize()
        .expect("shims/ directory should exist at the workspace root")
}

/// `true` if `python3` is available; tests skip (with a note) when it is not.
fn python3_present() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `python3 <shim> [--check]` feeding `stdin`; return (success, stdout).
fn run_shim(shim: &str, check: bool, stdin: &str) -> (bool, String) {
    let mut cmd = Command::new("python3");
    cmd.arg(shims_dir().join(shim));
    if check {
        cmd.arg("--check");
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn python3");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// A valid payload carrying context and an expected value.
const VALID: &str = r#"{"input":"q","output":"a","expected":"g","context":["c1","c2"]}"#;

#[test]
fn check_mode_accepts_valid_payload_and_emits_scored_verdict() {
    if !python3_present() {
        eprintln!("skipping shims test: python3 not found");
        return;
    }
    for shim in SHIMS {
        let (ok, stdout) = run_shim(shim, true, VALID);
        assert!(ok, "{shim} --check should exit 0 on a valid payload");
        let verdict: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("{shim} --check printed unparseable JSON {stdout:?}: {e}"));
        let score = verdict["score"]
            .as_f64()
            .unwrap_or_else(|| panic!("{shim} --check verdict has no numeric score: {stdout:?}"));
        assert!(
            (0.0..=1.0).contains(&score),
            "{shim} --check score {score} out of [0,1]"
        );
    }
}

#[test]
fn check_mode_rejects_malformed_json() {
    if !python3_present() {
        eprintln!("skipping shims test: python3 not found");
        return;
    }
    for shim in SHIMS {
        let (ok, _) = run_shim(shim, true, "not json at all");
        assert!(!ok, "{shim} --check should exit non-zero on malformed JSON");
    }
}

#[test]
fn check_mode_tolerates_missing_context() {
    // Decision: `--check` validates the *protocol* (stdin parses, required
    // input/output present, well-formed verdict out), NOT a metric's input
    // preconditions. So a payload lacking `context` still passes `--check` —
    // the context requirement is enforced only in normal scoring mode. This
    // keeps the CI self-test offline and independent of dataset shape.
    if !python3_present() {
        eprintln!("skipping shims test: python3 not found");
        return;
    }
    let no_context = r#"{"input":"q","output":"a","expected":"g"}"#;
    for shim in SHIMS {
        let (ok, stdout) = run_shim(shim, true, no_context);
        assert!(
            ok,
            "{shim} --check should exit 0 even without context (protocol-only check)"
        );
        let verdict: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("{shim} --check printed unparseable JSON {stdout:?}: {e}"));
        assert!(
            verdict["score"].as_f64().is_some(),
            "{shim} --check should still emit a numeric score: {stdout:?}"
        );
    }
}
