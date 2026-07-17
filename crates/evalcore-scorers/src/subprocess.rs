//! The any-language escape hatch: run a command, hand it the case as JSON on
//! stdin, read a verdict as JSON from stdout.
//!
//! Protocol (v0):
//!   stdin:  {"input": string, "output": string, "expected": json|null}
//!   stdout: {"score": number 0.0..=1.0, "passed"?: bool, "reason"?: string}
//! When `passed` is omitted it defaults to `score >= 0.5`. Commands MUST read
//! stdin (even if only to discard it) or they may exit before the payload is
//! written.

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

#[async_trait]
impl Scorer for SubprocessScorer {
    fn name(&self) -> String {
        "subprocess".into()
    }

    async fn score(&self, case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        let payload = serde_json::json!({
            "input": case.input,
            "output": output.text,
            "expected": case.expected,
        });

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
        }
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
