//! Local capabilities for real-engine tests; HTTP never leaves this fixture.
use std::{path::Path, rc::Rc, sync::Arc};

use boa_engine::{JsResult, module::SimpleModuleLoader};
use boa_utils::module::{
    builtin::BuiltinModuleLoader,
    combine::CombineModuleLoader,
    http::{CachedItem, HttpModuleError, HttpModuleLoader, HttpModuleSource},
};

use crate::{
    ConsoleSink,
    extensions::{JavaScriptExtensions, LuaExtensions, ScriptExtensionFactory},
};

pub(super) struct TestExtensions;

impl ScriptExtensionFactory for TestExtensions {
    fn javascript(&self, scripts_dir: &Path) -> JsResult<JavaScriptExtensions> {
        let loader = Rc::new(CombineModuleLoader::new(
            SimpleModuleLoader::new(scripts_dir)?,
            HttpModuleLoader::new(Arc::new(FixtureSource)),
            BuiltinModuleLoader,
        ));
        Ok(JavaScriptExtensions::new(
            loader,
            ConsoleSink::register_javascript,
        ))
    }

    fn lua(&self) -> LuaExtensions {
        LuaExtensions::new(mlua::StdLib::ALL_SAFE, ConsoleSink::register_lua)
    }
}

struct FixtureSource;

#[async_trait::async_trait]
impl HttpModuleSource for FixtureSource {
    async fn load(&self, url: &url::Url) -> Result<CachedItem, HttpModuleError> {
        let content = match url.as_str() {
            "https://esm.run/yaml@2.3.4" => "export { default } from 'nyan:yaml';",
            "https://esm.run/js-base64@3.7.6" => "export { Base64 } from 'nyan:js-base64';",
            "https://esm.run/array-from-async@3.0.0" => {
                "export default async function fromAsync(values) { return Promise.all(values); }"
            }
            _ => {
                return Err(HttpModuleError::LoadModule {
                    source: std::io::Error::other(format!("unexpected test URL: {url}")).into(),
                });
            }
        };
        Ok(CachedItem {
            mime: "application/javascript".into(),
            content: content.into(),
        })
    }
}
