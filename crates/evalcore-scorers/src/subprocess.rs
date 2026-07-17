//! The any-language escape hatch: run a command, hand it the case as JSON on
//! stdin, read a verdict as JSON from stdout.
//!
//! Protocol (v0):
//!   stdin:  {"input": string, "output": string, "expected": json|null,
//!            "context"?: array of strings}
//!   stdout: {"score": number 0.0..=1.0, "passed"?: bool, "reason"?: string}
//! `context` (the case's RAG chunks) is present ONLY when the case carries it —
//! omitted entirely otherwise, so scorers that don't need it see the original
//! payload shape. When `passed` is omitted it defaults to `score >= 0.5`.
//! Commands MUST read stdin (even if only to discard it) or they may exit
//! before the payload is written.

use std::process::Stdio;

use anyhow::{bail, Context};
use async_trait::async_trait;
use evalcore_core::{Score, Scorer, TargetOutput, TestCase};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

pub struct SubprocessScorer {
    cmd: String,
}

impl SubprocessScorer {
    pub fn new(cmd: String) -> Self {
        Self { cmd }
    }
}

#[derive(Deserialize)]
struct Verdict {
    score: f64,
    #[serde(default)]
    passed: Option<bool>,
    #[serde(default)]
    reason: Option<String>,
}

/// Build the stdin payload for a case. `context` is added only when the case
/// carries it, so a contextless case's payload keeps its original byte-shape.
fn build_payload(case: &TestCase, output: &TargetOutput) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "input": case.input,
        "output": output.text,
        "expected": case.expected,
    });
    if let Some(context) = &case.context {
        payload["context"] = serde_json::json!(context);
    }
    payload
}

#[async_trait]
impl Scorer for SubprocessScorer {
    fn name(&self) -> String {
        "subprocess".into()
    }

    async fn score(&self, case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        let payload = build_payload(case, output);

        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&self.cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn subprocess scorer: {}", self.cmd))?;

        // Write then drop stdin so the child sees EOF before we wait —
        // holding it open would deadlock commands that read to EOF.
        {
            let mut stdin = child
                .stdin
                .take()
                .context("subprocess scorer has no stdin")?;
            stdin
                .write_all(payload.to_string().as_bytes())
                .await
                .with_context(|| {
                    format!(
                        "failed writing to scorer {:?} — scorer commands must read stdin",
                        self.cmd
                    )
                })?;
            stdin.shutdown().await?;
        }

        let result = child.wait_with_output().await?;
        if !result.status.success() {
            bail!(
                "scorer command exited with {}: {}",
                result.status,
                String::from_utf8_lossy(&result.stderr).trim()
            );
        }

        let stdout = String::from_utf8_lossy(&result.stdout);
        let verdict: Verdict = serde_json::from_str(stdout.trim()).with_context(|| {
            format!(
                "scorer {:?} printed invalid verdict JSON: {:?}",
                self.cmd,
                stdout.trim()
            )
        })?;

        if !(0.0..=1.0).contains(&verdict.score) {
            bail!(
                "scorer {:?} returned score {} outside 0.0..=1.0",
                self.cmd,
                verdict.score
            );
        }

        let passed = verdict.passed.unwrap_or(verdict.score >= 0.5);
        Ok(Score {
            scorer: self.name(),
            value: verdict.score,
            passed,
            reason: verdict.reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(text: &str) -> TargetOutput {
        TargetOutput {
            text: text.into(),
            latency_ms: 0,
            tokens: None,
            trajectory: None,
        }
    }

    fn case() -> TestCase {
        TestCase {
            id: "t".into(),
            input: "the input".into(),
            expected: Some(serde_json::json!("the expectation")),
            trace: None,
            context: None,
        }
    }

    #[test]
    fn payload_omits_context_when_absent() {
        // Byte-shape pin: a contextless case must not gain a `context` key.
        let payload = build_payload(&case(), &output("hi"));
        assert_eq!(
            payload.to_string(),
            r#"{"expected":"the expectation","input":"the input","output":"hi"}"#
        );
    }

    #[test]
    fn payload_includes_context_when_present() {
        let mut c = case();
        c.context = Some(vec!["chunk a".into(), "chunk b".into()]);
        let payload = build_payload(&c, &output("hi"));
        assert_eq!(
            payload.to_string(),
            r#"{"context":["chunk a","chunk b"],"expected":"the expectation","input":"the input","output":"hi"}"#
        );
    }

    #[tokio::test]
    async fn scorer_receives_context_on_stdin_when_present() {
        // The command must read stdin fully (`cat`) before grepping it, or it
        // may exit before the payload is written.
        let scorer = SubprocessScorer::new(
            r#"body=$(cat); echo "$body" | grep -q "grounding chunk" && printf '{"score": 1.0}' || printf '{"score": 0.0}'"#
                .into(),
        );
        let mut with_ctx = case();
        with_ctx.context = Some(vec!["grounding chunk".into()]);
        let score = scorer.score(&with_ctx, &output("hi")).await.unwrap();
        assert!(
            score.passed,
            "context chunk should reach the scorer's stdin"
        );

        // The same command sees no context key for a contextless case.
        let score = scorer.score(&case(), &output("hi")).await.unwrap();
        assert!(!score.passed, "contextless payload carries no context");
    }

    #[tokio::test]
    async fn parses_verdict_and_defaults_passed_from_score() {
        let scorer = SubprocessScorer::new(r#"cat >/dev/null; printf '{"score": 0.9}'"#.into());
        let score = scorer.score(&case(), &output("hi")).await.unwrap();
        assert_eq!(score.value, 0.9);
        assert!(score.passed, "0.9 >= 0.5 should default to passed");
    }

    #[tokio::test]
    async fn explicit_passed_overrides_threshold() {
        let scorer = SubprocessScorer::new(
            r#"cat >/dev/null; printf '{"score": 0.9, "passed": false, "reason": "vibes off"}'"#
                .into(),
        );
        let score = scorer.score(&case(), &output("hi")).await.unwrap();
        assert!(!score.passed);
        assert_eq!(score.reason.as_deref(), Some("vibes off"));
    }

    #[tokio::test]
    async fn scorer_receives_the_payload_on_stdin() {
        // jq-free payload check: grep stdin for the input text.
        let scorer = SubprocessScorer::new(
            r#"grep -q "the input" && printf '{"score": 1.0}' || printf '{"score": 0.0}'"#.into(),
        );
        let score = scorer.score(&case(), &output("hi")).await.unwrap();
        assert!(score.passed, "payload should contain the case input");
    }

    #[tokio::test]
    async fn nonzero_exit_surfaces_stderr() {
        let scorer = SubprocessScorer::new(r#"cat >/dev/null; echo bad config >&2; exit 2"#.into());
        let err = scorer.score(&case(), &output("hi")).await.unwrap_err();
        assert!(err.to_string().contains("bad config"), "got: {err}");
    }

    #[tokio::test]
    async fn invalid_json_and_out_of_range_scores_are_errors() {
        let scorer = SubprocessScorer::new(r#"cat >/dev/null; printf 'not json'"#.into());
        assert!(scorer.score(&case(), &output("hi")).await.is_err());

        let scorer = SubprocessScorer::new(r#"cat >/dev/null; printf '{"score": 3.5}'"#.into());
        let err = scorer.score(&case(), &output("hi")).await.unwrap_err();
        assert!(err.to_string().contains("3.5"), "got: {err}");
    }
}
