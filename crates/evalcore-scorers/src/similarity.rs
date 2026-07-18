//! Embedding cosine-similarity scorer: embeds the case's `expected` and the
//! target output through an OpenAI-compatible `/embeddings` endpoint and passes
//! iff their cosine similarity clears the threshold.
//!
//! Like the judge, the embeddings target is *injected* — the CLI wraps it in
//! the record/replay cache, so this crate never depends on `evalcore-store` and
//! replayed scores are deterministic. A case with no `expected` is a failing
//! score with a reason (failures are data), never an error.

use anyhow::Context;
use async_trait::async_trait;
use evalcore_core::{Score, Scorer, Target, TargetOutput, TestCase};

/// Gate tolerance mirroring the suite gates: a cosine that exactly meets the
/// threshold must not be failed by floating-point rounding.
const THRESHOLD_TOLERANCE: f64 = 1e-9;

pub struct SimilarityScorer {
    target: Box<dyn Target>,
    threshold: f64,
}

impl SimilarityScorer {
    pub fn new(target: Box<dyn Target>, threshold: f64) -> Self {
        Self { target, threshold }
    }

    /// Embed one text through the injected (cached) target. The target encodes
    /// the vector as a JSON array in `TargetOutput.text`; we decode it back.
    async fn embed(&self, id: &str, text: &str) -> anyhow::Result<Vec<f64>> {
        let case = TestCase {
            id: id.into(),
            input: text.into(),
            expected: None,
            trace: None,
            context: None,
        };
        let out = self
            .target
            .invoke(&case)
            .await
            .context("embedding call failed")?;
        serde_json::from_str(&out.text).with_context(|| {
            format!(
                "embedding target returned an unparseable vector: {:?}",
                crate::snippet(&out.text)
            )
        })
    }

    fn failing(&self, reason: String) -> Score {
        Score {
            scorer: self.name(),
            value: 0.0,
            passed: false,
            reason: Some(reason),
        }
    }
}

/// Cosine similarity of two equal-length vectors. `Err` carries a human reason
/// for the degenerate cases (dimension mismatch, zero-magnitude) so the scorer
/// can turn them into failing scores instead of producing NaN.
fn cosine(a: &[f64], b: &[f64]) -> Result<f64, String> {
    if a.len() != b.len() {
        return Err(format!(
            "embedding dimensions differ ({} vs {})",
            a.len(),
            b.len()
        ));
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let mag_a = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mag_b = b.iter().map(|y| y * y).sum::<f64>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return Err("a zero-magnitude embedding has no direction to compare".into());
    }
    Ok(dot / (mag_a * mag_b))
}

#[async_trait]
impl Scorer for SimilarityScorer {
    fn name(&self) -> String {
        "similarity".into()
    }

    async fn score(&self, case: &TestCase, output: &TargetOutput) -> anyhow::Result<Score> {
        // Interpret `expected` as text the same way `exact` does: a JSON string
        // embeds verbatim, anything else embeds its serialized form.
        let expected = match case.expected.as_ref() {
            None => {
                return Ok(self.failing(
                    "similarity scorer requires the case to define `expected` to embed against"
                        .into(),
                ));
            }
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        };

        let expected_vec = self
            .embed(&format!("embed-expected:{}", case.id), &expected)
            .await?;
        let output_vec = self
            .embed(&format!("embed-output:{}", case.id), &output.text)
            .await?;

        let score = match cosine(&expected_vec, &output_vec) {
            Ok(score) => score,
            Err(reason) => return Ok(self.failing(reason)),
        };

        let passed = score >= self.threshold - THRESHOLD_TOLERANCE;
        Ok(Score {
            scorer: self.name(),
            value: score,
            passed,
            reason: if passed {
                None
            } else {
                Some(format!(
                    "cosine similarity {score:.4} is below threshold {}",
                    self.threshold
                ))
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evalcore_core::embeddings::EmbeddingsTarget;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn output(text: &str) -> TargetOutput {
        TargetOutput {
            text: text.into(),
            latency_ms: 0,
            tokens: None,
            trajectory: None,
        }
    }

    fn case_with_expected(expected: Option<&str>) -> TestCase {
        TestCase {
            id: "t".into(),
            input: "q".into(),
            expected: expected.map(serde_json::Value::from),
            trace: None,
            context: None,
        }
    }

    /// A wiremock `/embeddings` endpoint returning `expected_vec` when the
    /// request body carries `expected_text` and `output_vec` for `output_text`.
    async fn similarity_backed_by(
        expected_text: &str,
        expected_vec: serde_json::Value,
        output_text: &str,
        output_vec: serde_json::Value,
        threshold: f64,
    ) -> (MockServer, SimilarityScorer) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(body_string_contains(expected_text))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": expected_vec}],
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(body_string_contains(output_text))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": output_vec}],
            })))
            .mount(&server)
            .await;
        let target =
            EmbeddingsTarget::new(format!("{}/v1", server.uri()), "embed".into(), None, 120)
                .unwrap();
        (server, SimilarityScorer::new(Box::new(target), threshold))
    }

    #[test]
    fn cosine_identical_orthogonal_opposite() {
        assert!((cosine(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]).unwrap() - 1.0).abs() < 1e-12);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).unwrap().abs() < 1e-12);
        assert!((cosine(&[1.0, 0.0], &[-1.0, 0.0]).unwrap() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn cosine_guards_zero_and_mismatched_vectors() {
        assert!(cosine(&[0.0, 0.0], &[1.0, 1.0])
            .unwrap_err()
            .contains("zero-magnitude"));
        assert!(cosine(&[1.0], &[1.0, 2.0])
            .unwrap_err()
            .contains("dimensions differ"));
    }

    #[tokio::test]
    async fn happy_path_scores_exact_cosine() {
        // expected=[1,0], output=[0,1] -> cosine 0.0, below threshold 0.5.
        let (_server, scorer) = similarity_backed_by(
            "cat",
            serde_json::json!([1.0, 0.0]),
            "dog",
            serde_json::json!([0.0, 1.0]),
            0.5,
        )
        .await;
        let score = scorer
            .score(&case_with_expected(Some("cat")), &output("dog"))
            .await
            .unwrap();
        assert!(
            (score.value - 0.0).abs() < 1e-12,
            "cosine of orthogonal is 0"
        );
        assert!(!score.passed, "0.0 < 0.5");
    }

    #[tokio::test]
    async fn threshold_boundary_passes_when_score_equals_threshold() {
        // expected=[1,0], output=[0.6,0.8] -> cosine exactly 0.6; threshold 0.6.
        let (_server, scorer) = similarity_backed_by(
            "alpha",
            serde_json::json!([1.0, 0.0]),
            "beta",
            serde_json::json!([0.6, 0.8]),
            0.6,
        )
        .await;
        let score = scorer
            .score(&case_with_expected(Some("alpha")), &output("beta"))
            .await
            .unwrap();
        assert!((score.value - 0.6).abs() < 1e-12, "got {}", score.value);
        assert!(
            score.passed,
            "score == threshold must pass (1e-9 tolerance)"
        );
    }

    #[tokio::test]
    async fn missing_expected_is_a_failing_score() {
        let (_server, scorer) = similarity_backed_by(
            "unused",
            serde_json::json!([1.0]),
            "unused2",
            serde_json::json!([1.0]),
            0.5,
        )
        .await;
        let score = scorer
            .score(&case_with_expected(None), &output("anything"))
            .await
            .unwrap();
        assert!(!score.passed);
        assert_eq!(score.value, 0.0);
        assert!(score.reason.unwrap().contains("expected"));
    }

    #[tokio::test]
    async fn zero_vector_is_a_failing_score_not_nan() {
        let (_server, scorer) = similarity_backed_by(
            "alpha",
            serde_json::json!([0.0, 0.0]),
            "beta",
            serde_json::json!([1.0, 1.0]),
            0.5,
        )
        .await;
        let score = scorer
            .score(&case_with_expected(Some("alpha")), &output("beta"))
            .await
            .unwrap();
        assert!(!score.passed);
        assert!(!score.value.is_nan(), "must not be NaN");
        assert!(score.reason.unwrap().contains("zero-magnitude"));
    }

    #[tokio::test]
    async fn non_200_from_embeddings_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        let target =
            EmbeddingsTarget::new(format!("{}/v1", server.uri()), "embed".into(), None, 120)
                .unwrap()
                .with_max_retries(0);
        let scorer = SimilarityScorer::new(Box::new(target), 0.5);
        let err = scorer
            .score(&case_with_expected(Some("alpha")), &output("beta"))
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("500"), "got: {err:#}");
    }

    #[tokio::test]
    async fn malformed_embedding_body_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"no_embedding_here": true}],
            })))
            .mount(&server)
            .await;
        let target =
            EmbeddingsTarget::new(format!("{}/v1", server.uri()), "embed".into(), None, 120)
                .unwrap();
        let scorer = SimilarityScorer::new(Box::new(target), 0.5);
        let err = scorer
            .score(&case_with_expected(Some("alpha")), &output("beta"))
            .await
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("data[0].embedding"),
            "got: {err:#}"
        );
    }
}
