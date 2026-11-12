//! Script capabilities required by the pipeline, independent of engine implementations.

use serde::Serialize;
use snafu::Snafu;
use specta::Type;

use crate::profile::ScriptRuntime;

/// The operation that requires a compiled script engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScriptOperation {
    Transform,
    ItemPredicate,
    ItemExpression,
}

/// A missing engine rejects the entire build, even when transforms are lenient.
#[derive(Debug, Snafu, Serialize, serde::Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScriptExecutionError {
    #[snafu(display(
        "cannot start {runtime:?} for {operation:?}: engine unavailable in this build"
    ))]
    StartEngine {
        runtime: ScriptRuntime,
        operation: ScriptOperation,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_engine_error_keeps_display_and_serialized_fields() {
        for (runtime, runtime_name) in [
            (ScriptRuntime::JavaScript, "javascript"),
            (ScriptRuntime::Lua, "lua"),
        ] {
            for (operation, operation_name) in [
                (ScriptOperation::Transform, "transform"),
                (ScriptOperation::ItemPredicate, "item_predicate"),
                (ScriptOperation::ItemExpression, "item_expression"),
            ] {
                let error = ScriptExecutionError::StartEngine { runtime, operation };
                assert_eq!(
                    error.to_string(),
                    format!(
                        "cannot start {runtime:?} for {operation:?}: engine unavailable in this build"
                    )
                );
                assert_eq!(
                    serde_json::to_value(&error).unwrap(),
                    serde_json::json!({
                        "kind": "start_engine", "runtime": runtime_name, "operation": operation_name,
                    })
                );
            }
        }
    }
}
