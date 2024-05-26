use std::{path::Path, sync::Arc};

use crate::utils::dirs;

use super::runner::RunnerManager;
use anyhow::Context;
use parking_lot::Mutex;
use rquickjs::{
    async_with,
    loader::{BuiltinResolver, FileResolver, ScriptLoader},
    AsyncContext, AsyncRuntime, Function, Module,
};
use serde_yaml::Mapping;
use tauri::async_runtime;
use tracing_attributes::instrument;

struct JSRunner(AsyncRuntime);

impl RunnerManager for JSRunner {
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

    async fn process(&self, mapping: Mapping, path: &str) -> Result<Mapping, anyhow::Error> {
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

        let result = async_with!(ctx => |ctx| {
            let global = ctx.globals();
            global
                .set(
                    "print",
                    Function::new(ctx.clone(), print)?.with_name("print")?,
                )
                .context("failed to set print fn")?;
            // get filename
            let filename = Path::new(path)
                .file_name()
                .ok_or(anyhow::anyhow!("failed convert to filename"))?
                .to_string_lossy();
            let config = simd_json::to_string_pretty(&mapping)?;
            let promise = Module::evaluate(
                ctx.clone(),
                "main",
                format!(
                    r#"import main from "{filename}"
            const config = JSON.parse(`{config}`)
            const result = main(config)
            JSON.stringify(result)
            "#
                ),
            )
            .context("failed eval module")?;
            let mut output = promise
                .into_future::<rquickjs::String>()
                .await?
                .to_string()
                .context("failed to convert the result to std string")?;
            let buff = unsafe { output.as_bytes_mut() };
            let mapping = simd_json::from_slice::<Mapping>(buff)
                .context("failed to convert the result to mapping")?;
            // TODO: maybe it can be solved mapping in the future?
            // let mut loader = ModuleLoader::default();
            // let module = loader.load(&ctx, path).context("failed to load script")?;
            // let (module, promise) = module.eval().context("failed to eval module")?;
            // promise.into_future::<()>().await.context("failed to eval module")?;
            // let default_fn = module.get::<&str, Function>("default")?;
            // let args = Args::new(ctx, 1);
            // args.push_arg(mapping);
            Ok::<Mapping, anyhow::Error>(mapping)
        })
        .await?;

        Ok(result)
    }

    async fn process_honey(&self, mapping: Mapping, script: &str) -> Result<Mapping, anyhow::Error> {
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
        let result = async_with!(ctx => |ctx| {
            let module = Module::declare(ctx, "script", r#""#).context("fail to define the module")?;
            Ok::<(), anyhow::Error>(())
        }).await?;
        Ok(mapping)
    }
}
