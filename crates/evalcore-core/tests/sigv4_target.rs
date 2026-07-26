//! AWS SigV4-signed Anthropic targets, against a local wiremock server.
//! Proves the two axes — auth mode and Messages dialect — combine freely, and
//! that a signature actually reaches the wire. No AWS calls, ever.

use evalcore_config::{AnthropicAuth, AnthropicDialectConfig, SigV4Config, TargetConfig};
use evalcore_core::{build_target_with, SecretPolicy, TestCase};
use wiremock::matchers::{body_partial_json, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn case(input: &str) -> TestCase {
    TestCase {
        id: "t".into(),
        input: input.into(),
        expected: None,
        trace: None,
        context: None,
    }
}

fn message(text: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
    })
}

/// Env vars unique per test: the process env is global, and cargo runs tests
/// in threads.
struct ScopedEnv(Vec<String>);

impl ScopedEnv {
    fn set(pairs: &[(&str, &str)]) -> Self {
        for (key, value) in pairs {
            std::env::set_var(key, value);
        }
        Self(pairs.iter().map(|(k, _)| (*k).to_string()).collect())
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        for key in &self.0 {
            std::env::remove_var(key);
        }
    }
}

fn sigv4_config(url: String, dialect: AnthropicDialectConfig, suffix: &str) -> TargetConfig {
    TargetConfig::Anthropic {
        url,
        model: "anthropic.claude-v1:0".into(),
        api_key_env: None,
        max_retries: 0,
        timeout_seconds: 120,
        max_tokens: 512,
        cost: None,
        system: None,
        params: None,
        auth: AnthropicAuth::Sigv4,
        dialect,
        aws: Some(SigV4Config {
            region: "us-east-1".into(),
            access_key_env: format!("EVALCORE_TEST_AK_{suffix}"),
            secret_key_env: format!("EVALCORE_TEST_SK_{suffix}"),
            session_token_env: format!("EVALCORE_TEST_ST_{suffix}"),
            service: "bedrock".into(),
        }),
    }
}

/// Asserts the request carries a well-formed SigV4 Authorization header.
fn assert_signed(request: &Request) {
    let authorization = request
        .headers
        .get("authorization")
        .expect("signed requests carry an authorization header")
        .to_str()
        .unwrap();
    assert!(
        authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIAEVALCORETEST/"),
        "got: {authorization}"
    );
    assert!(
        authorization.contains("/us-east-1/bedrock/aws4_request"),
        "signature is scoped to the configured region and service; got: {authorization}"
    );
    // Every header we send must be covered, or AWS rejects the signature.
    for signed in ["content-type", "host", "x-amz-content-sha256", "x-amz-date"] {
        assert!(
            authorization.contains(signed),
            "{signed} must be in SignedHeaders; got: {authorization}"
        );
    }
}

#[tokio::test]
async fn sigv4_signs_the_legacy_bedrock_invoke_dialect() {
    let _env = ScopedEnv::set(&[
        ("EVALCORE_TEST_AK_INVOKE", "AKIAEVALCORETEST"),
        ("EVALCORE_TEST_SK_INVOKE", "secret"),
    ]);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        // Model in the path (escaped — Bedrock ids contain `:`), not the body.
        .and(path("/model/anthropic.claude-v1%3A0/invoke"))
        .and(header_exists("x-amz-date"))
        .and(body_partial_json(serde_json::json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": 512,
            "messages": [{"role": "user", "content": "hi"}],
        })))
        .respond_with(|request: &Request| {
            assert_signed(request);
            ResponseTemplate::new(200).set_body_json(message("signed ok"))
        })
        .expect(1)
        .mount(&server)
        .await;

    let target = build_target_with(
        &sigv4_config(
            server.uri(),
            AnthropicDialectConfig::BedrockInvoke,
            "INVOKE",
        ),
        SecretPolicy::Require,
    )
    .unwrap();
    assert_eq!(target.invoke(&case("hi")).await.unwrap().text, "signed ok");
    server.verify().await;
}

#[tokio::test]
async fn sigv4_also_signs_the_plain_messages_dialect() {
    // The combination that makes auth genuinely orthogonal: the Messages-shaped
    // AWS endpoints (Bedrock Mantle, Claude Platform on AWS) are SigV4-signed
    // but speak `/messages` with the model in the body.
    let _env = ScopedEnv::set(&[
        ("EVALCORE_TEST_AK_MSG", "AKIAEVALCORETEST"),
        ("EVALCORE_TEST_SK_MSG", "secret"),
    ]);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({
            "model": "anthropic.claude-v1:0",
            "max_tokens": 512,
        })))
        .respond_with(|request: &Request| {
            assert_signed(request);
            ResponseTemplate::new(200).set_body_json(message("mantle ok"))
        })
        .expect(1)
        .mount(&server)
        .await;

    let target = build_target_with(
        &sigv4_config(server.uri(), AnthropicDialectConfig::Messages, "MSG"),
        SecretPolicy::Require,
    )
    .unwrap();
    assert_eq!(target.invoke(&case("hi")).await.unwrap().text, "mantle ok");
    server.verify().await;
}

#[tokio::test]
async fn session_token_rides_along_when_the_env_var_is_set() {
    let _env = ScopedEnv::set(&[
        ("EVALCORE_TEST_AK_TOKEN", "AKIAEVALCORETEST"),
        ("EVALCORE_TEST_SK_TOKEN", "secret"),
        ("EVALCORE_TEST_ST_TOKEN", "temporary-session-token"),
    ]);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(|request: &Request| {
            let token = request
                .headers
                .get("x-amz-security-token")
                .expect("temporary credentials send their token")
                .to_str()
                .unwrap();
            assert_eq!(token, "temporary-session-token");
            let authorization = request
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap();
            assert!(
                authorization.contains("x-amz-security-token"),
                "the token must be signed, not just sent; got: {authorization}"
            );
            ResponseTemplate::new(200).set_body_json(message("ok"))
        })
        .expect(1)
        .mount(&server)
        .await;

    let target = build_target_with(
        &sigv4_config(server.uri(), AnthropicDialectConfig::Messages, "TOKEN"),
        SecretPolicy::Require,
    )
    .unwrap();
    target.invoke(&case("hi")).await.unwrap();
    server.verify().await;
}

#[test]
fn missing_aws_credentials_fail_fast_naming_the_variable() {
    let config = sigv4_config(
        "https://bedrock-runtime.us-east-1.amazonaws.com".into(),
        AnthropicDialectConfig::BedrockInvoke,
        "ABSENT",
    );
    let err = build_target_with(&config, SecretPolicy::Require)
        .err()
        .expect("a missing AWS key must be a build error, not a run-time 403");
    assert!(
        err.to_string().contains("EVALCORE_TEST_AK_ABSENT"),
        "got: {err}"
    );
    // Replay-only runs never call AWS, so they must build without credentials.
    assert!(build_target_with(&config, SecretPolicy::Optional).is_ok());
}

#[test]
fn sigv4_credentials_never_enter_the_cache_identity() {
    let _env = ScopedEnv::set(&[
        ("EVALCORE_TEST_AK_IDENT", "AKIAEVALCORETEST"),
        ("EVALCORE_TEST_SK_IDENT", "super-secret-key"),
    ]);
    let config = sigv4_config(
        "https://bedrock-runtime.us-east-1.amazonaws.com".into(),
        AnthropicDialectConfig::BedrockInvoke,
        "IDENT",
    );
    let identity = build_target_with(&config, SecretPolicy::Require)
        .unwrap()
        .cache_identity()
        .expect("signed targets are still cacheable");
    let serialized = serde_json::to_string(&identity).unwrap();
    for secret in ["AKIAEVALCORETEST", "super-secret-key"] {
        assert!(
            !serialized.contains(secret),
            "credentials must never be persisted; got: {serialized}"
        );
    }
    // The dialect does belong in the key — it changes the path and the body.
    assert_eq!(identity["dialect"], serde_json::json!("bedrock-invoke"));
}
