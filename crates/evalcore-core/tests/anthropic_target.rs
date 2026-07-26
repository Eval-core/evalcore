//! Anthropic Messages target tests. Never hits a real API — everything runs
//! against a local wiremock server.

use evalcore_core::{AnthropicTarget, Target, TestCase, TokenUsage};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn case(input: &str) -> TestCase {
    TestCase {
        id: "t".into(),
        input: input.into(),
        expected: None,
        trace: None,
        context: None,
    }
}

/// A minimal well-formed Messages response carrying `text`.
fn message(text: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "m",
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
    })
}

fn target(server: &MockServer, api_key: Option<&str>) -> AnthropicTarget {
    AnthropicTarget::new(
        format!("{}/v1", server.uri()),
        "m".into(),
        api_key.map(str::to_string),
        120,
    )
    .unwrap()
}

#[tokio::test]
async fn parses_messages_response_with_anthropic_auth_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        // x-api-key, not `Authorization: Bearer`; the version header is required.
        .and(header("x-api-key", "sk-ant-test"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(body_partial_json(serde_json::json!({
            "model": "m",
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": "hi"}],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(message("hello from mock")))
        .expect(1)
        .mount(&server)
        .await;

    let out = target(&server, Some("sk-ant-test"))
        .invoke(&case("hi"))
        .await
        .unwrap();
    assert_eq!(out.text, "hello from mock");
    server.verify().await;
}

#[tokio::test]
async fn system_is_sent_as_a_top_level_field_with_params_and_max_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(serde_json::json!({
            "model": "m",
            "max_tokens": 256,
            "system": "You are terse.",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(message("ok")))
        .expect(1)
        .mount(&server)
        .await;

    let mut params = serde_json::Map::new();
    params.insert("temperature".into(), serde_json::json!(0));
    let target = target(&server, None)
        .with_system(Some("You are terse.".into()))
        .with_max_tokens(256)
        .with_params(Some(params));

    target.invoke(&case("hi")).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn reads_text_past_a_thinking_block() {
    // With extended thinking on, `content[0]` is a thinking block with no
    // `text` field — reading `content[0].text` would fail on a valid response.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "the policy says 30 days", "signature": "abc"},
                {"type": "text", "text": "Refunds take 30 days."},
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 42, "output_tokens": 7},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let out = target(&server, None).invoke(&case("hi")).await.unwrap();
    assert_eq!(out.text, "Refunds take 30 days.");
    // input_tokens / output_tokens, not prompt_tokens / completion_tokens.
    assert_eq!(
        out.tokens,
        Some(TokenUsage {
            input: 42,
            output: 7
        })
    );
    server.verify().await;
}

#[tokio::test]
async fn policy_refusal_arrives_as_200_and_becomes_a_readable_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [],
            "stop_reason": "refusal",
            "usage": {"input_tokens": 12, "output_tokens": 0},
        })))
        // Permanent: a refusal is deterministic, so retrying is pure waste.
        .expect(1)
        .mount(&server)
        .await;

    let err = target(&server, None)
        .with_max_retries(3)
        .invoke(&case("hi"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("declined to answer"), "got: {err}");
    assert!(err.contains("refusal"), "got: {err}");
    // No "(after N attempts)" suffix: that is appended only to transient errors.
    assert!(!err.contains("attempts"), "refusal must not retry: {err}");
    server.verify().await;
}

#[tokio::test]
async fn surfaces_http_errors_with_status_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: required"}}"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let err = target(&server, None)
        .invoke(&case("hi"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("400"), "got: {err}");
    assert!(err.contains("max_tokens: required"), "got: {err}");
    server.verify().await;
}

#[tokio::test]
async fn rejects_malformed_and_textless_bodies() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .mount(&server)
        .await;
    let err = target(&server, None)
        .invoke(&case("hi"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("non-JSON body"), "got: {err}");

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{"type": "thinking", "thinking": "…"}],
            "stop_reason": "max_tokens",
        })))
        .mount(&server)
        .await;
    let err = target(&server, None)
        .invoke(&case("hi"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("no text block"), "got: {err}");
    assert!(err.contains("max_tokens"), "got: {err}");
}

#[tokio::test]
async fn retries_transient_429_then_succeeds() {
    let server = MockServer::start().await;
    // Mocks match in mount order; the 429 exhausts after one use.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string(r#"{"type":"error","error":{"type":"rate_limit_error"}}"#),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message("recovered")))
        .expect(1)
        .mount(&server)
        .await;

    let out = target(&server, None)
        .with_max_retries(1)
        .invoke(&case("hi"))
        .await
        .unwrap();
    assert_eq!(out.text, "recovered");
    server.verify().await;
}

#[tokio::test]
async fn request_timeout_names_the_budget_and_is_transient() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(5))
                .set_body_json(message("too late")),
        )
        .mount(&server)
        .await;

    let target = AnthropicTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 1)
        .unwrap()
        .with_max_retries(0);
    let err = target.invoke(&case("hi")).await.unwrap_err().to_string();
    assert!(err.contains("timed out after 1s"), "got: {err}");
    // The retry loop appends "(after N attempts)" only to transient errors.
    assert!(err.contains("after 1 attempts"), "got: {err}");
}
