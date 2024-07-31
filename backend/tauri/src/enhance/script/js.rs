use super::runner::{wrap_result, ProcessOutput, Runner};
use crate::enhance::utils::{take_logs, Logs, LogsExt};
use anyhow::Context as _;
use async_trait::async_trait;
use boa_engine::{
    builtins::promise::PromiseState,
    js_string,
    module::{Module, ModuleLoader as BoaModuleLoader, SimpleModuleLoader},
    property::Attribute,
    Context, JsError, JsNativeError, JsValue, Source,
};
use boa_utils::{
    module::{
        http::{HttpModuleLoader, Queue},
        ModuleLoader,
    },
    Console,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_yaml::Mapping;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
use tracing_attributes::instrument;
use utils::wrap_script_if_not_esm;

use std::result::Result as StdResult;

type Result<T, E = JsRunnerError> = StdResult<T, E>;

static CUSTOM_SCRIPTS_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let path = crate::utils::dirs::app_data_dir().unwrap().join("scripts");
    if !path.exists() {
        std::fs::create_dir_all(&path).unwrap();
    }
    dunce::canonicalize(path).unwrap()
});

// define a JsRunnerError due to boa engine error is not Send
#[derive(Debug, thiserror::Error)]
pub enum JsRunnerError {
    #[error("JsError: {0}")]
    JsError(#[from] boa_engine::JsError),
    #[error("JsNativeError: {0}")]
    JsNativeError(#[from] boa_engine::JsNativeError),
    #[error("TryNativeError: {0}")]
    TryNativeError(#[from] boa_engine::error::TryNativeError),
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Other: {0}")]
    Other(String),
}

pub struct BoaConsoleLogger(Arc<Mutex<Option<Logs>>>);
impl boa_utils::Logger for BoaConsoleLogger {
    fn log(&self, msg: boa_utils::LogMessage, _: &Console) {
        match msg {
            boa_utils::LogMessage::Log(msg) => self.0.lock().as_mut().unwrap().log(msg),
            boa_utils::LogMessage::Info(msg) => self.0.lock().as_mut().unwrap().info(msg),
            boa_utils::LogMessage::Warn(msg) => self.0.lock().as_mut().unwrap().warn(msg),
            boa_utils::LogMessage::Error(msg) => self.0.lock().as_mut().unwrap().error(msg),
        }
    }
}

pub struct JSRunner;

// boa engine is single-thread runner so that we can not define it in runner trait directly
pub struct BoaRunner {
    ctx: Rc<RefCell<Context>>,
    simple_loader: Rc<SimpleModuleLoader>,
}

impl BoaRunner {
    pub fn try_new() -> Result<Self> {
        let simple_loader = Rc::new(SimpleModuleLoader::new(CUSTOM_SCRIPTS_DIR.as_path())?);
        let http_loader: Rc<dyn BoaModuleLoader> = Rc::new(HttpModuleLoader);
        let loader = Rc::new(ModuleLoader::from(vec![
            simple_loader.clone() as Rc<dyn BoaModuleLoader>,
            http_loader,
        ]));
        let queue = Rc::new(Queue::default());
        let context = Context::builder()
            .job_queue(queue)
            .module_loader(loader.clone())
            .build()?;
        Ok(Self {
            ctx: Rc::new(RefCell::new(context)),
            simple_loader,
        })
    }

    pub fn setup_console(&self, logger: BoaConsoleLogger) -> Result<()> {
        let ctx = &mut self.ctx.borrow_mut();
        // it not concurrency safe. we should move to new boa_runtime console when it is ready for custom logger
        boa_utils::set_logger(Arc::new(logger));
        let console = Console::init(ctx);
        ctx.register_global_property(js_string!(Console::NAME), console, Attribute::all())?;
        Ok(())
    }

    pub fn get_ctx(&self) -> Rc<RefCell<Context>> {
        self.ctx.clone()
    }

    /// Parse a module to prepare for execution.
    pub fn parse_module(&self, source: &str, name: &str) -> Result<Module> {
        let ctx = &mut self.ctx.borrow_mut();
        let path_name = format!("./{name}.mjs");
        let source = Source::from_reader(source.as_bytes(), Some(Path::new(&path_name)));
        // Can also pass a `Some(realm)` if you need to execute the module in another realm.
        let module = Module::parse(source, None, ctx)?;
        // Don't forget to insert the parsed module into the loader itself, since the root module
        // is not automatically inserted by the `ModuleLoader::load_imported_module` impl.
        //
        // Simulate as if the "fake" module is located in the modules root, just to ensure that
        // the loader won't double load in case someone tries to import "./main.mjs".
        self.simple_loader
            .insert(CUSTOM_SCRIPTS_DIR.join(&path_name), module.clone());
        Ok(module)
    }

    pub fn execute_module(&self, module: &Module) -> Result<()> {
        let ctx = &mut self.ctx.borrow_mut();
        let promise_result = module.load_link_evaluate(ctx);

        // Very important to push forward the job queue after queueing promises.
        ctx.run_jobs();

        // Checking if the final promise didn't return an error.
        for i in 0..20 {
            match promise_result.state() {
                PromiseState::Pending => {
                    if i == 19 {
                        return Err(JsRunnerError::Other("module didn't execute!".to_string()));
                    }
                }
                PromiseState::Fulfilled(v) => {
                    assert_eq!(v, JsValue::undefined());
                    break;
                }
                PromiseState::Rejected(err) => {
                    return Err(JsError::from_opaque(err).try_native(ctx)?.into())
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
}

#[async_trait]
impl Runner for JSRunner {
    #[instrument]
    fn try_new() -> Result<JSRunner, anyhow::Error> {
        Ok(JSRunner)
    }

    async fn process(&self, mapping: Mapping, path: &str) -> ProcessOutput {
        let content = wrap_result!(tokio::fs::read_to_string(path)
            .await
            .context("failed to read the script file"));
        self.process_honey(mapping, &content).await
    }

    async fn process_honey(&self, mapping: Mapping, script: &str) -> ProcessOutput {
        let script = wrap_script_if_not_esm(script);
        let hash = crate::utils::help::get_uid("script");
        let path = CUSTOM_SCRIPTS_DIR.join(format!("{}.mjs", hash));
        wrap_result!(tokio::fs::write(&path, script)
            .await
            .context("failed to write the script file"));
        // boa engine is single-thread runner so that we can use it in tokio::task::spawn_blocking
        let res = tokio::task::spawn_blocking(move || {
            let wrapped_fn = move || {
                let logs = Arc::new(Mutex::new(Some(Logs::new())));
                let logger = BoaConsoleLogger(logs.clone());
                let boa_runner = wrap_result!(BoaRunner::try_new(), take_logs(logs));
                wrap_result!(boa_runner.setup_console(logger), take_logs(logs));
                let config = wrap_result!(
                    simd_json::serde::to_string_pretty(&mapping)
                        .map_err(|e| { std::io::Error::new(std::io::ErrorKind::InvalidData, e) }),
                    take_logs(logs)
                );
                let execute_module = format!(
                    r#"import process from "./{hash}.mjs";
        let config = JSON.parse(`{config}`);
        export let result = JSON.stringify(await process(config));
        "#
                );
                // let process_module = wrap_result!(
                //     boa_runner.parse_module(&script, "process").map_err(|e| {
                //         logs.error(format!("failed to parse the process module: {:?}", e));
                //         e
                //     }),
                //     logs
                // );
                // wrap_result!(boa_runner.execute_module(&process_module));
                let main_module = wrap_result!(
                    boa_runner.parse_module(&execute_module, "main"),
                    take_logs(logs)
                );
                wrap_result!(boa_runner.execute_module(&main_module));
                let ctx = boa_runner.get_ctx();
                let namespace = main_module.namespace(&mut ctx.borrow_mut());
                let result = wrap_result!(
                    namespace.get(js_string!("result"), &mut ctx.borrow_mut()),
                    take_logs(logs)
                );
                let mut result = wrap_result!(
                    result
                        .as_string()
                        .ok_or_else(|| JsNativeError::typ().with_message("Expected string"))
                        .map(|str| str.to_std_string_escaped()),
                    take_logs(logs)
                );
                let mapping = wrap_result!(
                    unsafe { simd_json::serde::from_str::<Mapping>(&mut result) }
                        .map_err(|e| { std::io::Error::new(std::io::ErrorKind::InvalidData, e) }),
                    take_logs(logs)
                );
                (Ok::<Mapping, JsRunnerError>(mapping), take_logs(logs))
            };
            let (res, logs) = wrapped_fn();
            match res {
                Ok(mapping) => (Ok(mapping), logs),
                Err(e) => {
                    tracing::error!("error: {:?}", e);
                    (Err(anyhow::anyhow!("{:?}", e)), logs)
                }
            }
        })
        .await;
        let _ = tokio::fs::remove_file(&path).await;
        match res {
            Ok(output) => output,
            Err(e) => (Err(e.into()), vec![]),
        }
    }
}

mod utils {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^function\b[\s\S]*?\bmain\b").unwrap());
    pub fn wrap_script_if_not_esm(script: &str) -> String {
        let script = script.trim_matches(&[' ', '\n', '\t', '\r']);
        if !RE.is_match(script) {
            script.to_string()
        } else {
            format!("export default {}", script)
        }
    }
}

mod test {
    #[test]
    fn test_wrap_script_if_not_esm() {
        let script = r#"function main(config) {
            return config
        };"#;
        let script = super::utils::wrap_script_if_not_esm(script);
        assert_eq!(
            script,
            "export default function main(config) {\n            return config\n        };"
        );
    }

    #[test]
    fn test_process_honey() {
        use super::{super::runner::Runner, JSRunner};
        let runner = JSRunner::try_new().unwrap();
        let mapping = serde_yaml::from_str(
            r#"
        rules:
                - RULE-SET,custom-reject,REJECT
                - RULE-SET,custom-direct,DIRECT
                - RULE-SET,custom-proxy,🚀
        tun:
            enable: false
        dns:
            enable: false
        "#,
        )
        .unwrap();
        let script = r#"
        export default async function main(config) {
            if (Array.isArray(config.rules)) {
                config.rules = [...config.rules, "MATCH,🚀"];
            }
            // print(JSON.stringify(config));
            console.log("Test console log");
            console.warn("Test console log");
            console.error("Test console log");
            config.proxies = ["Test"];
            return config;
        }"#;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let (res, logs) = runner.process_honey(mapping, script).await;
                eprintln!("logs: {:?}", logs);
                let mapping = res.unwrap();
                assert_eq!(
                    mapping["rules"],
                    serde_yaml::Value::Sequence(vec![
                        serde_yaml::Value::String("RULE-SET,custom-reject,REJECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-direct,DIRECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-proxy,🚀".to_string()),
                        serde_yaml::Value::String("MATCH,🚀".to_string())
                    ])
                );
                assert_eq!(
                    mapping["proxies"],
                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                        "Test".to_string()
                    ),])
                );
                let outs = simd_json::serde::to_string(&logs).unwrap();
                assert_eq!(
                    outs,
                    r#"[["log","Test console log"],["warn","Test console log"],["error","Test console log"]]"#
                );
            });
    }

    #[test]
    fn test_process_honey_with_fetch() {
        use super::{super::runner::Runner, JSRunner};
        let runner = JSRunner::try_new().unwrap();
        let mapping = serde_yaml::from_str(
            r#"
        rules:
                - RULE-SET,custom-reject,REJECT
                - RULE-SET,custom-direct,DIRECT
                - RULE-SET,custom-proxy,🚀
        tun:
            enable: false
        dns:
            enable: false
        "#,
        )
        .unwrap();
        let script = r#"
        import YAML from 'https://esm.run/yaml@2.3.4';
        import fromAsync from 'https://esm.run/array-from-async@3.0.0';
        import { Base64 } from 'https://esm.run/js-base64@3.7.6';


        export default async function main(config) {
            const data = `
            object:
                array: ["hello", "world"]
                key: "value"
            `;

            const object = YAML.parse(data).object;

            let result = await fromAsync([
                Promise.resolve(Base64.encode(object.array[0])),
                Promise.resolve(Base64.encode(object.array[1])),
            ]);
            // add result to config.rules
            config.rules.push(`${result[0]}`);
            config.rules.push(`${result[1]}`);
            return config;
        }"#;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let (res, logs) = runner.process_honey(mapping, script).await;
                eprintln!("logs: {:?}", logs);
                let mapping = res.unwrap();
                assert_eq!(
                    mapping["rules"],
                    serde_yaml::Value::Sequence(vec![
                        serde_yaml::Value::String("RULE-SET,custom-reject,REJECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-direct,DIRECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-proxy,🚀".to_string())
                    ])
                );
                let outs = simd_json::serde::to_string(&logs).unwrap();
                assert_eq!(outs, r#"[]"#);
            });
    }
}
