//! Filled by later plan tasks.

use crate::runtime::value::ConfigValue;

use super::{ScriptRunner, StepLogEntry};

pub(super) fn apply_overlay(
    _document: &ConfigValue,
    current: ConfigValue,
    _runner: &dyn ScriptRunner,
    _entries: &mut Vec<StepLogEntry>,
) -> ConfigValue {
    current
}
