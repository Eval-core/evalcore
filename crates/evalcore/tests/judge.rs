//! E2E: a suite scored by an LLM judge, with both the app model and the judge
//! model behind one mock server (distinguished by the `model` field in the
//! request body). `expect(1)` on each proves judge calls ride the
//! record/replay cache exactly like target calls.

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_response(content: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": content}}],
    }))
}

#[tokio::test(flavor = "multi_thread")]
async fn judged_suite_records_then_replays_both_models() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(serde_json::json!({"model": "app-model"})))
        .respond_with(chat_response("Refunds are processed within 30 days."))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(
            serde_json::json!({"model": "judge-model"}),
        ))
        .respond_with(chat_response(
            r#"{"score": 0.9, "reason": "grounded and specific"}"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        format!(
            r#"
targets:
  app:
    type: openai-compatible
    url: {url}/v1
    model: app-model
datasets:
  - file: cases.jsonl
scorers:
  - type: judge
    url: {url}/v1
    model: judge-model
    rubric: "Does the answer state a concrete refund window?"
    threshold: 0.7
"#,
            url = server.uri()
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        r#"{"id": "refund-window", "input": "How long do refunds take?"}"#,
    )
    .unwrap();

    let run = |extra: &[&str]| {
        Command::cargo_bin("evalcore")
            .unwrap()
            .arg("run")
            .arg(dir.path().join("evals.yaml"))
            .args(extra)
            .assert()
    };

    // First run: live app call + live judge call, both recorded.
    run(&[])
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));
    // Second run: both replayed — expect(1) above enforces zero new requests.
    run(&[])
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));
    // Replay mode: the judged suite is fully offline-capable.
    run(&["--cache", "replay"])
        .success()
        .stdout(predicate::str::contains("1 passed, 0 failed"));

    server.verify().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn failing_judge_verdict_shows_its_reason() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(serde_json::json!({"model": "app-model"})))
        .respond_with(chat_response("It depends."))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(
            serde_json::json!({"model": "judge-model"}),
        ))
        .respond_with(chat_response(
            r#"{"score": 0.2, "reason": "no concrete window given"}"#,
        ))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("evals.yaml"),
        format!(
            r#"
targets:
  app:
    type: openai-compatible
    url: {url}/v1
    model: app-model
datasets:
  - file: cases.jsonl
scorers:
  - type: judge
    url: {url}/v1
    model: judge-model
    rubric: "Does the answer state a concrete refund window?"
    threshold: 0.7
"#,
            url = server.uri()
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cases.jsonl"),
        r#"{"id": "vague", "input": "How long do refunds take?"}"#,
    )
    .unwrap();

    Command::cargo_bin("evalcore")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("evals.yaml"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL vague"))
        .stdout(predicate::str::contains("no concrete window given"));
}
