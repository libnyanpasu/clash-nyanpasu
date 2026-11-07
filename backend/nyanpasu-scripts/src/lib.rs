//! JavaScript and Lua adapters for config's synchronous script execution port.

use nyanpasu_config::profile::ScriptRuntime;

mod adapter;
pub use adapter::EnhanceScriptRunner;
mod js;
mod lua;
pub use js::ScriptDirs;
use lua::{create_lua_context, ordered_map};
mod runner;
use runner::RunnerManager;

#[derive(Debug, Clone)]
struct ScriptWrapper(ScriptRuntime, String);
