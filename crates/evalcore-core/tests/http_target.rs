//! `http` target tests. Never hits a real API — everything runs against a
//! local wiremock server.

use evalcore_core::{HttpTarget, Target, TestCase};
use reqwest::Method;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn case(input: &str) -> TestCase {
    TestCase {
        id: "t".into(),
        input: input.into(),
        expected: None,
        trace: None,
    }
}

#[tokio::test]
async fn happy_path_extracts_response_path_and_sends_substituted_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .and(header("authorization", "Bearer sk-test"))
        .and(header("content-type", "application/json"))
        .and(body_partial_json(json!({
            "question": "Where is my refund?",
            "session": "eval",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "answer": "Your refund is on its way.",
            "meta": {"latency": 3},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/chat", server.uri()), Method::POST)
        .with_api_key(Some("sk-test".into()))
        .with_body(Some(json!({"question": "{{input}}", "session": "eval"})))
        .with_response_path(Some("/answer".into()));
    let out = target.invoke(&case("Where is my refund?")).await.unwrap();
    assert_eq!(out.text, "Your refund is on its way.");
    assert!(out.tokens.is_none(), "http targets never report tokens");
    server.verify().await;
}

#[tokio::test]
async fn response_path_non_string_value_is_serialized_compactly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "answer": {"grade": "A", "score": 9},
        })))
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/chat", server.uri()), Method::POST)
        .with_body(Some(json!({"q": "{{input}}"})))
        .with_response_path(Some("/answer".into()));
    let out = target.invoke(&case("hi")).await.unwrap();
    // serde_json's Map is sorted (preserve_order is banned), so this is stable.
    assert_eq!(out.text, r#"{"grade":"A","score":9}"#);
}

#[tokio::test]
async fn raw_body_mode_returns_the_response_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/echo"))
        .respond_with(ResponseTemplate::new(200).set_body_string("plain text answer"))
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/echo", server.uri()), Method::POST)
        .with_body(Some(json!({"q": "{{input}}"})));
    let out = target.invoke(&case("hi")).await.unwrap();
    assert_eq!(out.text, "plain text answer");
}

#[tokio::test]
async fn custom_auth_header_and_empty_prefix_send_the_raw_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .and(header("x-api-key", "raw-key-123"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/chat", server.uri()), Method::POST)
        .with_body(Some(json!({"q": "{{input}}"})))
        .with_api_key(Some("raw-key-123".into()))
        .with_auth_header("x-api-key".into())
        .with_auth_prefix(String::new());
    target.invoke(&case("hi")).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn non_2xx_is_a_permanent_error_with_status_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request payload"))
        .expect(1) // 4xx (non-429) is never retried.
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/chat", server.uri()), Method::POST)
        .with_body(Some(json!({"q": "{{input}}"})))
        .with_max_retries(5);
    let err = target.invoke(&case("hi")).await.unwrap_err().to_string();
    assert!(err.contains("400"), "got: {err}");
    assert!(err.contains("bad request payload"), "got: {err}");
    server.verify().await;
}

#[tokio::test]
async fn malformed_json_with_response_path_errors_with_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string("this is not json"))
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/chat", server.uri()), Method::POST)
        .with_body(Some(json!({"q": "{{input}}"})))
        .with_response_path(Some("/answer".into()));
    let err = target.invoke(&case("hi")).await.unwrap_err().to_string();
    assert!(err.contains("non-JSON"), "got: {err}");
    assert!(err.contains("200"), "got: {err}");
    assert!(err.contains("this is not json"), "got: {err}");
}

#[tokio::test]
async fn missing_pointer_names_the_pointer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"reply": "hi"})))
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/chat", server.uri()), Method::POST)
        .with_body(Some(json!({"q": "{{input}}"})))
        .with_response_path(Some("/answer".into()));
    let err = target.invoke(&case("hi")).await.unwrap_err().to_string();
    assert!(
        err.contains("/answer"),
        "error must name the pointer, got: {err}"
    );
    assert!(
        err.contains("reply"),
        "error should include the body, got: {err}"
    );
}

#[tokio::test]
async fn retries_transient_429_then_succeeds() {
    let server = MockServer::start().await;
    // First attempt 429 (retry-after: 0 keeps the test fast), then 200.
    // Mocks match in mount order; the 429 exhausts after one use.
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("slow down"),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"answer": "recovered"})))
        .expect(1)
        .mount(&server)
        .await;

    let target = HttpTarget::new(format!("{}/chat", server.uri()), Method::POST)
        .with_body(Some(json!({"q": "{{input}}"})))
        .with_response_path(Some("/answer".into()))
        .with_max_retries(1);
    let out = target.invoke(&case("hi")).await.unwrap();
    assert_eq!(out.text, "recovered");
    server.verify().await;
}

#[tokio::test]
async fn get_percent_encodes_input_into_the_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;

    // `{{input}}` in the query is percent-encoded so `&` and `=` can't alter
    // the query structure.
    let url = format!("{}/search?q={}", server.uri(), "{{input}}");
    let target = HttpTarget::new(url, Method::GET);
    let out = target.invoke(&case("a b&c=d")).await.unwrap();
    assert_eq!(out.text, "ok");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url.query().unwrap(),
        "q=a%20b%26c%3Dd",
        "the raw query must be percent-encoded"
    );
    server.verify().await;
}
