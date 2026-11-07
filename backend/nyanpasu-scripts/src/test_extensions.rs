//! Local capabilities for real-engine tests; HTTP never leaves this fixture.
#[cfg(feature = "javascript")]
use std::{path::Path, rc::Rc, sync::Arc};

#[cfg(feature = "javascript")]
use boa_engine::{JsResult, module::SimpleModuleLoader};
#[cfg(feature = "javascript")]
use boa_utils::module::{
    builtin::BuiltinModuleLoader,
    combine::CombineModuleLoader,
    http::{CachedItem, HttpModuleError, HttpModuleLoader, HttpModuleSource},
};

#[cfg(any(feature = "javascript", feature = "lua"))]
use crate::ConsoleSink;
#[cfg(feature = "javascript")]
use crate::extensions::JavaScriptExtensions;
#[cfg(feature = "lua")]
use crate::extensions::LuaExtensions;
use crate::extensions::ScriptExtensionFactory;

pub(super) struct TestExtensions;

impl ScriptExtensionFactory for TestExtensions {
    #[cfg(feature = "javascript")]
    fn javascript(&self, scripts_dir: &Path) -> JsResult<Option<JavaScriptExtensions>> {
        let loader = Rc::new(CombineModuleLoader::new(
            SimpleModuleLoader::new(scripts_dir)?,
            HttpModuleLoader::new(Arc::new(FixtureSource)),
            BuiltinModuleLoader,
        ));
        Ok(Some(JavaScriptExtensions::new(
            loader,
            ConsoleSink::register_javascript,
        )))
    }

    #[cfg(feature = "lua")]
    fn lua(&self) -> Option<LuaExtensions> {
        Some(LuaExtensions::new(
            mlua::StdLib::ALL_SAFE,
            ConsoleSink::register_lua,
        ))
    }
}

#[cfg(feature = "javascript")]
struct FixtureSource;

#[cfg(feature = "javascript")]
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
