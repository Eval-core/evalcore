//! Built-in scorer implementations. The `Scorer` trait itself lives in
//! `evalcore-core`; this crate turns `ScorerConfig` values into runnable
//! scorers. One scorer per file.

mod contains;
mod exact;
mod judge;
mod pattern;
mod subprocess;
mod trajectory;

pub use contains::ContainsScorer;
pub use exact::ExactScorer;
pub use judge::JudgeScorer;
pub use pattern::RegexScorer;
pub use subprocess::SubprocessScorer;
pub use trajectory::TrajectoryScorer;

use anyhow::{bail, Context};
use evalcore_config::{ScorerConfig, TargetConfig};
use evalcore_core::{Scorer, Target};

/// Build all scorers up front. Expensive or fallible construction (compiling
/// regexes, resolving judge endpoints) happens here so config mistakes
/// surface before any case runs.
///
/// `build_judge_target` turns a judge's endpoint config into a runnable
/// `Target` — callers decide the wiring (the CLI wraps judges in the
/// record/replay cache; tests pass plain `evalcore_core::build_target`).
pub fn build_scorers<F>(
    configs: &[ScorerConfig],
    build_judge_target: F,
) -> anyhow::Result<Vec<Box<dyn Scorer>>>
where
    F: Fn(&TargetConfig) -> anyhow::Result<Box<dyn Target>>,
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
                        cost: None,
                    };
                    let target = build_judge_target(&target_config)
                        .context("failed to build judge target")?;
                    Ok(Box::new(JudgeScorer::new(
                        target,
                        rubric.clone(),
                        *threshold,
                    )))
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
    use evalcore_core::build_target;

    #[test]
    fn build_scorers_rejects_invalid_regex_before_run() {
        let configs = vec![ScorerConfig::Regex {
            pattern: "(unclosed".into(),
        }];
        let err = build_scorers(&configs, build_target)
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
        let err = build_scorers(&configs, build_target)
            .err()
            .expect("threshold 1.5 must be a build error");
        assert!(err.to_string().contains("1.5"), "got: {err}");
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
        ];
        assert_eq!(build_scorers(&configs, build_target).unwrap().len(), 5);
    }
}
