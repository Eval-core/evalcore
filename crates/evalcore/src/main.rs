//! The `evalcore` CLI.
//!
//! Exit codes: 0 = all cases passed; 1 = case failures or any error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{bail, Context};
use clap::{Parser, Subcommand, ValueEnum};
use evalcore_config::{EvalConfig, ScorerConfig};
use evalcore_core::{build_target_with, load_jsonl, run_suite, SecretPolicy, Target, TestCase};
use evalcore_scorers::build_scorers;
use evalcore_store::{CacheMode, CachedTarget, Store};

#[derive(Parser)]
#[command(name = "evalcore", version, about = "Snapshot testing for AI behavior")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse and validate an evals.yaml without running anything.
    Validate { config: PathBuf },
    /// Run an eval suite.
    Run {
        config: PathBuf,
        /// Target to run (defaults to the only target when exactly one is defined).
        #[arg(long)]
        target: Option<String>,
        #[arg(long, value_enum, default_value_t = Reporter::Terminal)]
        reporter: Reporter,
        /// Write the report to a file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Record/replay cache mode for cacheable targets (LLM APIs).
        /// auto: replay hits, record misses. replay: miss = failure (CI).
        /// live: always call and re-record. off: bypass entirely.
        #[arg(long, value_enum, default_value_t = CacheArg::Auto)]
        cache: CacheArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Reporter {
    Terminal,
    Json,
    Junit,
}

#[derive(Clone, Copy, ValueEnum)]
enum CacheArg {
    Auto,
    Replay,
    Live,
    Off,
}

impl CacheArg {
    fn mode(self) -> Option<CacheMode> {
        match self {
            CacheArg::Auto => Some(CacheMode::Auto),
            CacheArg::Replay => Some(CacheMode::Replay),
            CacheArg::Live => Some(CacheMode::Live),
            CacheArg::Off => None,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    match Cli::parse().command {
        Commands::Validate { config } => {
            let parsed = EvalConfig::from_path(&config)?;
            println!(
                "OK: {} target(s), {} dataset(s), {} scorer(s)",
                parsed.targets.len(),
                parsed.datasets.len(),
                parsed.scorers.len()
            );
            Ok(ExitCode::SUCCESS)
        }
        Commands::Run {
            config,
            target,
            reporter,
            output,
            cache,
        } => {
            run(
                &config,
                target.as_deref(),
                reporter,
                output.as_deref(),
                cache,
            )
            .await
        }
    }
}

async fn run(
    config_path: &Path,
    target_name: Option<&str>,
    reporter: Reporter,
    output_path: Option<&Path>,
    cache: CacheArg,
) -> anyhow::Result<ExitCode> {
    let config = EvalConfig::from_path(config_path)?;
    // Paths inside the config resolve relative to the config file itself, so
    // suites run identically from any working directory (CI, editors, make).
    let base_dir = config_path.parent().unwrap_or(Path::new("."));

    let (name, target_config) = match target_name {
        Some(name) => {
            let target_config = config.targets.get(name).with_context(|| {
                format!(
                    "target {name:?} not found; available: {}",
                    config
                        .targets
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            (name.to_string(), target_config)
        }
        None => {
            if config.targets.len() > 1 {
                bail!(
                    "multiple targets defined; pass --target <name> (available: {})",
                    config
                        .targets
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            let (name, target_config) = config.targets.iter().next().expect("validated non-empty");
            (name.clone(), target_config)
        }
    };

    // Replay-only runs never call the network, so missing API keys are fine —
    // that's what lets CI replay a committed cache with no secrets configured.
    let cache_mode = cache.mode();
    let secrets = if cache_mode == Some(CacheMode::Replay) {
        SecretPolicy::Optional
    } else {
        SecretPolicy::Require
    };
    let target = build_target_with(target_config, secrets)
        .with_context(|| format!("failed to build target {name:?}"))?;

    // One shared store: the main target and every LLM judge record into the
    // same cache file. Uncacheable targets (shell) stay bare, so no
    // .evalcore/ directory appears for purely local runs.
    let judge_configured = config
        .scorers
        .iter()
        .any(|s| matches!(s, ScorerConfig::Judge { .. }));
    let store: Option<Arc<Store>> = match cache_mode {
        Some(_) if target.cache_identity().is_some() || judge_configured => {
            Some(Arc::new(Store::open(&base_dir.join(".evalcore/cache.db"))?))
        }
        _ => None,
    };
    let wrap = |t: Box<dyn Target>| -> Box<dyn Target> {
        if let (Some(mode), Some(store)) = (cache_mode, store.as_ref()) {
            if t.cache_identity().is_some() {
                return Box::new(CachedTarget::new(t, Arc::clone(store), mode));
            }
        }
        t
    };
    let target = wrap(target);
    let scorers = build_scorers(&config.scorers, |cfg| {
        Ok(wrap(build_target_with(cfg, secrets)?))
    })?;

    let mut cases: Vec<TestCase> = Vec::new();
    for dataset in &config.datasets {
        cases.extend(load_jsonl(&base_dir.join(&dataset.file))?);
    }
    if cases.is_empty() {
        bail!("datasets contain no test cases");
    }

    let summary = run_suite(target.as_ref(), cases, &scorers, config.run.concurrency).await;

    let rendered = match reporter {
        Reporter::Terminal => evalcore_report::terminal(&summary),
        Reporter::Json => evalcore_report::json(&summary)?,
        Reporter::Junit => evalcore_report::junit(&summary),
    };
    match output_path {
        Some(path) => {
            std::fs::write(path, &rendered)
                .with_context(|| format!("failed to write report to {}", path.display()))?;
            eprintln!(
                "{} passed, {} failed, {} total — report written to {}",
                summary.passed(),
                summary.failed(),
                summary.total(),
                path.display()
            );
        }
        None => print!("{rendered}"),
    }

    Ok(if summary.all_passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
