//! LLM-as-judge scorer: grades an output against a natural-language rubric by
//! calling any OpenAI-compatible endpoint.
//!
//! The judge is just another `Target`, injected by the caller — which is how
//! judge calls get record/replay caching: the CLI wraps the judge target in
//! `CachedTarget` exactly like the main target, keyed on the full judge
//! prompt. Replayed verdicts are therefore deterministic.

use anyhow::{bail, Context};
use async_trait::async_trait;
use evalcore_core::{Score, Scorer, Target, TargetOutput, TestCase};
use serde::Deserialize;

pub struct JudgeScorer {
    target: Box<dyn Target>,
    rubric: String,
    threshold: f64,
}

impl JudgeScorer {
    pub fn new(target: Box<dyn Target>, rubric: String, threshold: f64) -> Self {
        Self {
            target,
            rubric,
            threshold,
        }
    }

    fn prompt(&self, case: &TestCase, output: &TargetOutput) -> String {
        let expected = case
            .expected
            .as_ref()
            .map(|e| format!("\n<expected>\n{e}\n</expected>\n"))
            .unwrap_or_default();
        format!(
            "You are an impartial evaluation judge.\n\n\
             Rubric: {rubric}\n\n\
             <input>\n{input}\n</input>\n{expected}\
             <output>\n{output}\n</output>\n\n\
             Grade how well the output satisfies the rubric.\n\
             Respond with only a JSON object, no prose, no code fences:\n\
             {{\"score\": <number between 0.0 and 1.0>, \"reason\": \"<one short sentence>\"}}",
            rubric = self.rubric,
            input = case.input,
            output = output.text,
        )
    }
}

#[derive(Deserialize)]
struct Verdict {
    score: f64,
    #[serde(default)]
    reason: Option<String>,
}

/// Models sometimes wrap JSON in code fences despite instructions; strip them
/// rather than failing the case over formatting.
fn strip_fences(text: &str) -> &str {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed)
}

#[async_trait]
impl Scorer for JudgeScorer {
    fn name(&self) -> String {
        "judge".into()
    }

    async fn score(&self, case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        let judge_case = TestCase {
            id: format!("judge:{}", case.id),
            input: self.prompt(case, output),
            expected: None,
        };
        let response = self
            .target
            .invoke(&judge_case)
            .await
            .context("judge call failed")?;

        let raw = strip_fences(&response.text);
        let verdict: Verdict = serde_json::from_str(raw).with_context(|| {
            format!(
                "judge returned an unparseable verdict: {:?}",
                crate::snippet(&response.text)
            )
        })?;
        if !(0.0..=1.0).contains(&verdict.score) {
            bail!("judge returned score {} outside 0.0..=1.0", verdict.score);
        }

        Ok(Score {
            scorer: self.name(),
            value: verdict.score,
            passed: verdict.score >= self.threshold,
            reason: verdict.reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evalcore_core::OpenAiCompatTarget;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn case(input: &str) -> TestCase {
        TestCase {
            id: "t".into(),
            input: input.into(),
            expected: None,
        }
    }

    fn output(text: &str) -> TargetOutput {
        TargetOutput {
            text: text.into(),
            latency_ms: 0,
            tokens: None,
        }
    }

    async fn judge_backed_by(server: &MockServer, verdict_json: &str) -> JudgeScorer {
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": verdict_json}}],
            })))
            .mount(server)
            .await;
        let target =
            OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "judge-model".into(), None);
        JudgeScorer::new(Box::new(target), "Is the answer grounded?".into(), 0.7)
    }

    #[tokio::test]
    async fn passes_at_or_above_threshold_with_reason() {
        let server = MockServer::start().await;
        let judge = judge_backed_by(&server, r#"{"score": 0.9, "reason": "well grounded"}"#).await;

        let score = judge.score(&case("q"), &output("a")).await.unwrap();
        assert!(score.passed);
        assert_eq!(score.value, 0.9);
        assert_eq!(score.reason.as_deref(), Some("well grounded"));
    }

    #[tokio::test]
    async fn fails_below_threshold() {
        let server = MockServer::start().await;
        let judge = judge_backed_by(&server, r#"{"score": 0.4, "reason": "unsupported"}"#).await;

        let score = judge.score(&case("q"), &output("a")).await.unwrap();
        assert!(!score.passed, "0.4 < 0.7 threshold");
        assert_eq!(score.value, 0.4);
    }

    #[tokio::test]
    async fn tolerates_code_fenced_verdicts() {
        let server = MockServer::start().await;
        let judge = judge_backed_by(&server, "```json\n{\"score\": 1.0}\n```").await;

        let score = judge.score(&case("q"), &output("a")).await.unwrap();
        assert!(score.passed);
    }

    #[tokio::test]
    async fn prompt_embeds_rubric_input_and_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Is the answer grounded?"))
            .and(body_string_contains("what is the refund window"))
            .and(body_string_contains("30 days"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "{\"score\": 1.0}"}}],
            })))
            .expect(1)
            .mount(&server)
            .await;

        let target =
            OpenAiCompatTarget::new(format!("{}/v1", server.uri()), "judge-model".into(), None);
        let judge = JudgeScorer::new(Box::new(target), "Is the answer grounded?".into(), 0.5);
        judge
            .score(&case("what is the refund window"), &output("30 days"))
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn unparseable_and_out_of_range_verdicts_are_errors() {
        let server = MockServer::start().await;
        let judge = judge_backed_by(&server, "I think it deserves an 8/10!").await;
        let err = judge.score(&case("q"), &output("a")).await.unwrap_err();
        assert!(err.to_string().contains("unparseable"), "got: {err}");

        let server = MockServer::start().await;
        let judge = judge_backed_by(&server, r#"{"score": 42}"#).await;
        let err = judge.score(&case("q"), &output("a")).await.unwrap_err();
        assert!(err.to_string().contains("42"), "got: {err}");
    }
}
