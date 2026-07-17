//! E2E proof that `http` targets are cacheable like any LLM target: the real
//! binary against a mock REST endpoint, with a mock-side `expect(1)` proving a
//! recorded suite replays offline — and without the API key.

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Writes an `http`-target suite whose body carries the `{{input}}` anchor and
/// extracts the answer via a JSON Pointer.
fn write_suite(dir: &std::path::Path, base_url: &str, scorer_value: &str, with_key: bool) {
    let api_key_line = if with_key {
        "\n    api_key_env: HTTP_E2E_KEY"
    } else {
        ""
    };
    std::fs::write(
        dir.join("evals.yaml"),
        format!(
            r#"
targets:
  my-rag:
    type: http
    url: {base_url}/chat
    method: POST{api_key_line}
    body:
      question: "{{{{input}}}}"
    response_path: /answer
datasets:
  - file: cases.jsonl
scorers:
  - type: contains
    value: "{scorer_value}"
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

#[tokio::test(flavor = "multi_thread")]
async fn http_target_records_then_replays_offline_without_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answer": "Your refund is on its way.",
        })))
        .expect(1) // exactly one live call across both runs proves replay is offline.
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &server.uri(), "refund", true);

    // Record with the key present (default --cache auto).
    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .env("HTTP_E2E_KEY", "sk-dummy")
        .assert()
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));

    // Replay with the key REMOVED — a deployed-app suite must gate CI offline
    // and keyless, exactly like an LLM target.
    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .args(["--cache", "replay"])
        .env_remove("HTTP_E2E_KEY")
        .assert()
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));

    assert!(
        dir.path().join(".evalcore/cache.db").exists(),
        "cache file created next to the config"
    );
    server.verify().await; // expect(1): a second live call would panic here.
}

#[tokio::test(flavor = "multi_thread")]
async fn http_target_failing_case_exits_one() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answer": "I cannot help with that.",
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    // The answer never contains "refund", so the single case fails.
    write_suite(dir.path(), &server.uri(), "refund", false);

    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("0 passed, 1 failed"));
}
