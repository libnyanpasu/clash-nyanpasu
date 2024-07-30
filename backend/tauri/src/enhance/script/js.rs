use super::runner::{wrap_result, ProcessOutput, Runner};
use crate::enhance::{Logs, LogsExt};
use anyhow::Context as _;
use async_trait::async_trait;
use boa_engine::{
    builtins::promise::PromiseState,
    js_string,
    module::{Module, SimpleModuleLoader},
    Context, JsError, JsNativeError, JsValue, NativeFunction, Source,
};
use once_cell::sync::Lazy;
use serde_yaml::Mapping;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
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

pub struct JSRunner;

// boa engine is single-thread runner so that we can not define it in runner trait directly
pub struct BoaRunner {
    ctx: Rc<RefCell<Context>>,
    loader: Rc<SimpleModuleLoader>,
}

impl BoaRunner {
    pub fn try_new() -> Result<Self> {
        let loader = Rc::new(SimpleModuleLoader::new(CUSTOM_SCRIPTS_DIR.as_path())?);
        let context = Context::builder().module_loader(loader.clone()).build()?;
        Ok(Self {
            ctx: Rc::new(RefCell::new(context)),
            loader,
        })
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
        self.loader
            .insert(CUSTOM_SCRIPTS_DIR.join(&path_name), module.clone());
        Ok(module)
    }

    pub fn execute_module(&self, module: &Module) -> Result<()> {
        let ctx = &mut self.ctx.borrow_mut();
        // The lifecycle of the module is tracked using promises which can be a bit cumbersome to use.
        // If you just want to directly execute a module, you can use the `Module::load_link_evaluate`
        // method to skip all the boilerplate.
        // This does the full version for demonstration purposes.
        //
        // parse -> load -> link -> evaluate
        let promise_result = module
            // Initial load that recursively loads the module's dependencies.
            // This returns a `JsPromise` that will be resolved when loading finishes,
            // which allows async loads and async fetches.
            .load(ctx)
            .then(
                Some(
                    NativeFunction::from_copy_closure_with_captures(
                        |_, _, module, context| {
                            // After loading, link all modules by resolving the imports
                            // and exports on the full module graph, initializing module
                            // environments. This returns a plain `Err` since all modules
                            // must link at the same time.
                            module.link(context)?;
                            Ok(JsValue::undefined())
                        },
                        module.clone(),
                    )
                    .to_js_function(ctx.realm()),
                ),
                None,
                ctx,
            )
            .then(
                Some(
                    NativeFunction::from_copy_closure_with_captures(
                        // Finally, evaluate the root module.
                        // This returns a `JsPromise` since a module could have
                        // top-level await statements, which defers module execution to the
                        // job queue.
                        |_, _, module, context| Ok(module.evaluate(context).into()),
                        module.clone(),
                    )
                    .to_js_function(ctx.realm()),
                ),
                None,
                ctx,
            );

        // Very important to push forward the job queue after queueing promises.
        ctx.run_jobs();

        // Checking if the final promise didn't return an error.
        match promise_result.state() {
            PromiseState::Pending => {
                return Err(JsRunnerError::Other("module didn't execute!".to_owned()))
            }
            PromiseState::Fulfilled(v) => {
                assert_eq!(v, JsValue::undefined());
            }
            PromiseState::Rejected(err) => {
                return Err(JsError::from_opaque(err).try_native(ctx)?.into())
            }
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
                let mut logs = Logs::new();
                let boa_runner = wrap_result!(BoaRunner::try_new(), logs);
                let config = wrap_result!(
                    simd_json::serde::to_string_pretty(&mapping)
                        .map_err(|e| { std::io::Error::new(std::io::ErrorKind::InvalidData, e) }),
                    logs
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
                    boa_runner
                        .parse_module(&execute_module, "main")
                        .map_err(|e| {
                            logs.error(format!("failed to parse the main module: {:?}", e));
                            e
                        }),
                    logs
                );
                wrap_result!(boa_runner.execute_module(&main_module));
                let ctx = boa_runner.get_ctx();
                let namespace = main_module.namespace(&mut ctx.borrow_mut());
                let result = wrap_result!(
                    namespace.get(js_string!("result"), &mut ctx.borrow_mut()),
                    logs
                );
                let mut result = wrap_result!(
                    result
                        .as_string()
                        .ok_or_else(|| JsNativeError::typ().with_message("Expected string"))
                        .map(|str| str.to_std_string_escaped()),
                    logs
                );
                let mapping = wrap_result!(
                    unsafe { simd_json::serde::from_str::<Mapping>(&mut result) }
                        .map_err(|e| { std::io::Error::new(std::io::ErrorKind::InvalidData, e) }),
                    logs
                );
                (Ok::<Mapping, JsRunnerError>(mapping), logs)
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
        // let ctx = AsyncContext::full(&self.0)
        //     .await
        //     .context("failed to get a context")?;
        // let outputs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        // let outputs_clone = outputs.clone();
        // let print = move |msg: String| {
        //     let mut outs = outputs_clone.lock();
        //     tracing::debug!("script log: {:?}", msg);
        //     outs.push(msg.clone())
        // };
        // let script = utils::wrap_script_if_not_esm(script);
        // let config = simd_json::to_string_pretty(&mapping)?;
        // let mut result = async_with!(ctx => |ctx| {
        //     let raw_ctx = ctx.clone();
        //     let run = || async move {
        //         let global = ctx.globals();
        //         global
        //             .set(
        //                 "print",
        //                 Function::new(ctx.clone(), print)?.with_name("print")?,
        //             )
        //             .context("failed to set print fn")?;
        //         // if user script fn is main(config): config should convert to esm
        //         let user_module = format!("{script};
        //         let config = JSON.parse('{config}');
        //         export let _processed_config = await main(config);
        //         ");
        //         println!("user_module: {:?}", user_module);
        //         Module::declare(ctx.clone(), "user_script", user_module).context("fail to define the user_script module")?;
        //         let promises = Module::evaluate(ctx.clone(), "process_honey", "
        //         import { _processed_config } from \"user_script\";
        //         globalThis.final_result = JSON.stringify(_processed_config);
        //         ").context("fail to eval the process_honey module")?;
        //         promises
        //             .into_future::<()>()
        //             .await
        //             .context("fail to eval the module")?;
        //         let final_result = ctx.globals()
        //             .get::<_, rquickjs::String>("final_result")
        //             .context("fail to get the final result")?
        //             .to_string()
        //             .context("fail to convert the final result to string")?;
        //         Ok::<String, anyhow::Error>(final_result)
        //     };
        //     let res = run().await;
        //     res.map_err(|e| {
        //         // println!("error: {:?}", e);
        //         // check whether the error inside is a QuickJS exception
        //         // TODO: maybe the chains should be Context -> RawException -> Error
        //         for cause in e.chain() {
        //             if let Some(rquickjs::Error::Exception) = cause.downcast_ref::<rquickjs::Error>() {
        //                 let raw_exception = raw_ctx.catch();
        //                 return e.context(format!("QuickJS exception: {:?}", raw_exception))
        //             }
        //         }
        //         e
        //     })
        // }).await?;
        // let buff = unsafe { result.as_bytes_mut() };
        // let mapping = simd_json::from_slice::<Mapping>(buff)
        //     .context("failed to convert the result to mapping")?;
        // let outs = outputs.lock();
        // Ok((mapping, outs.to_vec()))
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
                // assert_eq!(outs, vec!["{\"rules\":[\"111\",\"222\"],\"tun\":{\"enable\":false,\"dns\":{\"enable\":false}},\"proxies\":[\"111\"]}"]);
            });
    }
}
