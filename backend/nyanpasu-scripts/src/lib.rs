//! JavaScript and Lua adapters for config's synchronous script execution port.

#[cfg(any(feature = "javascript", feature = "lua"))]
use nyanpasu_config::profile::ScriptRuntime;

mod adapter;
pub use adapter::EnhanceScriptRunner;
#[cfg(feature = "javascript")]
mod js;
#[cfg(feature = "lua")]
mod lua;
pub use runner::{ConsoleSink, ScriptDirs};
pub mod extensions;
mod runner;
#[cfg(test)]
mod test_extensions;
#[cfg(any(feature = "javascript", feature = "lua"))]
use runner::RunnerManager;

#[cfg(any(feature = "javascript", feature = "lua"))]
#[derive(Debug, Clone)]
struct ScriptWrapper(ScriptRuntime, String);
