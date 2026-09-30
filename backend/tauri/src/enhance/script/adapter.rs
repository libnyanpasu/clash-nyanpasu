//! ScriptRunner adapter over the legacy boa/lua runners (PR-3 T06).
//! Owns a private current-thread runtime so `run` stays synchronous and the
//! whole pipeline can execute inside spawn_blocking (roadmap §4.0.4). Do NOT
//! call it from an async context on a runtime worker thread.

use nyanpasu_config::{
    profile::ScriptRuntime,
    runtime::{
        executor::{PortError, ScriptRunner, StepLogEntry},
        value::ConfigValue,
    },
};
use tracing::Instrument;

use super::{RunnerManager, ScriptDirs, create_lua_context, ordered_map};
use crate::enhance::{ScriptType, chain::ScriptWrapper};

pub struct EnhanceScriptRunner {
    runtime: tokio::runtime::Runtime,
    dirs: ScriptDirs,
}

impl EnhanceScriptRunner {
    pub fn new(dirs: ScriptDirs) -> std::io::Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            dirs,
        })
    }
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
    fn run(
        &self,
        runtime: ScriptRuntime,
        source: &str,
        config: &ConfigValue,
        logs: &mut Vec<StepLogEntry>,
    ) -> Result<ConfigValue, PortError> {
        let script_type = match runtime {
            ScriptRuntime::JavaScript => ScriptType::JavaScript,
            ScriptRuntime::Lua => ScriptType::Lua,
        };
        let mapping = config_to_mapping(config)?;
        let wrapper = ScriptWrapper(script_type, source.to_string());
        // TODO: make `ScriptRunner` async and remove the runtime block_on here, so that the whole pipeline can be async.
        let mapping = self
            .runtime
            .block_on(
                async {
                    let mut manager = RunnerManager::new(self.dirs.clone());
                    manager.process_script(&wrapper, mapping, logs).await
                }
                .in_current_span(),
            )
            .map_err(|e| PortError::from(e.to_string()))?;
        mapping_to_config(mapping)
    }

    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError> {
        let lua = create_lua_context().map_err(|e| format!("lua context: {e}"))?;
        let item_yaml = serde_yaml::to_value(item).map_err(|e| format!("item to yaml: {e}"))?;
        let lua_item =
            ordered_map::to_lua(&lua, &item_yaml).map_err(|e| format!("item to lua: {e}"))?;
        lua.globals()
            .set("item", lua_item)
            .map_err(|e| format!("set item: {e}"))?;
        lua.load(expr)
            .set_name("=predicate")
            .eval::<bool>()
            .map_err(|e| format!("predicate eval: {e}").into())
    }

    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
        let lua = create_lua_context().map_err(|e| format!("lua context: {e}"))?;
        let item_yaml = serde_yaml::to_value(item).map_err(|e| format!("item to yaml: {e}"))?;
        let lua_item =
            ordered_map::to_lua(&lua, &item_yaml).map_err(|e| format!("item to lua: {e}"))?;
        lua.globals()
            .set("item", lua_item)
            .map_err(|e| format!("set item: {e}"))?;
        let result = lua
            .load(expr)
            .set_name("=expression")
            .eval::<mlua::Value>()
            .map_err(|e| format!("expr eval: {e}"))?;
        let yaml = ordered_map::from_lua(&lua, result).map_err(|e| format!("lua to yaml: {e}"))?;
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
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(ScriptDirs::under(dir.path())).unwrap();
        let script = r#"
function main(config) {
  console.log("hello from js");
  config["mode"] = "rule";
  return config;
}
"#;
        let mut logs = Vec::new();
        let result = runner
            .run(
                ScriptRuntime::JavaScript,
                script,
                &value("mixed-port: 7890\n"),
                &mut logs,
            )
            .expect("script should succeed");
        assert_eq!(to_yaml(&result)["mode"], serde_yaml::Value::from("rule"));
        assert!(!logs.is_empty(), "console.log must surface as step log");
    }

    #[test]
    fn failing_script_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(ScriptDirs::under(dir.path())).unwrap();
        let result = runner.run(
            ScriptRuntime::JavaScript,
            "not valid js ][",
            &value("a: 1\n"),
            &mut Vec::new(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn eval_item_errors_name_the_expression() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(ScriptDirs::under(dir.path())).unwrap();
        let item = value("name: test-node\n");
        let error = runner
            .eval_item_predicate("item.missing.field", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("predicate:1:"), "{error}");
        assert!(!error.contains(".rs:"), "{error}");
        let error = runner
            .eval_item_expr("item.missing.field", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expression:1:"), "{error}");
        assert!(!error.contains(".rs:"), "{error}");
    }

    #[test]
    fn eval_item_expr_keeps_the_item_key_order() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(ScriptDirs::under(dir.path())).unwrap();
        let item = value("name: node\ntype: ss\nserver: example.com\nport: 443\n");
        let renamed = runner
            .eval_item_expr(
                r#"(function() item.name = "renamed"; return item end)()"#,
                &item,
            )
            .unwrap();
        assert_eq!(
            serde_yaml::to_string(&to_yaml(&renamed)).unwrap(),
            "name: renamed\ntype: ss\nserver: example.com\nport: 443\n"
        );
    }

    #[test]
    fn eval_item_predicate_and_expr_use_lua_item_global() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(ScriptDirs::under(dir.path())).unwrap();
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
