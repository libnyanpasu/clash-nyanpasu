//! Desktop composition of capabilities available to profile scripts.
#[cfg(feature = "script-javascript")]
use std::{path::Path, rc::Rc, sync::Arc};

#[cfg(feature = "script-javascript")]
use boa_engine::{JsResult, module::SimpleModuleLoader};
#[cfg(feature = "script-javascript")]
use boa_utils::module::{
    builtin::BuiltinModuleLoader,
    combine::CombineModuleLoader,
    http::{HttpModuleLoader, HttpModuleSource},
};
#[cfg(any(feature = "script-javascript", feature = "script-lua"))]
use nyanpasu_scripts::ConsoleSink;
#[cfg(feature = "script-javascript")]
use nyanpasu_scripts::extensions::JavaScriptExtensions;
#[cfg(feature = "script-lua")]
use nyanpasu_scripts::extensions::LuaExtensions;
use nyanpasu_scripts::extensions::ScriptExtensionFactory;

#[cfg(feature = "script-javascript")]
pub(crate) mod http;

pub(crate) struct DesktopScriptExtensions {
    #[cfg(feature = "script-javascript")]
    source: Arc<dyn HttpModuleSource>,
}

#[cfg(feature = "script-javascript")]
impl DesktopScriptExtensions {
    pub(crate) fn new(source: Arc<dyn HttpModuleSource>) -> Self {
        Self { source }
    }
}

impl ScriptExtensionFactory for DesktopScriptExtensions {
    #[cfg(feature = "script-javascript")]
    fn javascript(&self, scripts_dir: &Path) -> JsResult<Option<JavaScriptExtensions>> {
        let loader = Rc::new(CombineModuleLoader::new(
            SimpleModuleLoader::new(scripts_dir)?,
            HttpModuleLoader::new(self.source.clone()),
            BuiltinModuleLoader,
        ));
        Ok(Some(JavaScriptExtensions::new(
            loader,
            ConsoleSink::register_javascript,
        )))
    }

    #[cfg(feature = "script-lua")]
    fn lua(&self) -> Option<LuaExtensions> {
        Some(LuaExtensions::new(
            mlua::StdLib::ALL_SAFE,
            ConsoleSink::register_lua,
        ))
    }
}
