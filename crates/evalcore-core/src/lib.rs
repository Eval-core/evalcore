//! Domain types, the `Target` and `Scorer` traits, dataset loading, and the
//! run engine.
//!
//! Traits live here so implementation crates (`evalcore-scorers`, future
//! target crates) can depend on this one without cycles.

pub mod dataset;
pub mod engine;
pub mod target;
pub mod types;

pub use dataset::load_jsonl;
pub use engine::run_suite;
pub use target::{
    build_target, build_target_with, OpenAiCompatTarget, SecretPolicy, ShellTarget, Target,
};
pub use types::{CaseResult, RunSummary, Score, Scorer, TargetOutput, TestCase};
