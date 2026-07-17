//! Domain types, the `Target` and `Scorer` traits, dataset loading, and the
//! run engine.
//!
//! Traits live here so implementation crates (`evalcore-scorers`, future
//! target crates) can depend on this one without cycles.

pub mod baseline;
pub mod dataset;
pub mod engine;
pub mod http_target;
pub mod target;
pub mod trace;
pub mod types;

pub use baseline::{compare, BaselineDiff, CaseRegression};
pub use dataset::load_jsonl;
pub use engine::{run_suite, RunOptions};
pub use http_target::HttpTarget;
pub use target::{
    build_target, build_target_with, OpenAiCompatTarget, SecretPolicy, ShellTarget, Target,
    TraceTarget,
};
pub use trace::{normalize_trace, parse_trajectory, TraceStep, Trajectory};
pub use types::{
    CaseResult, CostRates, RunSummary, Score, Scorer, TargetOutput, TestCase, TokenUsage,
};
