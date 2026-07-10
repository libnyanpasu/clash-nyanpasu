//! ScriptRunner adapter over the legacy boa/lua runners (PR-3 T06).
//! Owns a private current-thread runtime so `run` stays synchronous and the
//! whole pipeline can execute inside spawn_blocking (roadmap §4.0.4). Do NOT
//! call it from an async context on a runtime worker thread.

use mlua::LuaSerdeExt as _;
use nyanpasu_config::{
    profile::ScriptRuntime,
    runtime::{
        executor::{PortError, ScriptRunOutcome, ScriptRunner, StepLogEntry, StepLogLevel},
        value::ConfigValue,
    },
};

use super::{RunnerManager, create_lua_context};
use crate::enhance::{ScriptType, chain::ScriptWrapper, utils::LogSpan};

pub struct EnhanceScriptRunner {
    runtime: tokio::runtime::Runtime,
}

impl EnhanceScriptRunner {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        })
    }
}

fn to_step_logs(logs: Vec<(LogSpan, String)>) -> Vec<StepLogEntry> {
    logs.into_iter()
        .map(|(span, message)| {
            let level = match span {
                LogSpan::Log => StepLogLevel::Log,
                LogSpan::Info => StepLogLevel::Info,
                LogSpan::Warn => StepLogLevel::Warn,
                LogSpan::Error => StepLogLevel::Error,
            };
            StepLogEntry::new(level, message)
        })
        .collect()
}

fn config_to_mapping(config: &ConfigValue) -> Result<serde_yaml::Mapping, PortError> {
    let value = serde_yaml::to_value(config).map_err(|e| format!("config to yaml: {e}"))?;
    value
        .as_mapping()
        .cloned()
        .ok_or_else(|| "config is not a mapping".into())
}

fn mapping_to_config(mapping: serde_yaml::Mapping) -> Result<ConfigValue, PortError> {
    ConfigValue::try_from(serde_yaml::Value::Mapping(mapping))
        .map_err(|e| format!("yaml to config: {e:?}").into())
}

impl ScriptRunner for EnhanceScriptRunner {
    fn run(&self, runtime: ScriptRuntime, source: &str, config: &ConfigValue) -> ScriptRunOutcome {
        let script_type = match runtime {
            ScriptRuntime::JavaScript => ScriptType::JavaScript,
            ScriptRuntime::Lua => ScriptType::Lua,
        };
        let mapping = match config_to_mapping(config) {
            Ok(mapping) => mapping,
            Err(error) => {
                return ScriptRunOutcome {
                    result: Err(error),
                    logs: Vec::new(),
                };
            }
        };
        let wrapper = ScriptWrapper(script_type, source.to_string());
        let (result, logs) = self.runtime.block_on(async {
            let mut manager = RunnerManager::new();
            manager.process_script(&wrapper, mapping).await
        });
        ScriptRunOutcome {
            result: result
                .map_err(|e| PortError::from(e.to_string()))
                .and_then(mapping_to_config),
            logs: to_step_logs(logs),
        }
    }

    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError> {
        let lua = create_lua_context().map_err(|e| format!("lua context: {e}"))?;
        let item_yaml = serde_yaml::to_value(item).map_err(|e| format!("item to yaml: {e}"))?;
        let lua_item = lua
            .to_value(&item_yaml)
            .map_err(|e| format!("item to lua: {e}"))?;
        lua.globals()
            .set("item", lua_item)
            .map_err(|e| format!("set item: {e}"))?;
        lua.load(expr)
            .eval::<bool>()
            .map_err(|e| format!("predicate eval: {e}").into())
    }

    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
        let lua = create_lua_context().map_err(|e| format!("lua context: {e}"))?;
        let item_yaml = serde_yaml::to_value(item).map_err(|e| format!("item to yaml: {e}"))?;
        let lua_item = lua
            .to_value(&item_yaml)
            .map_err(|e| format!("item to lua: {e}"))?;
        lua.globals()
            .set("item", lua_item)
            .map_err(|e| format!("set item: {e}"))?;
        let result = lua
            .load(expr)
            .eval::<mlua::Value>()
            .map_err(|e| format!("expr eval: {e}"))?;
        let yaml: serde_yaml::Value = lua
            .from_value(result)
            .map_err(|e| format!("lua to yaml: {e}"))?;
        ConfigValue::try_from(yaml).map_err(|e| format!("yaml to config: {e:?}").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(yaml: &str) -> ConfigValue {
        let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        ConfigValue::try_from(value).unwrap()
    }

    fn to_yaml(config: &ConfigValue) -> serde_yaml::Value {
        serde_yaml::to_value(config).unwrap()
    }

    #[test]
    fn runs_javascript_transform_and_captures_logs() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let script = r#"
function main(config) {
  console.log("hello from js");
  config["mode"] = "rule";
  return config;
}
"#;
        let outcome = runner.run(
            ScriptRuntime::JavaScript,
            script,
            &value("mixed-port: 7890\n"),
        );
        let result = outcome.result.expect("script should succeed");
        assert_eq!(to_yaml(&result)["mode"], serde_yaml::Value::from("rule"));
        assert!(
            !outcome.logs.is_empty(),
            "console.log must surface as step log"
        );
    }

    #[test]
    fn failing_script_returns_error() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let outcome = runner.run(
            ScriptRuntime::JavaScript,
            "not valid js ][",
            &value("a: 1\n"),
        );
        assert!(outcome.result.is_err());
    }

    #[test]
    fn eval_item_predicate_and_expr_use_lua_item_global() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let item = value("name: test-node\ntype: ss\n");
        assert!(
            runner
                .eval_item_predicate(r#"item.name == "test-node""#, &item)
                .unwrap()
        );
        assert!(
            !runner
                .eval_item_predicate(r#"item.name == "other""#, &item)
                .unwrap()
        );
        let replaced = runner
            .eval_item_expr(
                r#"(function() item.name = "renamed"; return item end)()"#,
                &item,
            )
            .unwrap();
        assert_eq!(
            to_yaml(&replaced)["name"],
            serde_yaml::Value::from("renamed")
        );
    }
}
