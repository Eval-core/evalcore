//! Built-in scorer implementations. The `Scorer` trait itself lives in
//! `evalcore-core`; this crate turns `ScorerConfig` values into runnable
//! scorers. One scorer per file.

mod contains;
mod exact;
mod json_schema;
mod judge;
mod pattern;
mod similarity;
mod subprocess;
mod trajectory;

pub use contains::ContainsScorer;
pub use exact::ExactScorer;
pub use json_schema::JsonSchemaScorer;
pub use judge::JudgeScorer;
pub use pattern::RegexScorer;
pub use similarity::SimilarityScorer;
pub use subprocess::SubprocessScorer;
pub use trajectory::TrajectoryScorer;

use std::path::Path;

use anyhow::{bail, Context};
use evalcore_config::{ScorerConfig, TargetConfig};
use evalcore_core::embeddings::TargetSpec;
use evalcore_core::{Scorer, Target};

/// Build all scorers up front. Expensive or fallible construction (compiling
/// regexes and JSON schemas, resolving judge/embedding endpoints) happens here
/// so config mistakes surface before any case runs.
///
/// `build_scorer_target` turns a scorer's endpoint spec ([`TargetSpec`]) into a
/// runnable `Target` — callers decide the wiring (the CLI wraps these in the
/// record/replay cache; tests pass `evalcore_core::embeddings::build_scorer_target`).
/// `Chat` specs preserve the judge's historical cache identity byte-for-byte;
/// `Embeddings` specs back the `similarity` scorer.
///
/// `base_dir` is the config file's directory: file paths in scorer config
/// (the json-schema `schema:`) resolve against it, exactly like dataset files.
pub fn build_scorers<F>(
    configs: &[ScorerConfig],
    base_dir: &Path,
    build_scorer_target: F,
) -> anyhow::Result<Vec<Box<dyn Scorer>>>
where
    F: Fn(TargetSpec) -> anyhow::Result<Box<dyn Target>>,
{
    configs
        .iter()
        .map(|config| -> anyhow::Result<Box<dyn Scorer>> {
            match config {
                ScorerConfig::Contains {
                    value,
                    case_sensitive,
                } => Ok(Box::new(ContainsScorer::new(
                    value.clone(),
                    *case_sensitive,
                ))),
                ScorerConfig::Exact { value } => Ok(Box::new(ExactScorer::new(value.clone()))),
                ScorerConfig::Regex { pattern } => {
                    let scorer = RegexScorer::new(pattern)
                        .with_context(|| format!("invalid regex scorer pattern: {pattern}"))?;
                    Ok(Box::new(scorer))
                }
                ScorerConfig::Subprocess { cmd } => {
                    Ok(Box::new(SubprocessScorer::new(cmd.clone())))
                }
                ScorerConfig::Trajectory { rules } => {
                    if rules.is_empty() {
                        bail!("trajectory scorer has no rules");
                    }
                    Ok(Box::new(TrajectoryScorer::new(rules.clone())))
                }
                ScorerConfig::Judge {
                    url,
                    model,
                    rubric,
                    api_key_env,
                    threshold,
                } => {
                    if !(0.0..=1.0).contains(threshold) {
                        bail!("judge threshold {threshold} outside 0.0..=1.0");
                    }
                    let target_config = TargetConfig::OpenaiCompatible {
                        url: url.clone(),
                        model: model.clone(),
                        api_key_env: api_key_env.clone(),
                        max_retries: evalcore_config::DEFAULT_MAX_RETRIES,
                        // Judges get the default timeout in v1 (no judge-level
                        // knob); it flows to the call via build_judge_target.
                        timeout_seconds: evalcore_config::DEFAULT_TIMEOUT_SECONDS,
                        cost: None,
                        system: None,
                        params: None,
                    };
                    let target = build_scorer_target(TargetSpec::Chat(target_config))
                        .context("failed to build judge target")?;
                    Ok(Box::new(JudgeScorer::new(
                        target,
                        rubric.clone(),
                        *threshold,
                    )))
                }
                ScorerConfig::JsonSchema { schema } => {
                    // Resolve the schema path relative to the config directory
                    // (like dataset files), then read + compile it once here.
                    let scorer = JsonSchemaScorer::new(&base_dir.join(schema))?;
                    Ok(Box::new(scorer))
                }
                ScorerConfig::Similarity {
                    url,
                    model,
                    api_key_env,
                    threshold,
                } => {
                    if !(-1.0..=1.0).contains(threshold) {
                        bail!("similarity threshold {threshold} outside -1.0..=1.0");
                    }
                    // The embeddings target has no TargetConfig variant (config
                    // is frozen); it rides the same injected-closure + cache
                    // path as the judge via TargetSpec::Embeddings. Embeddings
                    // get the default timeout (no similarity-level knob in v1).
                    let target = build_scorer_target(TargetSpec::Embeddings {
                        url: url.clone(),
                        model: model.clone(),
                        api_key_env: api_key_env.clone(),
                        timeout_seconds: evalcore_config::DEFAULT_TIMEOUT_SECONDS,
                    })
                    .context("failed to build embeddings target")?;
                    Ok(Box::new(SimilarityScorer::new(target, *threshold)))
                }
            }
        })
        .collect()
}

/// Truncate output text for failure reasons so reports stay readable.
pub(crate) fn snippet(text: &str) -> String {
    const MAX: usize = 200;
    if text.chars().count() <= MAX {
        text.to_string()
    } else {
        let cut: String = text.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evalcore_core::embeddings::build_scorer_target;
    use evalcore_core::SecretPolicy;

    /// Standard test wiring: build every scorer target eagerly, requiring
    /// secrets. Construction never touches the network, so this is offline.
    fn build(spec: TargetSpec) -> anyhow::Result<Box<dyn Target>> {
        build_scorer_target(spec, SecretPolicy::Require)
    }

    #[test]
    fn build_scorers_rejects_invalid_regex_before_run() {
        let configs = vec![ScorerConfig::Regex {
            pattern: "(unclosed".into(),
        }];
        let err = build_scorers(&configs, Path::new("."), build)
            .err()
            .expect("invalid regex must be a build error");
        assert!(err.to_string().contains("(unclosed"), "got: {err}");
    }

    #[test]
    fn build_scorers_rejects_out_of_range_judge_threshold() {
        let configs = vec![ScorerConfig::Judge {
            url: "http://localhost:9/v1".into(),
            model: "m".into(),
            rubric: "r".into(),
            api_key_env: None,
            threshold: 1.5,
        }];
        let err = build_scorers(&configs, Path::new("."), build)
            .err()
            .expect("threshold 1.5 must be a build error");
        assert!(err.to_string().contains("1.5"), "got: {err}");
    }

    #[test]
    fn judge_target_inherits_default_timeout() {
        // The judge maps to a TargetSpec::Chat(openai-compatible) target built
        // by the injected closure; capture that spec to prove the default
        // timeout flows to judge calls (there is no judge-level timeout knob).
        use std::cell::Cell;
        let seen: Cell<Option<u64>> = Cell::new(None);
        let configs = vec![ScorerConfig::Judge {
            url: "http://localhost:9/v1".into(),
            model: "judge".into(),
            rubric: "grounded?".into(),
            api_key_env: None,
            threshold: 0.5,
        }];
        build_scorers(&configs, Path::new("."), |spec| {
            if let TargetSpec::Chat(TargetConfig::OpenaiCompatible {
                timeout_seconds, ..
            }) = &spec
            {
                seen.set(Some(*timeout_seconds));
            }
            build(spec)
        })
        .unwrap();
        assert_eq!(seen.get(), Some(evalcore_config::DEFAULT_TIMEOUT_SECONDS));
    }

    /// HARD back-compat pin: the judge routes through TargetSpec::Chat, whose
    /// cache identity must stay byte-for-byte the openai-compatible shape it
    /// has always had — else every recorded judge cassette invalidates.
    #[test]
    fn judge_cache_identity_is_unchanged() {
        use std::cell::RefCell;
        let captured: RefCell<Option<serde_json::Value>> = RefCell::new(None);
        let configs = vec![ScorerConfig::Judge {
            url: "http://localhost:9/v1".into(),
            model: "judge-model".into(),
            rubric: "grounded?".into(),
            api_key_env: None,
            threshold: 0.5,
        }];
        build_scorers(&configs, Path::new("."), |spec| {
            let target = build(spec)?;
            *captured.borrow_mut() = target.cache_identity();
            Ok(target)
        })
        .unwrap();
        assert_eq!(
            captured.into_inner().expect("judge target has an identity"),
            serde_json::json!({
                "type": "openai-compatible",
                "url": "http://localhost:9/v1",
                "model": "judge-model",
            })
        );
    }

    #[test]
    fn build_scorers_builds_all_variants() {
        let configs = vec![
            ScorerConfig::Contains {
                value: "x".into(),
                case_sensitive: true,
            },
            ScorerConfig::Exact { value: None },
            ScorerConfig::Regex {
                pattern: "^x".into(),
            },
            ScorerConfig::Subprocess { cmd: "cat".into() },
            ScorerConfig::Judge {
                url: "http://localhost:9/v1".into(),
                model: "judge".into(),
                rubric: "grounded?".into(),
                api_key_env: None,
                threshold: 0.5,
            },
            // Similarity construction only builds the HTTP client — no network.
            ScorerConfig::Similarity {
                url: "http://localhost:9/v1".into(),
                model: "embed".into(),
                api_key_env: None,
                threshold: 0.8,
            },
        ];
        assert_eq!(
            build_scorers(&configs, Path::new("."), build)
                .unwrap()
                .len(),
            6
        );
    }
}
