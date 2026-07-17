//! HTTP target tests. Never hits a real API — everything runs against a
//! local wiremock server.

use evalcore_core::{OpenAiCompatTarget, Target, TestCase};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn case(input: &str) -> TestCase {
    TestCase {
        id: "t".into(),
        input: input.into(),
        expected: None,
    }
}

#[tokio::test]
async fn parses_openai_chat_completion_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-test"))
        .and(body_partial_json(serde_json::json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "hi"}],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hello from mock"}}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(
        format!("{}/v1", server.uri()),
        "test-model".into(),
        Some("sk-test".into()),
    );
    let out = target.invoke(&case("hi")).await.unwrap();
    assert_eq!(out.text, "hello from mock");
}

#[tokio::test]
async fn surfaces_http_errors_with_status_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string(r#"{"error": "rate limited"}"#))
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None);
    let err = target.invoke(&case("hi")).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("429"), "got: {msg}");
    assert!(msg.contains("rate limited"), "got: {msg}");
}

#[tokio::test]
async fn rejects_bodies_missing_message_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [],
        })))
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None);
    let err = target.invoke(&case("hi")).await.unwrap_err();
    assert!(
        err.to_string().contains("choices[0].message.content"),
        "got: {err}"
    );
}
