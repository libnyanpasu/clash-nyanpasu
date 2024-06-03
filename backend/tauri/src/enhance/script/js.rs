use std::sync::Arc;

use crate::utils::dirs;

use super::runner::{ProcessOutput, Runner};
use anyhow::Context;
use async_trait::async_trait;
use parking_lot::Mutex;
use rquickjs::{
    async_with,
    loader::{BuiltinResolver, FileResolver, ScriptLoader},
    AsyncContext, AsyncRuntime, Function, Module,
};
use serde_yaml::Mapping;
use tauri::async_runtime;
use tracing_attributes::instrument;
pub struct JSRunner(AsyncRuntime);

#[async_trait]
impl Runner for JSRunner {
    #[instrument]
    fn try_new() -> Result<JSRunner, anyhow::Error> {
        let js_runtime = AsyncRuntime::new().context("failed to create rquickjs runtime")?;
        // let ctx = AsyncContext::full(&js_runtime);
        let app_path = dirs::app_profiles_dir()?;
        let app_path = relative_path::RelativePathBuf::from_path(app_path)?;
        let resolver = (
            BuiltinResolver::default(), // .with_module(path)
            FileResolver::default().with_path(app_path),
        );
        let loader = ScriptLoader::default();
        let runtime: AsyncRuntime = async_runtime::block_on(async move {
            let runtime = js_runtime;
            runtime.set_loader(resolver, loader).await;
            runtime
        });
        Ok(JSRunner(runtime))
    }

    async fn process(&self, mapping: Mapping, path: &str) -> Result<ProcessOutput, anyhow::Error> {
        let content = tokio::fs::read_to_string(path)
            .await
            .context("failed to read the script file")?;
        self.process_honey(mapping, &content).await
    }

    async fn process_honey(
        &self,
        mapping: Mapping,
        script: &str,
    ) -> Result<ProcessOutput, anyhow::Error> {
        let ctx = AsyncContext::full(&self.0)
            .await
            .context("failed to get a context")?;
        let outputs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        let outputs_clone = outputs.clone();
        let print = move |msg: String| {
            let mut outs = outputs_clone.lock();
            tracing::debug!("script log: {:?}", msg);
            outs.push(msg.clone())
        };
        let script = utils::wrap_script_if_not_esm(script);
        let config = simd_json::to_string_pretty(&mapping)?;
        let mut result = async_with!(ctx => |ctx| {
            let global = ctx.globals();
            global
                .set(
                    "print",
                    Function::new(ctx.clone(), print)?.with_name("print")?,
                )
                .context("failed to set print fn")?;
            // if user script fn is main(config): config should convert to esm
            Module::declare(ctx.clone(), "user_script", script).context("fail to define the user_script module")?;
            let module = Module::declare(ctx, "process_honey", format!(r#"
            import user_script from "user_script"
            const config = JSON.parse(`{config}`)
            export const final_result = JSON.stringify(await user_script(config))
            "#)).context("fail to define the process_honey module")?;
            let (decl, promises) = module.eval().context("fail to eval the process_honey module")?;
            promises
                .into_future::<()>()
                .await
                .context("fail to eval the module")?;
            let ns = decl.namespace().context("fail to get the process_honey module namespace")?;
            let final_result = ns.get::<&str, String>("final_result").context("fail to get the final_result")?;
            Ok::<String, anyhow::Error>(final_result)
        }).await?;
        let buff = unsafe { result.as_bytes_mut() };
        let mapping = simd_json::from_slice::<Mapping>(buff)
            .context("failed to convert the result to mapping")?;
        let outs = outputs.lock();
        Ok((mapping, outs.to_vec()))
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
