//! Gemini generateContent target tests. Never hits a real API — everything
//! runs against a local wiremock server.

use evalcore_core::{GeminiTarget, Target, TestCase, TokenUsage};
use wiremock::matchers::{body_partial_json, header, method, path, query_param_is_missing};
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

/// A minimal well-formed generateContent response.
fn candidate(text: &str) -> serde_json::Value {
    serde_json::json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP",
        }],
    })
}

fn target(server: &MockServer, api_key: Option<&str>) -> GeminiTarget {
    GeminiTarget::new(
        format!("{}/v1beta", server.uri()),
        "gemini-2.5-pro".into(),
        api_key.map(str::to_string),
        120,
    )
    .unwrap()
}

/// The model rides in the path, after a colon-suffixed method name.
const GENERATE: &str = "/v1beta/models/gemini-2.5-pro:generateContent";

#[tokio::test]
async fn parses_response_and_sends_the_key_as_a_header_not_a_query_param() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .and(header("x-goog-api-key", "goog-test"))
        // The API also accepts ?key=, but a secret in the query string would
        // leak into error messages (which quote the URL) and into reports.
        .and(query_param_is_missing("key"))
        .and(body_partial_json(serde_json::json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(candidate("hello from mock")))
        .expect(1)
        .mount(&server)
        .await;

    let out = target(&server, Some("goog-test"))
        .invoke(&case("hi"))
        .await
        .unwrap();
    assert_eq!(out.text, "hello from mock");
    server.verify().await;
}

#[tokio::test]
async fn system_instruction_and_generation_config_reach_the_wire() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .and(body_partial_json(serde_json::json!({
            "systemInstruction": {"parts": [{"text": "You are terse."}]},
            "generationConfig": {"temperature": 0, "maxOutputTokens": 256},
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(candidate("ok")))
        .expect(1)
        .mount(&server)
        .await;

    let mut params = serde_json::Map::new();
    params.insert(
        "generationConfig".into(),
        serde_json::json!({"temperature": 0, "maxOutputTokens": 256}),
    );
    target(&server, None)
        .with_system(Some("You are terse.".into()))
        .with_params(Some(params))
        .invoke(&case("hi"))
        .await
        .unwrap();
    server.verify().await;
}

#[tokio::test]
async fn reads_text_past_a_thought_part_and_captures_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "candidates": [{
                "content": {"parts": [
                    {"text": "considering the refund policy", "thought": true},
                    {"text": "Refunds take 30 days."},
                ]},
                "finishReason": "STOP",
            }],
            "usageMetadata": {"promptTokenCount": 42, "candidatesTokenCount": 7},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let out = target(&server, None).invoke(&case("hi")).await.unwrap();
    // A `thought` part also carries `text` — it is reasoning, not the answer.
    assert_eq!(out.text, "Refunds take 30 days.");
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
async fn safety_blocks_are_readable_failures_and_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "promptFeedback": {"blockReason": "SAFETY"},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let err = target(&server, None)
        .with_max_retries(3)
        .invoke(&case("hi"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("blocked the prompt"), "got: {err}");
    assert!(err.contains("SAFETY"), "got: {err}");
    assert!(!err.contains("attempts"), "a block must not retry: {err}");
    server.verify().await;
}

#[tokio::test]
async fn surfaces_http_errors_and_malformed_bodies() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            r#"{"error":{"code":400,"message":"API key not valid","status":"INVALID_ARGUMENT"}}"#,
        ))
        .mount(&server)
        .await;
    let err = target(&server, None)
        .invoke(&case("hi"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("400"), "got: {err}");
    assert!(err.contains("API key not valid"), "got: {err}");

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;
    let err = target(&server, None)
        .invoke(&case("hi"))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("non-JSON body"), "got: {err}");
}

#[tokio::test]
async fn retries_transient_429_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string(r#"{"error":{"status":"RESOURCE_EXHAUSTED"}}"#),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(GENERATE))
        .respond_with(ResponseTemplate::new(200).set_body_json(candidate("recovered")))
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
