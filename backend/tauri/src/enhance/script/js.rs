use super::runner::{ProcessOutput, Runner, wrap_result};
use crate::enhance::utils::{Logs, LogsExt};
use anyhow::Context as _;
use async_trait::async_trait;
use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue, Source,
    builtins::promise::PromiseState,
    gc::{Finalize, Trace},
    job::SimpleJobExecutor,
    js_string,
    module::{Module, SimpleModuleLoader},
};
use boa_utils::module::{combine::CombineModuleLoader, http::HttpModuleLoader};
use boa_wintertc::console::{Console, ConsoleState, Logger};
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
    /// Each script is written here as a module file while it runs.
    pub scripts: PathBuf,
    /// Cache of the modules scripts import over HTTP.
    pub cache: PathBuf,
}

impl ScriptDirs {
    pub fn from_resolver(paths: &crate::utils::path::PathResolver) -> Self {
        Self {
            scripts: paths.scripts_dir(),
            cache: paths.cache_dir(),
        }
    }

    #[cfg(test)]
    pub(crate) fn under(root: &Path) -> Self {
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
    #[error("TryNativeError: {0}")]
    TryNativeError(#[from] boa_engine::error::TryNativeError),
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Other: {0}")]
    Other(String),
}

/// Collects the console output of one run. Each run owns its sink, so
/// concurrent runs never see each other's logs.
#[derive(Debug, Clone, Default)]
pub struct ConsoleSink(Rc<RefCell<Logs>>);

impl ConsoleSink {
    pub fn take(&self) -> Logs {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

impl Finalize for ConsoleSink {}

// SAFETY: the sink holds no `Gc` pointers.
unsafe impl Trace for ConsoleSink {
    boa_engine::gc::empty_trace!();
}

impl Logger for ConsoleSink {
    fn log(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.0.borrow_mut().log(msg);
        Ok(())
    }

    fn info(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.0.borrow_mut().info(msg);
        Ok(())
    }

    fn warn(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.0.borrow_mut().warn(msg);
        Ok(())
    }

    fn error(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.0.borrow_mut().error(msg);
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
        let context = Context::builder()
            .job_executor(queue)
            .module_loader(loader.clone())
            .build()?;
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
        let promise_result = module.load_link_evaluate(ctx);

        // Very important to push forward the job queue after queueing promises.
        let _ = ctx.run_jobs();

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
                    return Err(JsError::from_opaque(err).try_native(ctx)?.into());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
}

#[async_trait]
impl Runner for JSRunner {
    async fn process(&self, mapping: Mapping, path: &str) -> ProcessOutput {
        let content = wrap_result!(
            tokio::fs::read_to_string(path)
                .await
                .context("failed to read the script file")
        );
        self.process_honey(mapping, &content).await
    }

    async fn process_honey(&self, mapping: Mapping, script: &str) -> ProcessOutput {
        let script = wrap_result!(wrap_script_if_not_esm(script));
        let hash = crate::utils::help::get_uid("script");
        let path = self.scripts_dir.join(format!("{hash}.mjs"));
        wrap_result!(
            tokio::fs::write(&path, script.as_bytes())
                .await
                .context("failed to write the script file")
        );
        // boa engine is single-thread runner so that we can use it in tokio::task::spawn_blocking
        let (scripts_dir, cache_dir) = (self.scripts_dir.clone(), self.cache_dir.clone());
        let res = tokio::task::spawn_blocking(move || {
            let wrapped_fn = move || {
                let console = ConsoleSink::default();
                let boa_runner = wrap_result!(BoaRunner::try_new(scripts_dir, cache_dir));
                wrap_result!(boa_runner.setup_console(console.clone()), console.take());
                let config = wrap_result!(
                    serde_json::to_string(&mapping)
                        .map_err(|e| { std::io::Error::new(std::io::ErrorKind::InvalidData, e) }),
                    console.take()
                );
                let config = serde_json::to_string(&config).unwrap(); // escape the string
                let execute_module = format!(
                    r#"import process from "./{hash}.mjs";
        let config = JSON.parse({config});
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
                    console.take()
                );
                wrap_result!(boa_runner.execute_module(&main_module));
                let ctx = boa_runner.get_ctx();
                let namespace = main_module.namespace(&mut ctx.borrow_mut());
                let result = wrap_result!(
                    namespace.get(js_string!("result"), &mut ctx.borrow_mut()),
                    console.take()
                );
                let result = wrap_result!(
                    result
                        .as_string()
                        .ok_or_else(|| JsNativeError::typ().with_message("Expected string"))
                        .map(|str| str.to_std_string_escaped()),
                    console.take()
                );
                let mapping = wrap_result!(
                    serde_json::from_str(&result)
                        .map_err(|e| { std::io::Error::new(std::io::ErrorKind::InvalidData, e) }),
                    console.take()
                );
                (Ok::<Mapping, JsRunnerError>(mapping), console.take())
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
        let source_text = script.trim_matches(['\t', '\n', '\r', ' ']);
        let result = Parser::new(&allocator, source_text, source_type).parse();

        if !result.diagnostics.is_empty() {
            let mut errors = String::new();
            for error in result.diagnostics {
                errors.push_str(&format!(
                    "{:?}\n",
                    error.with_source_code(source_text.to_string())
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
            .find(|(name, _)| name.contains("main"))
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
    /// The runner creates its scripts dir where it is told, runs the script
    /// from there and leaves nothing behind.
    #[tokio::test]
    async fn scripts_run_from_the_injected_dir() {
        use super::{super::runner::Runner, JSRunner, ScriptDirs};

        let dir = tempfile::tempdir().unwrap();
        let dirs = ScriptDirs::under(dir.path());
        let runner = JSRunner::new(&dirs).unwrap();
        assert!(dirs.scripts.is_dir());

        let (result, _) = runner
            .process_honey(
                serde_yaml::from_str("a: 1").unwrap(),
                "export default function main(config) { return config; }",
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
        let (result, _) = runner.process_honey(input, script).await;
        let expected: serde_yaml::Mapping =
            serde_yaml::from_str("existing: true\na:\n  b: 1\n  c: 2\n").unwrap();
        assert_eq!(result.unwrap(), expected);
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
        let (a, b) = tokio::join!(
            runner.process_honey(serde_yaml::Mapping::new(), &script_a),
            runner.process_honey(serde_yaml::Mapping::new(), &script_b),
        );
        for (tag, (result, logs)) in [("a", a), ("b", b)] {
            result.unwrap();
            assert_eq!(logs.len(), 50);
            assert!(logs.iter().all(|(_, msg)| msg == tag), "{tag}: {logs:?}");
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
                let (res, logs) = runner.process_honey(mapping, script).await;
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
                let outs = serde_json::to_string(&logs).unwrap();
                assert_eq!(
                    outs,
                    r#"[["log","Test console log"],["warn","Test console log"],["error","Test console log"]]"#
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
                let (res, logs) = runner.process_honey(mapping, script).await;
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
                let outs = serde_json::to_string(&logs).unwrap();
                assert_eq!(outs, r#"[]"#);
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
                let (res, logs) = runner.process_honey(mapping, script).await;
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
                let outs = serde_json::to_string(&logs).unwrap();
                assert_eq!(outs, r#"[]"#);
            });
    }
}
