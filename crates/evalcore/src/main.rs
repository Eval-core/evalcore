//! The `evalcore` CLI.
//!
//! Exit codes: 0 = all cases passed; 1 = case failures or any error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{bail, Context};
use clap::{Parser, Subcommand, ValueEnum};
use evalcore_config::{EvalConfig, ScorerConfig, TargetConfig};
use evalcore_core::{
    build_target_with, load_jsonl, run_suite, CostRates, RunOptions, SecretPolicy, Target, TestCase,
};
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
        /// Gate on regressions against a stored baseline instead of absolute
        /// pass/fail: exit 0 iff no case regressed and no new case fails,
        /// tolerating failures already present in the baseline.
        #[arg(long)]
        baseline: Option<String>,
        /// Save this run's results as a named baseline (after comparison,
        /// when --baseline is also given — enabling rolling baselines).
        #[arg(long)]
        save_baseline: Option<String>,
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
            baseline,
            save_baseline,
        } => {
            run(RunArgs {
                config_path: &config,
                target_name: target.as_deref(),
                reporter,
                output_path: output.as_deref(),
                cache,
                baseline: baseline.as_deref(),
                save_baseline: save_baseline.as_deref(),
            })
            .await
        }
    }
}

struct RunArgs<'a> {
    config_path: &'a Path,
    target_name: Option<&'a str>,
    reporter: Reporter,
    output_path: Option<&'a Path>,
    cache: CacheArg,
    baseline: Option<&'a str>,
    save_baseline: Option<&'a str>,
}

async fn run(args: RunArgs<'_>) -> anyhow::Result<ExitCode> {
    let RunArgs {
        config_path,
        target_name,
        reporter,
        output_path,
        cache,
        baseline,
        save_baseline,
    } = args;
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
    // Baseline history lives in the same store file, so baseline flags also
    // force it open — even for shell targets that never touch the cache.
    let needs_history = baseline.is_some() || save_baseline.is_some();
    let store: Option<Arc<Store>> = match cache_mode {
        Some(_) if target.cache_identity().is_some() || judge_configured || needs_history => {
            Some(Arc::new(Store::open(&base_dir.join(".evalcore/cache.db"))?))
        }
        None if needs_history => Some(Arc::new(Store::open(&base_dir.join(".evalcore/cache.db"))?)),
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

    let cost_rates = match target_config {
        TargetConfig::OpenaiCompatible {
            cost: Some(cost), ..
        }
        | TargetConfig::Trace { cost: Some(cost) } => Some(CostRates {
            input_per_1m: cost.input_per_1m,
            output_per_1m: cost.output_per_1m,
        }),
        _ => None,
    };
    let options = RunOptions {
        concurrency: config.run.concurrency,
        budget_usd: config.run.budget_usd,
        cost_rates,
    };
    let summary = run_suite(target.as_ref(), cases, &scorers, options).await;

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

    // With --baseline the gate is "no regressions" instead of "all passed":
    // failures already accepted into the baseline don't fail CI.
    let mut gate_passed = summary.all_passed();
    if let Some(label) = baseline {
        let store = store.as_ref().expect("store opened for baseline");
        let baseline_run = store.load_baseline(label)?.with_context(|| {
            format!("no baseline {label:?} found — record one with --save-baseline {label}")
        })?;
        let diff = evalcore_core::compare(&baseline_run, &summary);
        let section = evalcore_report::baseline(&diff, label);
        // Keep machine reporters' stdout pure; the diff goes to stderr there.
        match (reporter, output_path) {
            (Reporter::Terminal, None) => print!("{section}"),
            _ => eprint!("{section}"),
        }
        gate_passed = !diff.gate_failed();
    }
    if let Some(label) = save_baseline {
        let store = store.as_ref().expect("store opened for baseline");
        store.save_baseline(label, &summary)?;
        eprintln!(
            "saved baseline {label:?} ({}/{} passed)",
            summary.passed(),
            summary.total()
        );
    }

    Ok(if gate_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
