//! LLM-as-judge scorer: grades an output against a natural-language rubric by
//! calling any OpenAI-compatible endpoint.
//!
//! The judge is just another `Target`, injected by the caller — which is how
//! judge calls get record/replay caching: the CLI wraps the judge target in
//! `CachedTarget` exactly like the main target, keyed on the full judge
//! prompt. Replayed verdicts are therefore deterministic.

use std::fmt::Write;

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
        // RAG context, when the case carries it: a clearly delimited section of
        // numbered chunks, placed before the answer so rubrics like "grounded
        // in the provided context?" have something to grade against. Empty when
        // absent — so a contextless prompt is byte-identical to before this
        // feature (a pinning test guards that), and any change to the context
        // changes the prompt, which is the judge's cache key.
        let context = case
            .context
            .as_ref()
            .map(|chunks| {
                let mut section = String::from("<context>\n");
                for (i, chunk) in chunks.iter().enumerate() {
                    // `write!` into a String never fails; number each chunk 1-based.
                    let _ = writeln!(section, "[{}] {chunk}", i + 1);
                }
                section.push_str("</context>\n");
                section
            })
            .unwrap_or_default();
        format!(
            "You are an impartial evaluation judge.\n\n\
             Rubric: {rubric}\n\n\
             <input>\n{input}\n</input>\n{expected}{context}\
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
            trace: None,
            context: None,
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
            trace: None,
            context: None,
        }
    }

    fn output(text: &str) -> TargetOutput {
        TargetOutput {
            text: text.into(),
            latency_ms: 0,
            tokens: None,
            trajectory: None,
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
        let target = OpenAiCompatTarget::new(
            format!("{}/v1", server.uri()),
            "judge-model".into(),
            None,
            120,
        )
        .unwrap();
        JudgeScorer::new(Box::new(target), "Is the answer grounded?".into(), 0.7)
    }

    /// A judge whose prompt we can inspect without any network — `prompt()` is
    /// pure, so the target is never invoked.
    fn offline_judge() -> JudgeScorer {
        let target =
            OpenAiCompatTarget::new("http://127.0.0.1:1/v1".into(), "m".into(), None, 120).unwrap();
        JudgeScorer::new(Box::new(target), "Is the answer grounded?".into(), 0.7)
    }

    #[test]
    fn contextless_prompt_is_byte_identical_to_pre_context() {
        // HARD back-compat: a case WITHOUT context must produce the exact prompt
        // it produced before the `context` feature existed, or every recorded
        // judge cassette would invalidate on upgrade. This literal is the pin.
        let prompt = offline_judge().prompt(&case("q"), &output("a"));
        assert_eq!(
            prompt,
            "You are an impartial evaluation judge.\n\n\
             Rubric: Is the answer grounded?\n\n\
             <input>\nq\n</input>\n\
             <output>\na\n</output>\n\n\
             Grade how well the output satisfies the rubric.\n\
             Respond with only a JSON object, no prose, no code fences:\n\
             {\"score\": <number between 0.0 and 1.0>, \"reason\": \"<one short sentence>\"}"
        );
    }

    #[test]
    fn context_section_is_numbered_and_precedes_the_answer() {
        let judge = offline_judge();
        let mut c = case("q");
        c.context = Some(vec!["first chunk".into(), "second chunk".into()]);
        let prompt = judge.prompt(&c, &output("a"));

        assert!(
            prompt.contains("<context>\n[1] first chunk\n[2] second chunk\n</context>\n"),
            "numbered chunks in a delimited section; got: {prompt}"
        );
        let context_at = prompt.find("<context>").unwrap();
        let answer_at = prompt.find("<output>").unwrap();
        assert!(
            context_at < answer_at,
            "context must be positioned before the answer"
        );
        // A context change re-keys the judge (its cache key is the prompt).
        assert_ne!(
            prompt,
            judge.prompt(&case("q"), &output("a")),
            "adding context must change the judge request"
        );
    }

    #[tokio::test]
    async fn with_context_request_carries_the_chunks() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("[1] retrieved chunk alpha"))
            .and(body_string_contains("[2] retrieved chunk beta"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "{\"score\": 1.0}"}}],
            })))
            .expect(1)
            .mount(&server)
            .await;

        let target = OpenAiCompatTarget::new(
            format!("{}/v1", server.uri()),
            "judge-model".into(),
            None,
            120,
        )
        .unwrap();
        let judge = JudgeScorer::new(Box::new(target), "grounded?".into(), 0.5);
        let mut c = case("q");
        c.context = Some(vec![
            "retrieved chunk alpha".into(),
            "retrieved chunk beta".into(),
        ]);
        judge.score(&c, &output("a")).await.unwrap();
        server.verify().await;
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

        let target = OpenAiCompatTarget::new(
            format!("{}/v1", server.uri()),
            "judge-model".into(),
            None,
            120,
        )
        .unwrap();
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
