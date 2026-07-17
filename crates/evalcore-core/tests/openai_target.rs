//! HTTP target tests. Never hits a real API — everything runs against a
//! local wiremock server.

use evalcore_core::{OpenAiCompatTarget, Target, TestCase, TokenUsage};
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

#[tokio::test]
async fn retries_transient_429_then_succeeds() {
    let server = MockServer::start().await;
    // First attempt: 429 with an immediate retry-after; second attempt: 200.
    // Mocks match in mount order; the 429 exhausts after one use.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string(r#"{"error": "rate limited"}"#),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "recovered"}}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None)
        .with_max_retries(1);
    let out = target.invoke(&case("hi")).await.unwrap();
    assert_eq!(out.text, "recovered");
    server.verify().await;
}

#[tokio::test]
async fn exhausted_retries_report_attempt_count() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(503)
                .insert_header("retry-after", "0")
                .set_body_string("overloaded"),
        )
        .expect(2)
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None)
        .with_max_retries(1);
    let err = target.invoke(&case("hi")).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("after 2 attempts"), "got: {msg}");
    server.verify().await;
}

#[tokio::test]
async fn client_errors_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
        .expect(1)
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None)
        .with_max_retries(5);
    let err = target.invoke(&case("hi")).await.unwrap_err();
    assert!(err.to_string().contains("401"), "got: {err}");
    server.verify().await;
}

#[tokio::test]
async fn captures_token_usage_when_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}}],
            "usage": {"prompt_tokens": 42, "completion_tokens": 7, "total_tokens": 49},
        })))
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None);
    let out = target.invoke(&case("hi")).await.unwrap();
    assert_eq!(
        out.tokens,
        Some(TokenUsage {
            input: 42,
            output: 7
        })
    );
}
