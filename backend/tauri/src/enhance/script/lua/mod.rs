use std::sync::Arc;

use anyhow::Error;
use mlua::prelude::*;
use parking_lot::Mutex;
use serde_yaml::{Mapping, Value};

use crate::enhance::{runner::wrap_result, utils::take_logs, Logs, LogsExt};

use super::runner::{ProcessOutput, Runner};

pub fn create_lua_context() -> Result<Lua, anyhow::Error> {
    let lua = Lua::new();
    lua.load_from_std_lib(LuaStdLib::ALL_SAFE)?;
    Ok(lua)
}

fn create_console(lua: &Lua, logger: Arc<Mutex<Option<Logs>>>) -> Result<(), anyhow::Error> {
    let table = lua.create_table()?;
    let logger_ = logger.clone();
    let log = lua.create_function(move |_, msg: String| {
        let mut logger = logger_.lock();
        logger.as_mut().unwrap().log(msg);
        Ok(())
    })?;
    let logger_ = logger.clone();
    let info = lua.create_function(move |_, msg: String| {
        let mut logger = logger_.lock();
        logger.as_mut().unwrap().info(msg);
        Ok(())
    })?;
    let logger_ = logger.clone();
    let warn = lua.create_function(move |_, msg: String| {
        let mut logger = logger_.lock();
        logger.as_mut().unwrap().warn(msg);
        Ok(())
    })?;
    let error = lua.create_function(move |_, msg: String| {
        let mut logger = logger.lock();
        logger.as_mut().unwrap().error(msg);
        Ok(())
    })?;
    table.set("log", log)?;
    table.set("info", info)?;
    table.set("warn", warn)?;
    table.set("error", error)?;
    lua.globals().set("console", table)?;
    Ok(())
}

/// This is a workaround for mihomo's yaml config based on the index of the map.
/// We compare the keys of the index order of the original mapping with the target mapping,
/// and then we correct the order of the target mapping.
/// This is a recursive call, so it will correct the order of the nested mapping.
fn correct_original_mapping_order(target: &mut Value, original: &Value) {
    if !target.is_mapping() && !target.is_sequence() {
        return;
    }

    match (target, original) {
        (Value::Mapping(target_mapping), Value::Mapping(original_mapping)) => {
            let original_keys: Vec<_> = original_mapping.keys().collect();
            let mut new_mapping = serde_yaml::Mapping::new();

            for key in original_keys {
                if let Some(mut value) = target_mapping.remove(key) {
                    if let Some(original_value) = original_mapping.get(key) {
                        correct_original_mapping_order(&mut value, original_value);
                    }
                    new_mapping.insert(key.clone(), value);
                }
            }

            let remaining_keys = target_mapping.keys().cloned().collect::<Vec<_>>();
            for key in remaining_keys {
                if let Some(value) = target_mapping.remove(&key) {
                    new_mapping.insert(key, value);
                }
            }

            *target_mapping = new_mapping;
        }
        (Value::Sequence(target), Value::Sequence(original)) if target.len() == original.len() => {
            for (target_value, original_value) in target.iter_mut().zip(original.iter()) {
                // TODO: Maybe here exist a bug when the mappings was not in the same order
                correct_original_mapping_order(target_value, original_value);
            }
        }
        _ => {}
    }
}

pub struct LuaRunner;

#[async_trait::async_trait]
impl Runner for LuaRunner {
    fn try_new() -> Result<Self, Error> {
        Ok(Self)
    }

    async fn process(&self, mapping: Mapping, path: &str) -> ProcessOutput {
        let file = wrap_result!(tokio::fs::read_to_string(path).await);
        self.process_honey(mapping, &file).await
    }
    // TODO: Keep the order of the dictionary structure in the configuration when processing lua. Because mihomo needs ordered dictionaries for dns policy.
    async fn process_honey(&self, mapping: Mapping, script: &str) -> ProcessOutput {
        let lua = wrap_result!(create_lua_context());
        let logger = Arc::new(Mutex::new(Some(Logs::new())));
        wrap_result!(create_console(&lua, logger.clone()), take_logs(logger));
        let config = wrap_result!(
            lua.to_value(&mapping)
                .context("Failed to convert mapping to value"),
            take_logs(logger)
        );
        wrap_result!(
            lua.globals()
                .set("config", config)
                .context("Failed to set config"),
            take_logs(logger)
        );
        let output = wrap_result!(
            lua.load(script)
                .eval::<mlua::Value>()
                .context("Failed to load script"),
            take_logs(logger)
        );
        if !output.is_table() {
            return wrap_result!(
                Err(anyhow::anyhow!(
                    "Script must return a table, data: {:?}",
                    output
                )),
                take_logs(logger)
            );
        }
        let config: Mapping = wrap_result!(
            lua.from_value(output)
                .context("Failed to convert output to config"),
            take_logs(logger)
        );

        // Correct the order of the mapping
        correct_original_mapping_order(
            &mut Value::Mapping(config.clone()),
            &Value::Mapping(mapping),
        );

        (Ok(config), take_logs(logger))
    }
}

mod tests {
    #[test]
    fn test_process_honey() {
        use super::*;
        use crate::enhance::runner::Runner;
        use serde_yaml::Mapping;

        let runner = LuaRunner;
        let mapping = r#"
        proxies:
        - 123
        - 12312
        - asdxxx
        shoud_remove: 123
        "#;

        let mapping = serde_yaml::from_str::<Mapping>(mapping).unwrap();
        let script = r#"
            console.log("Hello, world!");
            console.warn("Hello, world!");
            console.error("Hello, world!");
            config["proxies"] = {1, 2, 3};
            config["shoud_remove"] = nil;
            return config;
        "#;
        let expected = r#"
        proxies:
        - 1
        - 2
        - 3
        "#;

        let (result, logs) = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(runner.process_honey(mapping, script));
        eprintln!("{:?}\n{:?}", logs, result);
        assert!(result.is_ok());
        assert_eq!(logs.len(), 3);
        let expected = serde_yaml::from_str::<Mapping>(expected).unwrap();
        assert_eq!(expected, result.unwrap());
    }

    #[test]
    // TODO: use more common test case
    fn test_correct_original_mapping_order() {
        use super::*;
        use serde_yaml::Mapping;

        let mut target = serde_yaml::from_str::<Value>(
            r#"
            proxies:
            - 123
            - 12312
            - asdxxx
            shoud_remove: 123
            "#,
        )
        .unwrap();
        let original = serde_yaml::from_str::<Value>(
            r#"
            shoud_remove: 123
            proxies:
            - 123
            - 12312
            - asdxxx
            "#,
        )
        .unwrap();
        correct_original_mapping_order(&mut target, &original);
        let expected = serde_yaml::from_str::<Value>(
            r#"
            shoud_remove: 123
            proxies:
            - 123
            - 12312
            - asdxxx
            "#,
        )
        .unwrap();
        assert_eq!(expected, target);
    }
}
