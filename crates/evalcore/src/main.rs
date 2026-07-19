//! The `evalcore` CLI.
//!
//! Exit codes: 0 = all cases passed; 1 = case failures or any error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context};
use clap::{Parser, Subcommand, ValueEnum};
use evalcore_config::{EvalConfig, GateConfig, ScorerConfig, TargetConfig};
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
        /// Matrix mode: run the whole suite against several targets and print a
        /// side-by-side comparison. Comma-separated target names (at least two,
        /// distinct, each defined). Overrides `run.matrix` in the config.
        /// `run.budget_usd` applies per arm. Mutually exclusive with `--target`,
        /// `--baseline`, and `--save-baseline`.
        #[arg(long)]
        matrix: Option<String>,
        #[arg(long, value_enum, default_value_t = Reporter::Terminal)]
        reporter: Reporter,
        /// Write the report to a file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Also write a self-contained HTML report to this path, in addition
        /// to the primary --reporter output (which is unchanged). Composes with
        /// every reporter and embeds the baseline diff when --baseline is used.
        #[arg(long)]
        html: Option<PathBuf>,
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
        /// Do not append a run-history row (overrides `run.history: true`). The
        /// eval verdict and report bytes are unaffected either way — history is
        /// metadata for `evalcore serve`.
        #[arg(long)]
        no_history: bool,
    },
    /// Serve a local, read-only web viewer over the run history in a store.
    /// Binds 127.0.0.1 only (localhost is the security model) and runs until
    /// interrupted.
    Serve {
        /// Store to read (defaults to `.evalcore/cache.db`).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Port to bind on 127.0.0.1.
        #[arg(long, default_value_t = 7878)]
        port: u16,
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
            matrix,
            reporter,
            output,
            html,
            cache,
            baseline,
            save_baseline,
            no_history,
        } => {
            run(RunArgs {
                config_path: &config,
                target_name: target.as_deref(),
                matrix: matrix.as_deref(),
                reporter,
                output_path: output.as_deref(),
                html_path: html.as_deref(),
                cache,
                baseline: baseline.as_deref(),
                save_baseline: save_baseline.as_deref(),
                no_history,
            })
            .await
        }
        Commands::Serve { store, port } => serve(store.as_deref(), port).await,
    }
}

/// Open a store and run the local read-only viewer. The store path defaults to
/// `.evalcore/cache.db`; the bind address is fixed at 127.0.0.1 inside the serve
/// crate. Runs until the process is interrupted.
async fn serve(store_path: Option<&Path>, port: u16) -> anyhow::Result<ExitCode> {
    let default_path = PathBuf::from(".evalcore/cache.db");
    let path = store_path.unwrap_or(&default_path);
    let store = Arc::new(
        Store::open(path).with_context(|| format!("failed to open store {}", path.display()))?,
    );
    println!("serving http://127.0.0.1:{port}");
    evalcore_serve::run(store, port).await?;
    Ok(ExitCode::SUCCESS)
}

struct RunArgs<'a> {
    config_path: &'a Path,
    target_name: Option<&'a str>,
    matrix: Option<&'a str>,
    reporter: Reporter,
    output_path: Option<&'a Path>,
    html_path: Option<&'a Path>,
    cache: CacheArg,
    baseline: Option<&'a str>,
    save_baseline: Option<&'a str>,
    no_history: bool,
}

/// Record a run-history row for one executed run (matrix: one call per arm),
/// reusing an already-open store or opening one at the default location. Never
/// changes the run's outcome: any failure is a stderr warning and returns.
/// Called only after gates/classification are attached, so the stored summary
/// is exactly what the viewer shows.
fn record_history(
    existing: Option<&Arc<Store>>,
    base_dir: &Path,
    config_path: &Path,
    target: &str,
    summary: &evalcore_core::RunSummary,
) {
    // The config path is stored exactly as the user gave it on the CLI.
    let config = config_path.display().to_string();
    let result = match existing {
        Some(store) => store.record_run(&config, target, summary),
        None => Store::open(&base_dir.join(".evalcore/cache.db"))
            .and_then(|store| store.record_run(&config, target, summary)),
    };
    if let Err(err) = result {
        eprintln!("warning: could not record run history: {err}");
    }
}

/// Token cost rates declared by a target's config, if any. Only the priced
/// target types (openai-compatible, trace) carry a `cost` block; others are
/// uncosted.
fn cost_rates_for(target_config: &TargetConfig) -> Option<CostRates> {
    match target_config {
        TargetConfig::OpenaiCompatible {
            cost: Some(cost), ..
        }
        | TargetConfig::Trace { cost: Some(cost) } => Some(CostRates {
            input_per_1m: cost.input_per_1m,
            output_per_1m: cost.output_per_1m,
        }),
        _ => None,
    }
}

/// Wrap a target in the record/replay cache when a cache mode and a store are
/// active and the target has a cache identity; otherwise pass it through bare
/// (shell targets, or cache off).
fn wrap_target(
    t: Box<dyn Target>,
    cache_mode: Option<CacheMode>,
    store: Option<&Arc<Store>>,
) -> Box<dyn Target> {
    if let (Some(mode), Some(store)) = (cache_mode, store) {
        if t.cache_identity().is_some() {
            return Box::new(CachedTarget::new(t, Arc::clone(store), mode));
        }
    }
    t
}

async fn run(args: RunArgs<'_>) -> anyhow::Result<ExitCode> {
    let RunArgs {
        config_path,
        target_name,
        matrix,
        reporter,
        output_path,
        html_path,
        cache,
        baseline,
        save_baseline,
        no_history,
    } = args;
    let config = EvalConfig::from_path(config_path)?;
    // Paths inside the config resolve relative to the config file itself, so
    // suites run identically from any working directory (CI, editors, make).
    let base_dir = config_path.parent().unwrap_or(Path::new("."));

    // Matrix mode: `--matrix` (a comma list) overrides `run.matrix` in the
    // config. When either is present, run the whole suite once per target and
    // print a comparison — a separate code path from the single-target run
    // below, which stays byte-identical for non-matrix runs.
    let matrix_names: Option<Vec<String>> = match matrix {
        Some(csv) => Some(
            csv.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        ),
        None => config.run.matrix.clone(),
    };
    if let Some(names) = matrix_names {
        return run_matrix(RunMatrixArgs {
            config: &config,
            config_path,
            base_dir,
            names,
            target_name,
            reporter,
            output_path,
            html_path,
            cache,
            baseline,
            save_baseline,
            no_history,
        })
        .await;
    }

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
    let scorers = build_scorers(&config.scorers, base_dir, |spec| {
        // Judge specs (TargetSpec::Chat) build exactly as before; similarity
        // specs (TargetSpec::Embeddings) build the embeddings target. Both are
        // wrapped in the record/replay cache, like the main target.
        Ok(wrap(evalcore_core::embeddings::build_scorer_target(
            spec, secrets,
        )?))
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
        trials: config.run.trials.clone(),
    };
    // Classification aggregates are computed when opted in, or implicitly when an
    // accuracy/macro_f1 gate needs them. Snapshot the cases' labels before the
    // engine consumes `cases`; the pure function pairs them with the results.
    let want_classification = config.run.classification
        || config.run.gates.iter().any(|gate| {
            matches!(
                gate,
                GateConfig::Accuracy { .. } | GateConfig::MacroF1 { .. }
            )
        });
    let classification_cases = want_classification.then(|| cases.clone());
    let mut summary = run_suite(target.as_ref(), cases, &scorers, options).await;
    // Attach classification before gates so accuracy/macro_f1 gates can read it.
    if let Some(cases) = classification_cases {
        summary.classification = Some(evalcore_core::compute_classification(
            &cases,
            &summary.results,
        ));
    }
    // Suite-level gates are evaluated in the core (wiring stays wiring): the
    // pure function computes the outcomes, which ride along in the summary for
    // reporting and, below, fold into the exit code.
    summary.gates = evalcore_core::evaluate_gates(&config.run.gates, &summary);
    // Capture the gate verdict now: the summary's gates are cleared before it
    // is persisted as a baseline (baselines are pure per-case snapshots).
    let gates_passed = summary.gates.iter().all(|g| g.passed);

    // Run history: append one row (with gates/classification attached, before
    // any reporting). A side-effect only — a failure warns on stderr and never
    // touches the verdict, exit code, or report bytes below.
    if config.run.history && !no_history {
        record_history(store.as_ref(), base_dir, config_path, &name, &summary);
    }

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
    // Retain the baseline diff so the HTML report can embed it below; the
    // terminal/stderr section is still printed exactly as before.
    let mut baseline_diff: Option<evalcore_core::BaselineDiff> = None;
    if let Some(label) = baseline {
        let store = store
            .as_ref()
            .context("--baseline requires the history store, which was not opened")?;
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
        baseline_diff = Some(diff);
    }

    // Additional HTML artifact, alongside (never replacing) the primary
    // reporter. Written before save_baseline clears the summary's gates so the
    // report keeps its gates panel; embeds the baseline diff when one exists.
    if let Some(path) = html_path {
        let rendered_html = evalcore_report::html(&summary, baseline_diff.as_ref());
        std::fs::write(path, &rendered_html)
            .with_context(|| format!("failed to write HTML report to {}", path.display()))?;
    }

    if let Some(label) = save_baseline {
        let store = store
            .as_ref()
            .context("--save-baseline requires the history store, which was not opened")?;
        // A baseline is a pure per-case snapshot; gate results and classification
        // aggregates are run-scoped, not case data, so they never enter stored
        // history (and old rows have neither field — kept byte-compatible).
        summary.gates = Vec::new();
        summary.classification = None;
        store.save_baseline(label, &summary)?;
        eprintln!(
            "saved baseline {label:?} ({}/{} passed)",
            summary.passed(),
            summary.total()
        );
    }

    // Suite gates are absolute floors, additive to whichever contract applied
    // above: with --baseline, accepted failures stay tolerated, but slipping
    // below a gate floor still fails the run even when it isn't a regression.
    if !gates_passed {
        gate_passed = false;
    }

    Ok(if gate_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

struct RunMatrixArgs<'a> {
    config: &'a EvalConfig,
    config_path: &'a Path,
    base_dir: &'a Path,
    names: Vec<String>,
    target_name: Option<&'a str>,
    reporter: Reporter,
    output_path: Option<&'a Path>,
    html_path: Option<&'a Path>,
    cache: CacheArg,
    baseline: Option<&'a str>,
    save_baseline: Option<&'a str>,
    no_history: bool,
}

/// Run the suite once per matrix target, in list order, and render the
/// comparison. Each arm is priced with its own target's cost rates;
/// `run.budget_usd` applies per arm. The exit code is 0 iff every arm satisfies
/// today's whole contract (all cases pass and every gate holds), else 1.
async fn run_matrix(args: RunMatrixArgs<'_>) -> anyhow::Result<ExitCode> {
    let RunMatrixArgs {
        config,
        config_path,
        base_dir,
        names,
        target_name,
        reporter,
        output_path,
        html_path,
        cache,
        baseline,
        save_baseline,
        no_history,
    } = args;

    // Matrix is mutually exclusive with target selection and with baselines —
    // both are per-run concepts. Reject loudly rather than silently choosing.
    if target_name.is_some() {
        bail!(
            "cannot combine --target with a matrix: a matrix already runs the suite against \
             several targets. Drop --target, or drop the matrix."
        );
    }
    if baseline.is_some() || save_baseline.is_some() {
        bail!("baselines are per-run; run targets separately with --target to baseline them");
    }
    // Validate the resolved names (CLI form; the config form is validated at
    // parse). Same message, so both surfaces report identically.
    evalcore_config::validate_matrix_names(&names, &config.targets)
        .map_err(|msg| anyhow!("{msg}"))?;

    let cache_mode = cache.mode();
    let secrets = if cache_mode == Some(CacheMode::Replay) {
        SecretPolicy::Optional
    } else {
        SecretPolicy::Require
    };

    // Build every arm's target up front (list order), so the store decision can
    // see whether any arm is cacheable before opening it.
    let mut raw_targets: Vec<(String, Box<dyn Target>)> = Vec::new();
    for name in &names {
        let target_config = config.targets.get(name).expect("validated to exist");
        let target = build_target_with(target_config, secrets)
            .with_context(|| format!("failed to build target {name:?}"))?;
        raw_targets.push((name.clone(), target));
    }

    // One shared store across arms: judges/embeddings scorers reuse the same
    // cassettes (they grade each arm's distinct output, so their prompts differ
    // per arm anyway). Baselines are rejected above, so no history is needed.
    let judge_configured = config
        .scorers
        .iter()
        .any(|s| matches!(s, ScorerConfig::Judge { .. }));
    let any_cacheable = raw_targets
        .iter()
        .any(|(_, t)| t.cache_identity().is_some());
    let store: Option<Arc<Store>> = match cache_mode {
        Some(_) if any_cacheable || judge_configured => {
            Some(Arc::new(Store::open(&base_dir.join(".evalcore/cache.db"))?))
        }
        _ => None,
    };

    let scorers = build_scorers(&config.scorers, base_dir, |spec| {
        Ok(wrap_target(
            evalcore_core::embeddings::build_scorer_target(spec, secrets)?,
            cache_mode,
            store.as_ref(),
        ))
    })?;

    let mut cases: Vec<TestCase> = Vec::new();
    for dataset in &config.datasets {
        cases.extend(load_jsonl(&base_dir.join(&dataset.file))?);
    }
    if cases.is_empty() {
        bail!("datasets contain no test cases");
    }

    let want_classification = config.run.classification
        || config.run.gates.iter().any(|gate| {
            matches!(
                gate,
                GateConfig::Accuracy { .. } | GateConfig::MacroF1 { .. }
            )
        });

    // Run each arm sequentially, in the user's list order — determinism and
    // predictable rate-limit behavior. Every arm honors today's whole contract.
    let mut arms: Vec<evalcore_core::MatrixArm> = Vec::new();
    let mut all_ok = true;
    for (name, raw) in raw_targets {
        let target = wrap_target(raw, cache_mode, store.as_ref());
        let target_config = config.targets.get(&name).expect("validated to exist");
        let options = RunOptions {
            concurrency: config.run.concurrency,
            budget_usd: config.run.budget_usd,
            cost_rates: cost_rates_for(target_config),
            trials: config.run.trials.clone(),
        };
        let classification_cases = want_classification.then(|| cases.clone());
        let mut summary = run_suite(target.as_ref(), cases.clone(), &scorers, options).await;
        if let Some(cases) = classification_cases {
            summary.classification = Some(evalcore_core::compute_classification(
                &cases,
                &summary.results,
            ));
        }
        summary.gates = evalcore_core::evaluate_gates(&config.run.gates, &summary);
        let gates_passed = summary.gates.iter().all(|g| g.passed);
        all_ok &= summary.all_passed() && gates_passed;
        // One history row per arm, target = the arm's name. Side-effect only,
        // recorded with gates attached and before rendering; a failure warns.
        if config.run.history && !no_history {
            record_history(store.as_ref(), base_dir, config_path, &name, &summary);
        }
        arms.push(evalcore_core::MatrixArm {
            target: name,
            summary,
        });
    }

    let matrix_summary = evalcore_core::MatrixSummary { arms };
    let comparison = evalcore_core::compare_arms(&matrix_summary);

    let rendered = match reporter {
        Reporter::Terminal => evalcore_report::terminal_matrix(&matrix_summary, &comparison),
        Reporter::Json => evalcore_report::json_matrix(&matrix_summary, &comparison)?,
        Reporter::Junit => evalcore_report::junit_matrix(&matrix_summary),
    };
    match output_path {
        Some(path) => {
            std::fs::write(path, &rendered)
                .with_context(|| format!("failed to write report to {}", path.display()))?;
            let passed: usize = matrix_summary.arms.iter().map(|a| a.summary.passed()).sum();
            let failed: usize = matrix_summary.arms.iter().map(|a| a.summary.failed()).sum();
            let total: usize = matrix_summary.arms.iter().map(|a| a.summary.total()).sum();
            eprintln!(
                "{passed} passed, {failed} failed, {total} total across {} targets — report written to {}",
                matrix_summary.arms.len(),
                path.display()
            );
        }
        None => print!("{rendered}"),
    }
    if let Some(path) = html_path {
        let rendered_html = evalcore_report::html_matrix(&matrix_summary, &comparison);
        std::fs::write(path, &rendered_html)
            .with_context(|| format!("failed to write HTML report to {}", path.display()))?;
    }

    Ok(if all_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
