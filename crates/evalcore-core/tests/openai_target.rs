//! HTTP target tests. Never hits a real API — everything runs against a
//! local wiremock server.

use std::io::{Read, Write};

use evalcore_core::{OpenAiCompatTarget, Target, TestCase, TokenUsage};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A loopback server that returns 2xx headers immediately, promises a large body
/// via `Content-Length`, then sends only a fragment and stalls — forcing the
/// client's total timeout to fire during the body read, not the send. wiremock's
/// `set_delay` delays the whole response (headers included) and can't express
/// headers-fast/body-slow, so this hand-rolled loopback server is used instead.
/// Loopback only, no external network. Returns the base URL.
fn spawn_body_stall_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf); // drain the request enough to reply
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{\"partial\":",
            );
            let _ = stream.flush();
            // Hold open past the client's 1s budget without finishing the body.
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
    format!("http://{addr}")
}

fn case(input: &str) -> TestCase {
    TestCase {
        id: "t".into(),
        input: input.into(),
        expected: None,
        trace: None,
        context: None,
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
        120,
    )
    .unwrap();
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

    let target =
        OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 120).unwrap();
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

    let target =
        OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 120).unwrap();
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

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 120)
        .unwrap()
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

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 120)
        .unwrap()
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

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 120)
        .unwrap()
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

    let target =
        OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 120).unwrap();
    let out = target.invoke(&case("hi")).await.unwrap();
    assert_eq!(
        out.tokens,
        Some(TokenUsage {
            input: 42,
            output: 7
        })
    );
}

#[tokio::test]
async fn system_prompt_and_params_reach_the_wire() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "You are terse."},
                {"role": "user", "content": "hi"},
            ],
            "temperature": 0,
            "max_tokens": 128,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "ok"}}],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut params = serde_json::Map::new();
    params.insert("temperature".into(), serde_json::json!(0));
    params.insert("max_tokens".into(), serde_json::json!(128));
    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 120)
        .unwrap()
        .with_system(Some("You are terse.".into()))
        .with_params(Some(params));

    target.invoke(&case("hi")).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn request_timeout_fires_reported_and_is_transient() {
    let server = MockServer::start().await;
    // The response is delayed far past the 1s budget, so the attempt aborts
    // before any bytes arrive. Keeps wall time ~1s (the timeout, not the delay).
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(5))
                .set_body_json(serde_json::json!({
                    "choices": [{"message": {"role": "assistant", "content": "too late"}}],
                })),
        )
        .mount(&server)
        .await;

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 1)
        .unwrap()
        .with_max_retries(0);
    let err = target.invoke(&case("hi")).await.unwrap_err().to_string();
    assert!(err.contains("timed out after 1s"), "got: {err}");
    // The retry loop only appends "(after N attempts)" to a *transient* error;
    // a permanent one returns verbatim. So this also pins timeout = transient.
    assert!(err.contains("after 1 attempts"), "got: {err}");
}

#[tokio::test]
async fn request_timeout_is_transient_and_retried_then_succeeds() {
    let server = MockServer::start().await;
    // First attempt hangs past the 1s budget (times out); the retry gets a fast
    // 200. Both mocks must be hit — proving the timeout was retried, not fatal.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(5))
                .set_body_json(serde_json::json!({"choices": []})),
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

    let target = OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "m".into(), None, 1)
        .unwrap()
        .with_max_retries(1);
    let out = target.invoke(&case("hi")).await.unwrap();
    assert_eq!(out.text, "recovered");
    server.verify().await;
}

#[tokio::test]
async fn body_read_timeout_is_transient_not_swallowed() {
    // Headers arrive fast, then the body stalls: the total budget must fire in
    // the body read and be reported as a timeout — not swallowed into an empty
    // body that then fails as a non-JSON parse error.
    let base = spawn_body_stall_server();
    let target = OpenAiCompatTarget::new(format!("{base}/v1"), "m".into(), None, 1)
        .unwrap()
        .with_max_retries(0);
    let err = target.invoke(&case("hi")).await.unwrap_err().to_string();
    assert!(err.contains("timed out after 1s"), "got: {err}");
    // Transient-only "(after N attempts)" suffix proves it's retryable, not a
    // permanent parse failure.
    assert!(err.contains("after 1 attempts"), "got: {err}");
}
