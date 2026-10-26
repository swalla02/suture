//! RuleArena's airline bag fees, the first stage of the rule-decisions
//! experiment (docs/superpowers/specs/2026-10-26-rule-decisions-experiment.md).

pub mod data;
pub mod reference;
pub mod split;
pub mod sutured;

/// `eval/rules/airline/data`, from this crate.
pub fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")
}
