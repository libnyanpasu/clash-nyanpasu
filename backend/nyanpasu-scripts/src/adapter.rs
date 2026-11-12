//! ScriptRunner adapter over the legacy boa/lua runners (PR-3 T06).
//! Owns a private current-thread runtime so `run` stays synchronous and the
//! whole pipeline can execute inside spawn_blocking (roadmap §4.0.4). Do NOT
//! call it from an async context on a runtime worker thread.

use std::sync::Arc;

use nyanpasu_config::{
    profile::ScriptRuntime,
    runtime::{
        executor::{PortError, ScriptExecutionError, ScriptOperation, ScriptRunner, StepLogEntry},
        value::ConfigValue,
    },
};
#[cfg(any(feature = "javascript", feature = "lua"))]
use tracing::Instrument;

use super::ScriptDirs;
#[cfg(any(feature = "javascript", feature = "lua"))]
use super::{RunnerManager, ScriptWrapper};
use crate::extensions::ScriptExtensionFactory;
#[cfg(feature = "lua")]
use crate::lua::{create_lua_context, ordered_map};

/// Runs config transforms synchronously using a private current-thread runtime.
/// Construct, use and drop it in a blocking context, outside async workers.
pub struct EnhanceScriptRunner {
    #[cfg(any(feature = "javascript", feature = "lua"))]
    runtime: tokio::runtime::Runtime,
    #[cfg(any(feature = "javascript", feature = "lua"))]
    dirs: ScriptDirs,
    #[cfg(any(feature = "javascript", feature = "lua"))]
    extensions: Arc<dyn ScriptExtensionFactory>,
}

impl EnhanceScriptRunner {
    /// Creates the runtime with explicit module directories and host extension factories.
    ///
    /// # Errors
    /// Returns the IO error if the current-thread runtime cannot be created.
    pub fn new(
        dirs: ScriptDirs,
        extensions: Arc<dyn ScriptExtensionFactory>,
    ) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        #[cfg(not(any(feature = "javascript", feature = "lua")))]
        let _ = (runtime, dirs, extensions);
        Ok(Self {
            #[cfg(any(feature = "javascript", feature = "lua"))]
            runtime,
            #[cfg(any(feature = "javascript", feature = "lua"))]
            dirs,
            #[cfg(any(feature = "javascript", feature = "lua"))]
            extensions,
        })
    }
}

#[cfg(any(feature = "javascript", feature = "lua"))]
fn config_to_mapping(config: &ConfigValue) -> Result<serde_yaml::Mapping, PortError> {
    let value = serde_yaml::to_value(config).map_err(|e| format!("config to yaml: {e}"))?;
    value
        .as_mapping()
        .cloned()
        .ok_or_else(|| "config is not a mapping".into())
}

#[cfg(any(feature = "javascript", feature = "lua"))]
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
        #[cfg(not(any(feature = "javascript", feature = "lua")))]
        let _ = (source, config, logs);
        match runtime {
            #[cfg(feature = "javascript")]
            ScriptRuntime::JavaScript => self.run_transform(runtime, source, config, logs),
            #[cfg(feature = "lua")]
            ScriptRuntime::Lua => self.run_transform(runtime, source, config, logs),
            #[cfg(not(feature = "javascript"))]
            ScriptRuntime::JavaScript => Err(ScriptExecutionError::StartEngine {
                runtime,
                operation: ScriptOperation::Transform,
            }
            .into()),
            #[cfg(not(feature = "lua"))]
            ScriptRuntime::Lua => Err(ScriptExecutionError::StartEngine {
                runtime,
                operation: ScriptOperation::Transform,
            }
            .into()),
        }
    }

    #[cfg(feature = "lua")]
    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError> {
        let extensions = self
            .extensions
            .lua()
            .ok_or(ScriptExecutionError::StartEngine {
                runtime: ScriptRuntime::Lua,
                operation: ScriptOperation::ItemPredicate,
            })?;
        let lua = create_lua_context(&extensions).map_err(|e| format!("lua context: {e}"))?;
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

    #[cfg(feature = "lua")]
    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
        let extensions = self
            .extensions
            .lua()
            .ok_or(ScriptExecutionError::StartEngine {
                runtime: ScriptRuntime::Lua,
                operation: ScriptOperation::ItemExpression,
            })?;
        let lua = create_lua_context(&extensions).map_err(|e| format!("lua context: {e}"))?;
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

    #[cfg(not(feature = "lua"))]
    fn eval_item_predicate(&self, _expr: &str, _item: &ConfigValue) -> Result<bool, PortError> {
        Err(ScriptExecutionError::StartEngine {
            runtime: ScriptRuntime::Lua,
            operation: ScriptOperation::ItemPredicate,
        }
        .into())
    }

    #[cfg(not(feature = "lua"))]
    fn eval_item_expr(&self, _expr: &str, _item: &ConfigValue) -> Result<ConfigValue, PortError> {
        Err(ScriptExecutionError::StartEngine {
            runtime: ScriptRuntime::Lua,
            operation: ScriptOperation::ItemExpression,
        }
        .into())
    }
}

#[cfg(any(feature = "javascript", feature = "lua"))]
impl EnhanceScriptRunner {
    fn run_transform(
        &self,
        runtime: ScriptRuntime,
        source: &str,
        config: &ConfigValue,
        logs: &mut Vec<StepLogEntry>,
    ) -> Result<ConfigValue, PortError> {
        let mapping = config_to_mapping(config)?;
        let wrapper = ScriptWrapper(runtime, source.to_string());
        // TODO: make `ScriptRunner` async and remove the runtime block_on here, so that the whole pipeline can be async.
        let mapping = self
            .runtime
            .block_on(
                async {
                    let mut manager =
                        RunnerManager::new(self.dirs.clone(), self.extensions.clone());
                    manager.process_script(&wrapper, mapping, logs).await
                }
                .in_current_span(),
            )
            .map_err(|error| match error.downcast::<ScriptExecutionError>() {
                Ok(error) => Box::new(error) as PortError,
                Err(error) => error.to_string().into(),
            })?;
        mapping_to_config(mapping)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(yaml: &str) -> ConfigValue {
        let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        ConfigValue::try_from(value).unwrap()
    }

    #[cfg(any(feature = "javascript", feature = "lua"))]
    fn to_yaml(config: &ConfigValue) -> serde_yaml::Value {
        serde_yaml::to_value(config).unwrap()
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn runs_javascript_transform_and_captures_logs() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(
            ScriptDirs::under(dir.path()),
            Arc::new(crate::test_extensions::TestExtensions),
        )
        .unwrap();
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

    #[cfg(feature = "javascript")]
    #[test]
    fn failing_script_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(
            ScriptDirs::under(dir.path()),
            Arc::new(crate::test_extensions::TestExtensions),
        )
        .unwrap();
        let result = runner.run(
            ScriptRuntime::JavaScript,
            "not valid js ][",
            &value("a: 1\n"),
            &mut Vec::new(),
        );
        assert!(result.is_err());
    }

    #[cfg(feature = "lua")]
    #[test]
    fn eval_item_errors_name_the_expression() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(
            ScriptDirs::under(dir.path()),
            Arc::new(crate::test_extensions::TestExtensions),
        )
        .unwrap();
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

    #[cfg(feature = "lua")]
    #[test]
    fn eval_item_expr_keeps_the_item_key_order() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(
            ScriptDirs::under(dir.path()),
            Arc::new(crate::test_extensions::TestExtensions),
        )
        .unwrap();
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

    #[cfg(feature = "lua")]
    #[test]
    fn eval_item_predicate_and_expr_use_lua_item_global() {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(
            ScriptDirs::under(dir.path()),
            Arc::new(crate::test_extensions::TestExtensions),
        )
        .unwrap();
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

    fn runner() -> (tempfile::TempDir, EnhanceScriptRunner) {
        let dir = tempfile::tempdir().unwrap();
        let runner = EnhanceScriptRunner::new(
            ScriptDirs::under(dir.path()),
            Arc::new(crate::test_extensions::TestExtensions),
        )
        .unwrap();
        (dir, runner)
    }

    fn assert_start_engine(error: PortError, runtime: ScriptRuntime, operation: ScriptOperation) {
        assert!(
            matches!(
                error.downcast_ref::<ScriptExecutionError>(),
                Some(ScriptExecutionError::StartEngine {
                    runtime: actual_runtime,
                    operation: actual_operation,
                }) if *actual_runtime == runtime && *actual_operation == operation
            ),
            "{error:?}"
        );
    }

    struct DecliningExtensions;

    impl ScriptExtensionFactory for DecliningExtensions {}

    #[test]
    fn declined_engine_returns_start_engine_error() {
        let dir = tempfile::tempdir().unwrap();
        let runner =
            EnhanceScriptRunner::new(ScriptDirs::under(dir.path()), Arc::new(DecliningExtensions))
                .unwrap();
        for runtime in [ScriptRuntime::JavaScript, ScriptRuntime::Lua] {
            let mut logs = Vec::new();
            assert_start_engine(
                runner
                    .run(runtime, "invalid ][", &value("{}"), &mut logs)
                    .unwrap_err(),
                runtime,
                ScriptOperation::Transform,
            );
            assert!(logs.is_empty(), "a declined engine must not run a script");
        }
    }

    #[test]
    fn declined_lua_predicate_returns_start_engine_error() {
        let dir = tempfile::tempdir().unwrap();
        let runner =
            EnhanceScriptRunner::new(ScriptDirs::under(dir.path()), Arc::new(DecliningExtensions))
                .unwrap();
        assert_start_engine(
            runner
                .eval_item_predicate("invalid ][", &value("[]"))
                .unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemPredicate,
        );
    }

    #[test]
    fn declined_lua_expression_returns_start_engine_error() {
        let dir = tempfile::tempdir().unwrap();
        let runner =
            EnhanceScriptRunner::new(ScriptDirs::under(dir.path()), Arc::new(DecliningExtensions))
                .unwrap();
        assert_start_engine(
            runner
                .eval_item_expr("invalid ][", &value("[]"))
                .unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemExpression,
        );
    }

    #[cfg(not(feature = "javascript"))]
    #[test]
    fn javascript_disabled_returns_start_engine_error() {
        // A non-mapping input would fail conversion if capability admission ran too late.
        let (_dir, runner) = runner();
        let error = runner
            .run(
                ScriptRuntime::JavaScript,
                "invalid",
                &value("[]"),
                &mut vec![],
            )
            .unwrap_err();
        assert_start_engine(error, ScriptRuntime::JavaScript, ScriptOperation::Transform);
    }

    #[cfg(not(feature = "lua"))]
    #[test]
    fn lua_disabled_returns_start_engine_error() {
        let (_dir, runner) = runner();
        let error = runner
            .run(ScriptRuntime::Lua, "invalid", &value("[]"), &mut vec![])
            .unwrap_err();
        assert_start_engine(error, ScriptRuntime::Lua, ScriptOperation::Transform);
    }

    #[cfg(not(feature = "lua"))]
    #[test]
    fn lua_disabled_predicate_returns_start_engine_error() {
        let (_dir, runner) = runner();
        assert_start_engine(
            runner
                .eval_item_predicate("invalid", &value("[]"))
                .unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemPredicate,
        );
    }

    #[cfg(not(feature = "lua"))]
    #[test]
    fn lua_disabled_expression_returns_start_engine_error() {
        let (_dir, runner) = runner();
        assert_start_engine(
            runner.eval_item_expr("invalid", &value("[]")).unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemExpression,
        );
    }

    #[cfg(any(feature = "javascript", feature = "lua"))]
    #[test]
    fn enabled_engine_errors_are_not_misclassified_as_unavailable() {
        let (_dir, runner) = runner();
        for runtime in [ScriptRuntime::JavaScript, ScriptRuntime::Lua] {
            let enabled = match runtime {
                ScriptRuntime::JavaScript => cfg!(feature = "javascript"),
                ScriptRuntime::Lua => cfg!(feature = "lua"),
            };
            if enabled {
                let error = runner
                    .run(runtime, "invalid ][", &value("{}"), &mut vec![])
                    .unwrap_err();
                assert!(
                    error.downcast_ref::<ScriptExecutionError>().is_none(),
                    "{error:?}"
                );
            }
        }
        #[cfg(feature = "lua")]
        {
            assert!(
                runner
                    .eval_item_predicate("invalid ][", &value("{}"))
                    .unwrap_err()
                    .downcast_ref::<ScriptExecutionError>()
                    .is_none()
            );
            assert!(
                runner
                    .eval_item_expr("invalid ][", &value("{}"))
                    .unwrap_err()
                    .downcast_ref::<ScriptExecutionError>()
                    .is_none()
            );
        }
    }

    #[test]
    fn disabled_engine_real_runner_reaches_pipeline() {
        use nyanpasu_config::{
            clash::config::ClashConfig,
            profile::{ManagedProfilePath, Profiles},
            runtime::executor::{
                BuiltinTransform, ExecutionTarget, GuardInputs, ProfileContentSource,
                RuntimePipelineError, RuntimePipelineInputs, TunFlavor, TunParams, TunStackWrite,
                execute,
            },
        };
        struct NoContent;
        impl ProfileContentSource for NoContent {
            fn read(&self, _: &ManagedProfilePath) -> Result<String, PortError> {
                panic!("bare pipeline reads no files")
            }
        }
        let (_dir, runner) = runner();
        let profiles = Profiles::default();
        let clash = ClashConfig::default();
        for (runtime, source, enabled) in [
            (
                ScriptRuntime::JavaScript,
                "function main(config) { config.marker = 'executed'; return config; }",
                cfg!(feature = "javascript"),
            ),
            (
                ScriptRuntime::Lua,
                "config.marker = 'executed'; return config",
                cfg!(feature = "lua"),
            ),
        ] {
            let builtins = [BuiltinTransform {
                name: "real-engine".into(),
                runtime,
                source: source.into(),
            }];
            let inputs = RuntimePipelineInputs {
                profiles: &profiles,
                target: ExecutionTarget::Bare,
                guard: GuardInputs {
                    overrides: &clash.overrides,
                    ports: Default::default(),
                    routing_mark: None,
                },
                whitelist_enabled: false,
                expand_include_all: false,
                tun: TunParams {
                    enable: false,
                    overrides: Default::default(),
                    flavor: TunFlavor::Standard {
                        stack: TunStackWrite::Omitted,
                        congestion_controller: None,
                    },
                    windows_fake_ip_filter: false,
                },
                builtin_transforms: &builtins,
            };
            let result = execute(&inputs, &NoContent, &runner);
            if enabled {
                let artifact = result.unwrap();
                assert_eq!(artifact.final_config.to_json()["marker"], "executed");
                assert!(artifact.transform_failures.is_empty());
            } else {
                let error = result.unwrap_err();
                let RuntimePipelineError::ExecuteScript { source } = error else {
                    panic!("{error:?}")
                };
                assert_start_engine(Box::new(source), runtime, ScriptOperation::Transform);
            }
        }
    }
}
