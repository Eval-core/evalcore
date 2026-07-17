//! E2E proof of the record/replay cache: the real binary, a mock LLM server,
//! and a mock-side guarantee (`expect(1)`) that repeated runs cost exactly one
//! network call.

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write_suite(dir: &std::path::Path, base_url: &str) {
    std::fs::write(
        dir.join("evals.yaml"),
        format!(
            r#"
targets:
  mock-llm:
    type: openai-compatible
    url: {base_url}/v1
    model: test-model
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "refund"
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("cases.jsonl"),
        r#"{"id": "case-1", "input": "Where is my refund?"}"#,
    )
    .unwrap();
}

fn run_suite(dir: &std::path::Path, extra_args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.join("evals.yaml"))
        .args(extra_args)
        .assert()
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_runs_hit_the_network_exactly_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Your refund is on its way."}}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &server.uri());

    // First run (default --cache auto): live call, recorded.
    run_suite(dir.path(), &[])
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));

    // Second run: served from .evalcore/cache.db, no network.
    run_suite(dir.path(), &[])
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));

    // Replay mode: still green, provably offline-capable.
    run_suite(dir.path(), &["--cache", "replay"])
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));

    assert!(
        dir.path().join(".evalcore/cache.db").exists(),
        "cache file created next to the config"
    );
    // expect(1) is enforced here: >1 request panics.
    server.verify().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_mode_fails_loudly_on_a_cold_cache() {
    // Server that must never be contacted.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "never seen"}}],
        })))
        .expect(0)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &server.uri());

    run_suite(dir.path(), &["--cache", "replay"])
        .code(1)
        .stdout(predicate::str::contains("cache miss"))
        .stdout(predicate::str::contains("case-1"));

    server.verify().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_needs_no_api_key_once_recorded() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Your refund is on its way."}}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        format!(
            r#"
targets:
  mock-llm:
    type: openai-compatible
    url: {}/v1
    model: test-model
    api_key_env: EVALCORE_E2E_KEY
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "refund"
"#,
            server.uri()
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        r#"{"id": "case-1", "input": "Where is my refund?"}"#,
    )
    .unwrap();

    // Record with the key present.
    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .env("EVALCORE_E2E_KEY", "sk-dummy")
        .assert()
        .success();

    // Replay with the key absent — must still pass: the CI-without-secrets story.
    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--cache", "replay"])
        .env_remove("EVALCORE_E2E_KEY")
        .assert()
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));

    server.verify().await;
}
