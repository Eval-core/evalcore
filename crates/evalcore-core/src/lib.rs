//! Domain types, the `Target` and `Scorer` traits, dataset loading, and the
//! run engine.
//!
//! Traits live here so implementation crates (`evalcore-scorers`, future
//! target crates) can depend on this one without cycles.

pub mod baseline;
pub mod classification;
pub mod comparison;
pub mod dataset;
pub mod embeddings;
pub mod engine;
pub mod gates;
pub mod http_target;
pub mod target;
pub mod trace;
pub mod types;

pub use baseline::{compare, BaselineDiff, CaseRegression};
pub use classification::compute_classification;
pub use comparison::{
    compare_arms, ArmStats, ComparisonCell, ComparisonRow, MatrixArm, MatrixComparison,
    MatrixSummary,
};
pub use dataset::load_jsonl;
pub use engine::{run_suite, ProgressSink, RunOptions};
pub use gates::{evaluate_gates, GateResult};
pub use http_target::HttpTarget;
pub use target::{
    build_target, build_target_with, OpenAiCompatTarget, SecretPolicy, ShellTarget, Target,
    TraceTarget,
};
pub use trace::{normalize_trace, parse_trajectory, TraceStep, Trajectory};
pub use types::{
    CaseResult, ClassMetrics, ClassificationSummary, CostRates, RunSummary, Score, Scorer,
    TargetOutput, TestCase, TokenUsage,
};
