use nyanpasu_config::profile::ScriptRuntime;

pub mod adapter;
pub use adapter::EnhanceScriptRunner;
mod js;
mod lua;
pub use js::ScriptDirs;
pub use lua::{create_lua_context, ordered_map};
pub mod runner;
pub use runner::RunnerManager;

#[derive(Debug, Clone)]
struct ScriptWrapper(ScriptRuntime, String);
