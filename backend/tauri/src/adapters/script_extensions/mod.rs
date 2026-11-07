//! Desktop composition of capabilities available to profile scripts.
use std::{path::Path, rc::Rc, sync::Arc};

use boa_engine::{JsResult, module::SimpleModuleLoader};
use boa_utils::module::{
    builtin::BuiltinModuleLoader,
    combine::CombineModuleLoader,
    http::{HttpModuleLoader, HttpModuleSource},
};
use nyanpasu_scripts::{
    ConsoleSink,
    extensions::{JavaScriptExtensions, LuaExtensions, ScriptExtensionFactory},
};

pub(crate) mod http;

pub(crate) struct DesktopScriptExtensions {
    source: Arc<dyn HttpModuleSource>,
}

impl DesktopScriptExtensions {
    pub(crate) fn new(source: Arc<dyn HttpModuleSource>) -> Self {
        Self { source }
    }
}

impl ScriptExtensionFactory for DesktopScriptExtensions {
    fn javascript(&self, scripts_dir: &Path) -> JsResult<JavaScriptExtensions> {
        let loader = Rc::new(CombineModuleLoader::new(
            SimpleModuleLoader::new(scripts_dir)?,
            HttpModuleLoader::new(self.source.clone()),
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
