use super::runner::{ConsoleSink, Runner};
use anyhow::Context as _;
use async_trait::async_trait;
use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue, Source,
    builtins::promise::PromiseState,
    gc::{Finalize, Trace},
    job::SimpleJobExecutor,
    js_string,
    module::{Module, SimpleModuleLoader},
    object::builtins::JsPromise,
};
use boa_runtime::console::{Console, ConsoleState, Logger};
use boa_utils::module::{combine::CombineModuleLoader, http::HttpModuleLoader};
use nyanpasu_config::runtime::executor::{StepLogEntry, StepLogLevel};
use serde_yaml::Mapping;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};
use utils::wrap_script_if_not_esm;

use std::result::Result as StdResult;

type Result<T, E = JsRunnerError> = StdResult<T, E>;

/// Where the JavaScript runner keeps the modules it runs and the modules it
/// downloads.
#[derive(Debug, Clone)]
pub struct ScriptDirs {
    /// Module root the script's relative imports resolve against. The script
    /// itself runs from memory; nothing is written here.
    pub scripts: PathBuf,
    /// Cache of the modules scripts import over HTTP.
    pub cache: PathBuf,
}

impl ScriptDirs {
    pub fn new(scripts: PathBuf, cache: PathBuf) -> Self {
        Self { scripts, cache }
    }

    pub fn under(root: &Path) -> Self {
        Self {
            scripts: root.join("scripts"),
            cache: root.join("cache"),
        }
    }
}

// define a JsRunnerError due to boa engine error is not Send
#[derive(Debug, thiserror::Error)]
pub enum JsRunnerError {
    #[error("JsError: {0}")]
    JsError(#[from] boa_engine::JsError),
    #[error("JsNativeError: {0}")]
    JsNativeError(#[from] boa_engine::JsNativeError),
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Other: {0}")]
    Other(String),
}

impl Finalize for ConsoleSink {}

// SAFETY: the sink holds no `Gc` pointers.
unsafe impl Trace for ConsoleSink {
    boa_engine::gc::empty_trace!();
}

impl Logger for ConsoleSink {
    fn log(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.push(StepLogLevel::Log, msg);
        Ok(())
    }

    fn info(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.push(StepLogLevel::Info, msg);
        Ok(())
    }

    fn warn(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.push(StepLogLevel::Warn, msg);
        Ok(())
    }

    fn error(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.push(StepLogLevel::Error, msg);
        Ok(())
    }
}

pub struct JSRunner {
    /// Canonical, like the root the module loader resolves imports against.
    scripts_dir: PathBuf,
    cache_dir: PathBuf,
}

impl JSRunner {
    pub fn new(dirs: &ScriptDirs) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&dirs.scripts).with_context(|| {
            format!(
                "failed to create the scripts dir {}",
                dirs.scripts.display()
            )
        })?;
        Ok(Self {
            scripts_dir: dunce::canonicalize(&dirs.scripts)?,
            cache_dir: dirs.cache.clone(),
        })
    }
}

/// Registers the Web APIs scripts may use. Only pure ones: timers, fetch and
/// other time- or IO-bound APIs would make a transform's output depend on
/// more than its input.
fn register_web_apis(context: &mut Context) -> JsResult<()> {
    boa_runtime::base64::register(None, context)?;
    boa_runtime::clone::register(None, context)?;
    boa_runtime::text::register(None, context)?;
    boa_runtime::url::Url::register(None, context)?;
    Ok(())
}

// boa engine is single-thread runner so that we can not define it in runner trait directly
pub struct BoaRunner {
    ctx: Rc<RefCell<Context>>,
    simple_loader: Rc<SimpleModuleLoader>,
    scripts_dir: PathBuf,
}

impl BoaRunner {
    pub fn try_new(scripts_dir: PathBuf, cache_dir: PathBuf) -> Result<Self> {
        let loader = Rc::new(CombineModuleLoader::new(
            SimpleModuleLoader::new(&scripts_dir)?,
            HttpModuleLoader::new(cache_dir, Duration::from_secs(60 * 60 * 24 * 30)),
        ));
        let simple_loader = loader.clone_simple();
        let queue = Rc::new(SimpleJobExecutor::new());
        let mut context = Context::builder()
            .job_executor(queue)
            .module_loader(loader.clone())
            .build()?;
        register_web_apis(&mut context)?;
        Ok(Self {
            ctx: Rc::new(RefCell::new(context)),
            simple_loader,
            scripts_dir,
        })
    }

    pub fn setup_console(&self, logger: impl Logger + 'static) -> Result<()> {
        Console::register_with_logger(logger, &mut self.ctx.borrow_mut())?;
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
            .insert(self.scripts_dir.join(&path_name), module.clone());
        Ok(module)
    }

    pub fn execute_module(&self, module: &Module) -> Result<()> {
        let ctx = &mut self.ctx.borrow_mut();
        let promise = module.load_link_evaluate(ctx);
        settle(&promise, ctx)?;
        Ok(())
    }
}

/// Runs the job queue until `promise` settles and returns its value.
fn settle(promise: &JsPromise, ctx: &mut Context) -> Result<JsValue> {
    // Very important to push forward the job queue after queueing promises.
    let _ = ctx.run_jobs();

    // Checking if the final promise didn't return an error.
    for _ in 0..20 {
        match promise.state() {
            PromiseState::Pending => std::thread::sleep(Duration::from_millis(100)),
            PromiseState::Fulfilled(v) => return Ok(v),
            PromiseState::Rejected(err) => return Err(JsError::from_opaque(err).into()),
        }
    }
    Err(JsRunnerError::Other("the script didn't finish".to_string()))
}

/// Evaluates `script` as a module from memory, then calls its default export
/// with `mapping` and awaits the result. Everything the script logs goes to
/// `console`.
fn run_module(
    scripts_dir: PathBuf,
    cache_dir: PathBuf,
    script: &str,
    mapping: Mapping,
    console: ConsoleSink,
) -> Result<Mapping> {
    let boa_runner = BoaRunner::try_new(scripts_dir, cache_dir)?;
    boa_runner.setup_console(console)?;
    let module = boa_runner.parse_module(script, "main")?;
    boa_runner.execute_module(&module)?;

    let ctx = boa_runner.get_ctx();
    let ctx = &mut ctx.borrow_mut();
    let main = module.namespace(ctx).get(js_string!("default"), ctx)?;
    let main = main
        .as_callable()
        .ok_or_else(|| JsNativeError::typ().with_message("the default export is not a function"))?;
    let config = serde_json::to_value(&mapping)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let config = JsValue::from_json(&config, ctx)?;
    let returned = main.call(&JsValue::undefined(), &[config], ctx)?;
    let result = settle(&JsPromise::resolve(returned, ctx)?, ctx)?;

    // JSON.stringify rather than `JsValue::to_json`: it honours `toJSON` and
    // drops functions and undefined members, which scripts rely on.
    let json = ctx.intrinsics().objects().json();
    let stringify = json.get(js_string!("stringify"), ctx)?;
    let result = stringify
        .as_callable()
        .ok_or_else(|| JsNativeError::typ().with_message("JSON.stringify is not callable"))?
        .call(&json.into(), &[result], ctx)?;
    let result = result
        .as_string()
        .ok_or_else(|| JsNativeError::typ().with_message("Expected string"))
        .map(|str| str.to_std_string_escaped())?;
    let mapping = serde_json::from_str(&result)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(mapping)
}

#[async_trait]
impl Runner for JSRunner {
    async fn process(
        &self,
        mapping: Mapping,
        path: &str,
        logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping> {
        let content = tokio::fs::read_to_string(path)
            .await
            .context("failed to read the script file")?;
        self.process_honey(mapping, &content, logs).await
    }

    async fn process_honey(
        &self,
        mapping: Mapping,
        script: &str,
        logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping> {
        let script = wrap_script_if_not_esm(script)?.into_owned();
        // boa engine is single-thread runner so that we can use it in tokio::task::spawn_blocking
        let (scripts_dir, cache_dir) = (self.scripts_dir.clone(), self.cache_dir.clone());
        let res = tokio::task::spawn_blocking(move || {
            // The sink is not Send, so it lives on this thread with the boa
            // context and only its entries cross back.
            let console = ConsoleSink::default();
            let result = run_module(scripts_dir, cache_dir, &script, mapping, console.clone())
                .map_err(|e| {
                    // Display, not Debug: a JsError's Display carries the
                    // script's stack with file, line and column.
                    tracing::error!("error: {e}");
                    anyhow::anyhow!("{e}")
                });
            (result, console.take())
        })
        .await;
        let (result, run_logs) = res?;
        logs.extend(run_logs);
        result
    }
}

mod utils {
    use oxc_allocator::Allocator;
    use oxc_ast_visit::{
        Visit,
        walk::{walk_function, walk_module_export_name},
    };
    use oxc_parser::Parser;
    use oxc_span::{SourceType, Span};
    use oxc_syntax::scope::ScopeFlags;

    use std::borrow::Cow;

    #[derive(Debug)]
    // TODO: support fn params check and support typescript type erase
    #[allow(dead_code)]
    struct DefaultExport {
        span: Span,
        is_function: bool,
    }

    #[derive(Debug, Default)]
    struct FunctionVisitor<'n> {
        exported_name: Vec<Cow<'n, str>>,
        declared_functions: Vec<(Cow<'n, str>, Cow<'n, Span>)>,
        default_export: Option<DefaultExport>,
    }

    impl<'n> Visit<'n> for FunctionVisitor<'n> {
        // Visit module exported name to confirm whether exists default export
        fn visit_module_export_name(&mut self, it: &oxc_ast::ast::ModuleExportName<'n>) {
            match it {
                oxc_ast::ast::ModuleExportName::IdentifierName(id) => {
                    self.exported_name.push(Cow::Borrowed(id.name.as_str()))
                }
                oxc_ast::ast::ModuleExportName::IdentifierReference(id) => {
                    self.exported_name.push(Cow::Borrowed(id.name.as_str()))
                }
                oxc_ast::ast::ModuleExportName::StringLiteral(s) => {
                    self.exported_name.push(Cow::Borrowed(s.value.as_str()))
                }
            }
            walk_module_export_name(self, it);
        }

        // Visit function declaration to save the function name and span and check whether it is default export
        fn visit_function(&mut self, it: &oxc_ast::ast::Function<'n>, flags: ScopeFlags) {
            // eprintln!("function: {:#?}", it);
            if let Some(id) = it.id.clone() {
                self.declared_functions
                    .push((Cow::Borrowed(id.name.as_str()), Cow::Owned(it.span)));
            }
            walk_function(self, it, flags);
        }

        // Visit export default declaration to save the default export
        fn visit_export_default_declaration(
            &mut self,
            it: &oxc_ast::ast::ExportDefaultDeclaration<'n>,
        ) {
            self.default_export = Some(DefaultExport {
                is_function: matches!(
                    it.declaration,
                    oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(_)
                ),
                span: it.span,
            });
        }
    }

    /// This is a tool function to wrap the script if it is not a ESM script.
    pub fn wrap_script_if_not_esm(script: &str) -> Result<Cow<'_, str>, anyhow::Error> {
        let allocator = Allocator::default();
        let source_type = SourceType::default().with_module(true);
        // Parse the script as given: the spans below are inserted back into it.
        let result = Parser::new(&allocator, script, source_type).parse();

        if !result.diagnostics.is_empty() {
            let mut errors = String::new();
            for error in result.diagnostics {
                errors.push_str(&format!(
                    "{:?}\n",
                    error.with_source_code(script.to_string())
                ));
            }
            return Err(anyhow::anyhow!("parse error: {}", errors));
        }
        #[cfg(test)]
        eprintln!("result: {:#?}", result.program);
        let mut visitor = FunctionVisitor::default();
        visitor.visit_program(&result.program);
        #[cfg(test)]
        eprintln!("visitor: {:#?}", visitor);
        if visitor.default_export.is_some() {
            return Ok(Cow::Borrowed(script));
        }
        // check whether `function main` exists
        match visitor
            .declared_functions
            .iter()
            .find(|(name, _)| name == "main")
        {
            Some((_, span)) => {
                // just insert `export default` before the function
                let mut script = script.to_string();
                script.insert_str(span.start as usize, "export default ");
                Ok(Cow::Owned(script))
            }
            None => Err(anyhow::anyhow!("no default export or main function")),
        }
    }
}

#[cfg(test)]
mod test {
    use nyanpasu_config::runtime::executor::{StepLogEntry, StepLogLevel};

    /// The runner creates its scripts dir where it is told, runs the script
    /// from there and leaves nothing behind.
    #[tokio::test]
    async fn scripts_run_from_the_injected_dir() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let dirs = ScriptDirs::under(dir.path());
        let runner = JSRunner::new(&dirs).unwrap();
        assert!(dirs.scripts.is_dir());

        let result = runner
            .process_honey(
                serde_yaml::from_str("a: 1").unwrap(),
                "export default function main(config) { return config; }",
                &mut Vec::new(),
            )
            .await;
        let expected: serde_yaml::Mapping = serde_yaml::from_str("a: 1").unwrap();
        assert_eq!(result.unwrap(), expected);
        assert_eq!(std::fs::read_dir(&dirs.scripts).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn yaml_template_preserves_nested_config_through_runner() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
        let input = serde_yaml::from_str("existing: true").unwrap();
        let script = r#"
            import { yaml } from 'nyan:utils';
            export default function main(config) {
                config.a = yaml`nested:
  b: 1
  c: 2
`.nested;
                return config;
            }
        "#;
        let result = runner.process_honey(input, script, &mut Vec::new()).await;
        let expected: serde_yaml::Mapping =
            serde_yaml::from_str("existing: true\na:\n  b: 1\n  c: 2\n").unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[tokio::test]
    async fn logs_written_before_a_throw_survive_the_failure() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
        let mut logs = Vec::new();
        let result = runner
            .process_honey(
                serde_yaml::Mapping::new(),
                "export default function main(config) { console.log('before'); throw new Error('boom'); }",
                &mut logs,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(logs, vec![StepLogEntry::new(StepLogLevel::Log, "before")]);
    }

    async fn run_js(script: &str, input: &str) -> serde_yaml::Mapping {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
        let mut logs = Vec::new();
        runner
            .process_honey(serde_yaml::from_str(input).unwrap(), script, &mut logs)
            .await
            .unwrap_or_else(|e| panic!("{e:?}\nlogs: {logs:?}"))
    }

    fn yaml(text: &str) -> serde_yaml::Mapping {
        serde_yaml::from_str(text).unwrap()
    }

    #[tokio::test]
    async fn scripts_can_deep_copy_with_structured_clone() {
        let output = run_js(
            "export default function main(config) { const copy = structuredClone(config); copy.a.b = 2; config.copied = copy.a.b; return config; }",
            "a:\n  b: 1\n",
        )
        .await;
        assert_eq!(output, yaml("a:\n  b: 1\ncopied: 2\n"));
    }

    #[tokio::test]
    async fn scripts_can_use_atob_and_btoa() {
        let output = run_js(
            "export default function main(config) { config.encoded = btoa('hello'); config.decoded = atob('aGVsbG8='); return config; }",
            "{}",
        )
        .await;
        assert_eq!(output, yaml("encoded: aGVsbG8=\ndecoded: hello\n"));
    }

    #[tokio::test]
    async fn scripts_can_use_text_encoder_and_decoder() {
        let output = run_js(
            "export default function main(config) { const bytes = new TextEncoder().encode('你好'); config.length = bytes.length; config.text = new TextDecoder().decode(bytes); return config; }",
            "{}",
        )
        .await;
        assert_eq!(output, yaml("length: 6\ntext: 你好\n"));
    }

    #[tokio::test]
    async fn scripts_can_parse_share_links_with_url() {
        let output = run_js(
            "export default function main(config) { const url = new URL('trojan://secret@example.com:443?sni=a.com#Node%201'); config.user = url.username; config.host = url.hostname; config.port = url.port; config.search = url.search; config.name = decodeURIComponent(url.hash.slice(1)); return config; }",
            "{}",
        )
        .await;
        assert_eq!(
            output,
            yaml(
                "user: secret\nhost: example.com\nport: '443'\nsearch: ?sni=a.com\nname: Node 1\n"
            )
        );
    }

    /// The returned config goes through `JSON.stringify`, so `toJSON` applies
    /// and undefined members are dropped, as when the runner built the result
    /// in JavaScript.
    #[tokio::test]
    async fn returned_config_follows_json_stringify() {
        let output = run_js(
            "export default async function main(config) { config.gone = undefined; config.when = new Date(0); config.link = new URL('https://example.com/a'); return config; }",
            "kept: 1\n",
        )
        .await;
        assert_eq!(
            output,
            yaml("kept: 1\nwhen: 1970-01-01T00:00:00.000Z\nlink: https://example.com/a\n")
        );
    }

    async fn run_js_error(script: &str) -> String {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
        runner
            .process_honey(serde_yaml::Mapping::new(), script, &mut Vec::new())
            .await
            .unwrap_err()
            .to_string()
    }

    #[tokio::test]
    async fn errors_carry_the_js_stack() {
        let error = run_js_error(
            "export default function main(config) {\n  helper();\n}\nfunction helper() {\n  throw new Error('boom');\n}\n",
        )
        .await;
        assert!(error.contains("Error: boom"), "{error}");
        assert!(error.contains("at helper (./main.mjs:5:9)"), "{error}");
        assert!(error.contains("at main (./main.mjs:2:"), "{error}");
    }

    #[tokio::test]
    async fn errors_after_an_await_carry_the_js_stack() {
        let error = run_js_error(
            "export default async function main(config) {\n  await null;\n  config.proxies.length;\n}\n",
        )
        .await;
        assert!(error.contains("TypeError"), "{error}");
        assert!(error.contains("at main (./main.mjs:3:"), "{error}");
    }

    #[tokio::test]
    async fn a_default_export_that_is_not_a_function_fails() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
        let error = runner
            .process_honey(
                serde_yaml::Mapping::new(),
                "export default 42;",
                &mut Vec::new(),
            )
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("the default export is not a function"),
            "{error}"
        );
    }

    /// Each run owns its console sink, so runs executing at the same time
    /// never see each other's logs.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_runs_keep_their_logs_apart() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
        let script = |tag: &str| {
            format!(
                "export default function main(config) {{ for (let i = 0; i < 50; i++) console.log('{tag}'); return config; }}"
            )
        };
        let (script_a, script_b) = (script("a"), script("b"));
        let (mut logs_a, mut logs_b) = (Vec::new(), Vec::new());
        let (a, b) = tokio::join!(
            runner.process_honey(serde_yaml::Mapping::new(), &script_a, &mut logs_a),
            runner.process_honey(serde_yaml::Mapping::new(), &script_b, &mut logs_b),
        );
        for (tag, result, logs) in [("a", a, logs_a), ("b", b, logs_b)] {
            result.unwrap();
            assert_eq!(logs.len(), 50);
            assert!(
                logs.iter().all(|entry| entry.message == tag),
                "{tag}: {logs:?}"
            );
        }
    }

    #[test]
    fn test_wrap_script_if_not_esm() {
        let script = r#"function main(config) {
            return config
        };"#;
        let script = super::utils::wrap_script_if_not_esm(script).unwrap();
        assert_eq!(
            script,
            "export default function main(config) {\n            return config\n        };"
        );
    }

    #[test]
    fn test_wrap_script_if_esm() {
        let script =
            "export default function main(config) {\n            return config\n        };";
        let script = super::utils::wrap_script_if_not_esm(script).unwrap();
        assert_eq!(
            script,
            "export default function main(config) {\n            return config\n        };"
        );
    }

    /// Leading blank lines are part of the script, so the insert offset
    /// must be taken from the script itself, not from a trimmed copy.
    #[test]
    fn wrap_inserts_export_before_main_after_leading_blank_lines() {
        let script =
            "\n\n// helper\nconst answer = 42;\nfunction main(config) {\n  return config;\n}\n";
        let script = super::utils::wrap_script_if_not_esm(script).unwrap();
        assert_eq!(
            script,
            "\n\n// helper\nconst answer = 42;\nexport default function main(config) {\n  return config;\n}\n"
        );
    }

    #[test]
    fn wrap_exports_main_rather_than_a_function_whose_name_contains_main() {
        let script = "function mainHelper(config) {\n  return config;\n}\nfunction main(config) {\n  return mainHelper(config);\n}\n";
        let script = super::utils::wrap_script_if_not_esm(script).unwrap();
        assert_eq!(
            script,
            "function mainHelper(config) {\n  return config;\n}\nexport default function main(config) {\n  return mainHelper(config);\n}\n"
        );
    }

    #[test]
    fn test_wrap_script_if_not_esm_sample_2() {
        let script = r#"// 国内DNS服务器
const domesticNameservers = [
  "https://dns.alidns.com/dns-query", // 阿里云公共DNS
  "https://doh.pub/dns-query", // 腾讯DNSPod
  "https://doh.360.cn/dns-query" // 360安全DNS
];
// 国外DNS服务器
const foreignNameservers = [
  "https://1.1.1.1/dns-query", // Cloudflare(主)
  "https://1.0.0.1/dns-query", // Cloudflare(备)
  "https://208.67.222.222/dns-query", // OpenDNS(主)
  "https://208.67.220.220/dns-query", // OpenDNS(备)
  "https://194.242.2.2/dns-query", // Mullvad(主)
  "https://194.242.2.3/dns-query" // Mullvad(备)
];
        function main(config) {
            // do something
            return config
        };"#;
        let script = super::utils::wrap_script_if_not_esm(script).unwrap();
        assert_eq!(
            script,
            r#"// 国内DNS服务器
const domesticNameservers = [
  "https://dns.alidns.com/dns-query", // 阿里云公共DNS
  "https://doh.pub/dns-query", // 腾讯DNSPod
  "https://doh.360.cn/dns-query" // 360安全DNS
];
// 国外DNS服务器
const foreignNameservers = [
  "https://1.1.1.1/dns-query", // Cloudflare(主)
  "https://1.0.0.1/dns-query", // Cloudflare(备)
  "https://208.67.222.222/dns-query", // OpenDNS(主)
  "https://208.67.220.220/dns-query", // OpenDNS(备)
  "https://194.242.2.2/dns-query", // Mullvad(主)
  "https://194.242.2.3/dns-query" // Mullvad(备)
];
        export default function main(config) {
            // do something
            return config
        };"#
        );
    }

    #[test]
    fn test_process_honey() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};
        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
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
                let mut logs = Vec::new();
                let res = runner.process_honey(mapping, script, &mut logs).await;
                eprintln!("logs: {logs:?}");
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
                assert_eq!(
                    logs,
                    vec![
                        StepLogEntry::new(StepLogLevel::Log, "Test console log"),
                        StepLogEntry::new(StepLogLevel::Warn, "Test console log"),
                        StepLogEntry::new(StepLogLevel::Error, "Test console log"),
                    ]
                );
            });
    }

    #[test_log::test]
    fn test_process_honey_with_fetch() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};
        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
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
                let mut logs = Vec::new();
                let res = runner.process_honey(mapping, script, &mut logs).await;
                eprintln!("logs: {logs:?}");
                let mapping = res.unwrap();
                assert_eq!(
                    mapping["rules"],
                    serde_yaml::Value::Sequence(vec![
                        serde_yaml::Value::String("RULE-SET,custom-reject,REJECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-direct,DIRECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-proxy,🚀".to_string()),
                        serde_yaml::Value::String("aGVsbG8=".to_string()),
                        serde_yaml::Value::String("d29ybGQ=".to_string()),
                    ])
                );
                assert_eq!(logs, vec![]);
            });
    }

    #[test_log::test]
    fn test_process_honey_with_builtin_modules() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};
        let dir = tempfile::tempdir().unwrap();
        let runner = JSRunner::new(&ScriptDirs::under(dir.path())).unwrap();
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
        import { yaml } from "nyan:utils";
        import { Base64 } from "nyan:js-base64";


        export default async function main(config) {
            const data = yaml`
            object:
                array: ["hello", "world"]
                key: "value"
            `;

            const object = data.object;

            let result = [
                Base64.encode(object.array[0]),
                Base64.encode(object.array[1]),
            ];
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
                let mut logs = Vec::new();
                let res = runner.process_honey(mapping, script, &mut logs).await;
                eprintln!("logs: {logs:?}");
                let mapping = res.unwrap();
                assert_eq!(
                    mapping["rules"],
                    serde_yaml::Value::Sequence(vec![
                        serde_yaml::Value::String("RULE-SET,custom-reject,REJECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-direct,DIRECT".to_string()),
                        serde_yaml::Value::String("RULE-SET,custom-proxy,🚀".to_string()),
                        serde_yaml::Value::String("aGVsbG8=".to_string()),
                        serde_yaml::Value::String("d29ybGQ=".to_string()),
                    ])
                );
                assert_eq!(logs, vec![]);
            });
    }
}
