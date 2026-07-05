//! Caller-implemented ports. The executor performs no IO of its own.

use crate::{
    profile::{ManagedProfilePath, ScriptRuntime},
    runtime::value::ConfigValue,
};

use super::artifact::StepLogEntry;

pub type PortError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Reads the text content of a materialized profile file.
///
/// Adapters may pre-load every needed path into a map so the whole pipeline
/// runs without blocking (spec §6.3).
pub trait ProfileContentSource {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError>;
}

/// Mirrors the legacy `ProcessOutput = (Result<Mapping>, Logs)`: logs are
/// returned even when the script fails (enhance/utils.rs:114-119 parity).
pub struct ScriptRunOutcome {
    pub result: Result<ConfigValue, PortError>,
    pub logs: Vec<StepLogEntry>,
}

/// Script execution port.
///
/// Adapter obligations (spec §6.2):
/// 1. `run` MUST return an order-stable config — mihomo's dns policy depends
///    on mapping order (legacy lua runner's `correct_original_mapping_order`).
/// 2. Temp files, module loaders and thread hops are adapter-internal.
/// 3. `eval_item_*` carry legacy `use_merge` Lua-only semantics.
/// 4. Identical inputs must produce identical replies (determinism contract).
pub trait ScriptRunner {
    fn run(&self, runtime: ScriptRuntime, source: &str, config: &ConfigValue) -> ScriptRunOutcome;

    /// Overlay `filter__` string filter / `when` predicate.
    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError>;

    /// Overlay `filter__` `when + expr` replacement value.
    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError>;
}
