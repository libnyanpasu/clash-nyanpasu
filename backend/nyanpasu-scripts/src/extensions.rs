//! Host construction inputs for script capabilities, without engine-owned infrastructure.
use std::{path::Path, rc::Rc};

use boa_engine::{Context, JsResult};
use boa_utils::module::combine::CombineModuleLoader;
use mlua::{Lua, StdLib};

use crate::runner::ConsoleSink;

/// A host factory may cross threads; returned engine objects stay on the calling thread.
pub trait ScriptExtensionFactory: Send + Sync {
    /// Constructs loaders and console registration for one JavaScript execution.
    ///
    /// # Errors
    /// Returns the host loader construction error without leaving its thread.
    fn javascript(&self, scripts_dir: &Path) -> JsResult<JavaScriptExtensions>;
    /// Supplies the Lua library set and per-run console registration.
    fn lua(&self) -> LuaExtensions;
}

/// JavaScript capabilities created on the same thread as the Boa context.
pub struct JavaScriptExtensions {
    pub(crate) loader: Rc<CombineModuleLoader>,
    pub(crate) console: fn(ConsoleSink, &mut Context) -> JsResult<()>,
}

impl JavaScriptExtensions {
    /// Uses the injected local/HTTP/builtin loader and the per-run console installer.
    pub fn new(
        loader: Rc<CombineModuleLoader>,
        console: fn(ConsoleSink, &mut Context) -> JsResult<()>,
    ) -> Self {
        Self { loader, console }
    }
}

/// Lua capabilities, applied to a fresh state for each execution.
#[derive(Clone, Copy)]
pub struct LuaExtensions {
    pub(crate) standard_libraries: StdLib,
    pub(crate) console: fn(&Lua, &ConsoleSink) -> mlua::Result<()>,
}

impl LuaExtensions {
    /// Chooses standard libraries and the console installer explicitly.
    pub fn new(
        standard_libraries: StdLib,
        console: fn(&Lua, &ConsoleSink) -> mlua::Result<()>,
    ) -> Self {
        Self {
            standard_libraries,
            console,
        }
    }
}
