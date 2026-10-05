use mlua::prelude::*;
use nyanpasu_config::runtime::executor::StepLogEntry;
use serde_yaml::{Mapping, Value};

use super::runner::{ConsoleSink, Runner};

mod console;
pub mod ordered_map;

pub fn create_lua_context() -> Result<Lua, anyhow::Error> {
    let lua = Lua::new();
    lua.load_std_libs(LuaStdLib::ALL_SAFE)?;
    ordered_map::register(&lua)?;
    Ok(lua)
}

/// Runs `script` against `mapping`. Everything the script logs goes to
/// `console`.
fn run_script(mapping: Mapping, script: &str, console: &ConsoleSink) -> anyhow::Result<Mapping> {
    let lua = create_lua_context()?;
    console::register(&lua, console)?;
    let config = ordered_map::to_lua(&lua, &Value::Mapping(mapping))
        .context("Failed to convert mapping to value")?;
    lua.globals()
        .set("config", config)
        .context("Failed to set config")?;
    // Without a name mlua labels the chunk with this Rust call site, so
    // errors and tracebacks would point at Rust source instead of the script.
    let output = lua.load(script).set_name("=script").eval::<mlua::Value>()?;
    if !output.is_table() {
        anyhow::bail!("Script must return a table, data: {:?}", output);
    }
    match ordered_map::from_lua(&lua, output).context("Failed to convert output to config")? {
        Value::Mapping(config) => Ok(config),
        _ => anyhow::bail!("Script must return a mapping, not a sequence"),
    }
}

pub struct LuaRunner;

#[async_trait::async_trait]
impl Runner for LuaRunner {
    async fn process(
        &self,
        mapping: Mapping,
        path: &str,
        logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping> {
        let file = tokio::fs::read_to_string(path).await?;
        self.process_honey(mapping, &file, logs).await
    }
    async fn process_honey(
        &self,
        mapping: Mapping,
        script: &str,
        logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping> {
        let console = ConsoleSink::default();
        let result = run_script(mapping, script, &console);
        logs.extend(console.take());
        result
    }
}

mod tests {
    #[test]
    fn test_process_honey() {
        use super::*;
        use crate::enhance::script::runner::Runner;
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

        let mut logs = Vec::new();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(runner.process_honey(mapping, script, &mut logs));
        eprintln!("{logs:?}\n{result:?}");
        assert!(result.is_ok());
        assert_eq!(logs.len(), 3);
        let expected = serde_yaml::from_str::<Mapping>(expected).unwrap();
        assert_eq!(expected, result.unwrap());
    }

    #[tokio::test]
    async fn logs_written_before_an_error_survive_the_failure() {
        use super::*;
        use crate::enhance::script::runner::Runner;
        use nyanpasu_config::runtime::executor::StepLogLevel;

        let mut logs = Vec::new();
        let result = LuaRunner
            .process_honey(
                Mapping::new(),
                r#"console.log("before"); error("boom")"#,
                &mut logs,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(logs, vec![StepLogEntry::new(StepLogLevel::Log, "before")]);
    }

    #[tokio::test]
    async fn keeps_the_key_order_of_the_config() {
        use super::*;
        use crate::enhance::script::runner::Runner;

        let config: Mapping = serde_yaml::from_str(
            "zeta: 1\nalpha: 2\ndns:\n  nameserver-policy:\n    p1: a\n    p2: b\n",
        )
        .unwrap();
        let script = r#"
            OrderedMap.insert(config.dns["nameserver-policy"], 1, "p0", "z")
            config.omega = 3
            return config
        "#;
        let result = LuaRunner
            .process_honey(config, script, &mut Vec::new())
            .await
            .unwrap();
        assert_eq!(
            serde_yaml::to_string(&result).unwrap(),
            "zeta: 1\nalpha: 2\ndns:\n  nameserver-policy:\n    p0: z\n    p1: a\n    p2: b\nomega: 3\n"
        );
    }

    #[tokio::test]
    async fn a_script_returning_a_sequence_fails() {
        use super::*;
        use crate::enhance::script::runner::Runner;

        let error = LuaRunner
            .process_honey(Mapping::new(), "return { 1, 2 }", &mut Vec::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Script must return a mapping, not a sequence"
        );
    }

    #[tokio::test]
    async fn errors_name_the_script_and_carry_the_traceback() {
        use super::*;
        use crate::enhance::script::runner::Runner;

        let script = "local function helper()\n  error('boom')\nend\nhelper()\n";
        let error = LuaRunner
            .process_honey(Mapping::new(), script, &mut Vec::new())
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("runtime error: script:2: boom"),
            "{error}"
        );
        assert!(error.contains("script:4: in main chunk"), "{error}");
        assert!(!error.contains(".rs:"), "{error}");
    }
}
