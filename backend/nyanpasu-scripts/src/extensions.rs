//! Host construction inputs for script capabilities, without engine-owned infrastructure.
#[cfg(feature = "javascript")]
use std::{path::Path, rc::Rc};

#[cfg(feature = "javascript")]
use boa_engine::{Context, JsResult};
#[cfg(feature = "javascript")]
use boa_utils::module::combine::CombineModuleLoader;
#[cfg(feature = "lua")]
use mlua::{Lua, StdLib};

#[cfg(any(feature = "javascript", feature = "lua"))]
use crate::runner::ConsoleSink;

/// A host factory may cross threads; returned engine objects stay on the calling thread.
///
/// An engine is available only when its scripts feature is enabled and the host supplies
/// extensions. The default methods decline; `None` rejects the operation with
/// `ScriptExecutionError::StartEngine` before executing a script, without a fallback.
pub trait ScriptExtensionFactory: Send + Sync {
    /// Constructs loaders and console registration for one JavaScript execution.
    ///
    /// # Errors
    /// Returns the host loader construction error without leaving its thread.
    #[cfg(feature = "javascript")]
    fn javascript(&self, _scripts_dir: &Path) -> JsResult<Option<JavaScriptExtensions>> {
        Ok(None)
    }
    /// Supplies the Lua library set and per-run console registration.
    #[cfg(feature = "lua")]
    fn lua(&self) -> Option<LuaExtensions> {
        None
    }
}

/// JavaScript capabilities created on the same thread as the Boa context.
#[cfg(feature = "javascript")]
pub struct JavaScriptExtensions {
    pub(crate) loader: Rc<CombineModuleLoader>,
    pub(crate) console: fn(ConsoleSink, &mut Context) -> JsResult<()>,
}

#[cfg(feature = "javascript")]
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
#[cfg(feature = "lua")]
#[derive(Clone, Copy)]
pub struct LuaExtensions {
    pub(crate) standard_libraries: StdLib,
    pub(crate) console: fn(&Lua, &ConsoleSink) -> mlua::Result<()>,
}

#[cfg(feature = "lua")]
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
