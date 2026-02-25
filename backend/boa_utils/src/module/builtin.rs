use std::{cell::RefCell, io::Read, rc::Rc};

use anyhow::Context as _;
use boa_engine::{Context, JsNativeError, JsResult, JsString, Module, module::ModuleLoader};
use boa_parser::Source;
use include_compress_bytes::include_bytes_brotli;
use include_url_macro::include_url_bytes_with_brotli;
use phf::phf_map;

pub(crate) const BUILTIN_MODULE_PREFIX: &str = "nyan:";

static BUILTIN_MODULES: phf::Map<&str, &[u8]> = phf_map! {
    // Remote resources
    "es-toolkit" => include_url_bytes_with_brotli!("https://fastly.jsdelivr.net/npm/es-toolkit@1.39.10/+esm"),
    "yaml" => include_url_bytes_with_brotli!("https://fastly.jsdelivr.net/npm/yaml@2.8.1/+esm"),
    "dedent" => include_url_bytes_with_brotli!("https://fastly.jsdelivr.net/npm/dedent@1.7.0/+esm"),
    "js-base64" => include_url_bytes_with_brotli!("https://fastly.jsdelivr.net/npm/js-base64@3.7.8/+esm"),

    // Local utils,
    "utils" => include_bytes_brotli!("./builtin/utils.js"),
};

/// A ModuleLoader load resources from builtin static resources
pub struct BuiltinModuleLoader;

impl ModuleLoader for BuiltinModuleLoader {
    async fn load_imported_module(
        self: Rc<Self>,
        _referrer: boa_engine::module::Referrer,
        specifier: JsString,
        context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let specifier_str = specifier.to_std_string_escaped();
        let result: Result<_, anyhow::Error> = (|| {
            let module_name = specifier_str
                .strip_prefix(BUILTIN_MODULE_PREFIX)
                .context("Not builtin module prefix")?;
            log::trace!("Trying to reading builtin module: {}", module_name);
            let module_data = BUILTIN_MODULES
                .get(module_name)
                .context("Builtin module not found")?;
            let mut data = Vec::with_capacity(1024 * 8);
            {
                let mut reader = brotli::Decompressor::new(&**module_data, 4096);
                let mut buf = [0u8; 1024 * 8];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(read) => data.extend_from_slice(&buf[..read]),
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                            continue;
                        }
                        Err(err) => Err(err).context("failed to decode br stream")?,
                    }
                }
            }
            Ok(data)
        })();

        let data = result.map_err(|err| {
            log::error!("Failed to loading builtin module: {}", specifier_str);
            JsNativeError::typ().with_message(err.to_string())
        })?;

        log::trace!("Finishing loading builtin module: {}", specifier_str);
        let source = Source::from_bytes(&data);
        Module::parse(source, None, &mut context.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use boa_engine::{JsValue, job::SimpleJobExecutor};

    use super::*;

    #[test_log::test]
    fn test_builtin_module_loader() -> JsResult<()> {
        use boa_engine::{builtins::promise::PromiseState, js_string};
        use std::rc::Rc;

        // A simple snippet that imports modules from the web instead of the file system.
        const SRC: &str = r#"
            import { isEqual } from 'nyan:es-toolkit';
            import dedent from 'nyan:dedent';
            import YAML from 'nyan:yaml';
            import { Base64 } from 'nyan:js-base64';

            if (isEqual(1, 2)) {
                throw new Error('Wrong isEqual implementation');
            }

            const data = dedent`
                object:
                    array: ["hello", "world"]
                    key: "value"
            `;

            const object = YAML.parse(data).object;

            let result = [
                Base64.encode(object.array[0]),
                Base64.encode(object.array[1]),
            ]

            export default result;
        "#;

        let queue = Rc::new(SimpleJobExecutor::new());
        let mut context = Context::builder()
            .job_executor(queue)
            // NEW: sets the context module loader to our custom loader
            .module_loader(Rc::new(BuiltinModuleLoader))
            .build()?;

        let module = Module::parse(Source::from_bytes(SRC.as_bytes()), None, &mut context)?;

        // Calling `Module::load_link_evaluate` takes care of having to define promise handlers for
        // `Module::load` and `Module::evaluate`.
        let promise = module.load_link_evaluate(&mut context);

        // Important to call `Context::run_jobs`, or else all the futures and promises won't be
        // pushed forward by the job queue.
        let _ = context.run_jobs();

        match promise.state() {
            // Our job queue guarantees that all promises and futures are finished after returning
            // from `Context::run_jobs`.
            // Some other job queue designs only execute a "microtick" or a single pass through the
            // pending promises and futures. In that case, you can pass this logic as a promise handler
            // for `promise` instead.
            PromiseState::Pending => panic!("module didn't execute!"),
            // All modules after successfully evaluating return `JsValue::undefined()`.
            PromiseState::Fulfilled(v) => {
                assert_eq!(v, JsValue::undefined())
            }
            PromiseState::Rejected(err) => {
                panic!("{:#?}: {}", err.display_obj(false), err.display());
            }
        }

        let default = module
            .namespace(&mut context)
            .get(js_string!("default"), &mut context)?;

        // `default` should contain the result of our calculations.
        let default = default
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("default export was not an object"))?;

        assert_eq!(
            default.get(0, &mut context)?.as_string().ok_or_else(
                || JsNativeError::typ().with_message("array element was not a string")
            )?,
            js_string!("aGVsbG8=")
        );
        assert_eq!(
            default.get(1, &mut context)?.as_string().ok_or_else(
                || JsNativeError::typ().with_message("array element was not a string")
            )?,
            js_string!("d29ybGQ=")
        );

        Ok(())
    }

    #[test_log::test]
    fn test_builtin_utils() -> JsResult<()> {
        use boa_engine::{builtins::promise::PromiseState, js_string};
        use std::rc::Rc;

        // A simple snippet that imports modules from the web instead of the file system.
        const SRC: &str = r#"
            import { yaml } from 'nyan:utils';
            import { Base64 } from 'nyan:js-base64';

            const data = yaml`
                object:
                    array: ["hello", "world"]
                    key: "value"
            `;

            const object = data.object;

            let result = [
                Base64.encode(object.array[0]),
                Base64.encode(object.array[1]),
            ]

            export default result;
        "#;

        let queue = Rc::new(SimpleJobExecutor::new());
        let mut context = Context::builder()
            .job_executor(queue)
            // NEW: sets the context module loader to our custom loader
            .module_loader(Rc::new(BuiltinModuleLoader))
            .build()?;

        let module = Module::parse(Source::from_bytes(SRC.as_bytes()), None, &mut context)?;

        // Calling `Module::load_link_evaluate` takes care of having to define promise handlers for
        // `Module::load` and `Module::evaluate`.
        let promise = module.load_link_evaluate(&mut context);

        // Important to call `Context::run_jobs`, or else all the futures and promises won't be
        // pushed forward by the job queue.
        let _ = context.run_jobs();

        match promise.state() {
            // Our job queue guarantees that all promises and futures are finished after returning
            // from `Context::run_jobs`.
            // Some other job queue designs only execute a "microtick" or a single pass through the
            // pending promises and futures. In that case, you can pass this logic as a promise handler
            // for `promise` instead.
            PromiseState::Pending => panic!("module didn't execute!"),
            // All modules after successfully evaluating return `JsValue::undefined()`.
            PromiseState::Fulfilled(v) => {
                assert_eq!(v, JsValue::undefined())
            }
            PromiseState::Rejected(err) => {
                panic!("{:#?}: {}", err.display_obj(false), err.display());
            }
        }

        let default = module
            .namespace(&mut context)
            .get(js_string!("default"), &mut context)?;

        // `default` should contain the result of our calculations.
        let default = default
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("default export was not an object"))?;

        assert_eq!(
            default.get(0, &mut context)?.as_string().ok_or_else(
                || JsNativeError::typ().with_message("array element was not a string")
            )?,
            js_string!("aGVsbG8=")
        );
        assert_eq!(
            default.get(1, &mut context)?.as_string().ok_or_else(
                || JsNativeError::typ().with_message("array element was not a string")
            )?,
            js_string!("d29ybGQ=")
        );

        Ok(())
    }
}
