//! Local capabilities for real-engine tests; HTTP never leaves this fixture.
use std::{path::Path, rc::Rc, sync::Arc};

use boa_engine::{JsResult, module::SimpleModuleLoader};
use boa_utils::module::{
    builtin::BuiltinModuleLoader,
    combine::CombineModuleLoader,
    http::{CachedItem, HttpModuleError, HttpModuleLoader, HttpModuleSource},
};
use nyanpasu_scripts::{
    ConsoleSink,
    extensions::{JavaScriptExtensions, LuaExtensions, ScriptExtensionFactory},
};

pub(super) struct TestExtensions;

impl ScriptExtensionFactory for TestExtensions {
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

    fn lua(&self) -> Option<LuaExtensions> {
        Some(LuaExtensions::new(
            mlua::StdLib::ALL_SAFE,
            ConsoleSink::register_lua,
        ))
    }
}

struct FixtureSource;

#[async_trait::async_trait]
impl HttpModuleSource for FixtureSource {
    async fn load(&self, url: &url::Url) -> Result<CachedItem, HttpModuleError> {
        Err(HttpModuleError::LoadModule {
            source: std::io::Error::other(format!(
                "unexpected HTTP import in transform fixture: {url}"
            ))
            .into(),
        })
    }
}
