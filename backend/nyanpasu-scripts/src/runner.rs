use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};
#[cfg(any(feature = "javascript", feature = "lua"))]
use std::{collections::HashMap, sync::Arc};

#[cfg(any(feature = "javascript", feature = "lua"))]
use async_trait::async_trait;
#[cfg(any(feature = "javascript", feature = "lua"))]
use nyanpasu_config::profile::ScriptRuntime;
#[cfg(any(feature = "javascript", feature = "lua"))]
use nyanpasu_config::runtime::executor::{ScriptExecutionError, ScriptOperation};
use nyanpasu_config::runtime::executor::{StepLogEntry, StepLogLevel};
#[cfg(any(feature = "javascript", feature = "lua"))]
use serde_yaml::Mapping;

#[cfg(any(feature = "javascript", feature = "lua"))]
use super::ScriptWrapper;
#[cfg(feature = "javascript")]
use super::js;
#[cfg(feature = "lua")]
use super::lua;
#[cfg(any(feature = "javascript", feature = "lua"))]
use crate::extensions::ScriptExtensionFactory;

/// Where the JavaScript runner keeps the modules it runs and the modules it
/// downloads.
#[derive(Debug, Clone)]
pub struct ScriptDirs {
    /// Module root the script's relative imports resolve against. The script
    /// itself runs from memory; nothing is written here.
    pub scripts: PathBuf,
    /// Cache of the modules scripts import over HTTP.
    pub cache: PathBuf,
}

impl ScriptDirs {
    /// Uses the supplied module root and HTTP module cache paths.
    pub fn new(scripts: PathBuf, cache: PathBuf) -> Self {
        Self { scripts, cache }
    }

    /// Places the module root and cache in separate directories under `root`.
    pub fn under(root: &Path) -> Self {
        Self {
            scripts: root.join("scripts"),
            cache: root.join("cache"),
        }
    }
}

/// Collects the console output of one run. The engine's console callbacks
/// hold clones; the runner drains it into the caller's logs once the run
/// ends, whether the script succeeded or not.
#[derive(Debug, Clone, Default)]
pub struct ConsoleSink(Rc<RefCell<Vec<StepLogEntry>>>);

impl ConsoleSink {
    /// Appends an entry to this execution's console output.
    pub fn push(&self, level: StepLogLevel, message: String) {
        self.0.borrow_mut().push(StepLogEntry::new(level, message));
    }

    /// Drains the collected entries in their original order.
    pub fn take(&self) -> Vec<StepLogEntry> {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

/// Everything a script logs goes to `logs`, including what it logged before
/// failing.
#[cfg(any(feature = "javascript", feature = "lua"))]
#[async_trait]
pub trait Runner: Send + Sync {
    #[allow(dead_code)]
    /// Process profiles by script file path
    async fn process(
        &self,
        mapping: Mapping,
        path: &str,
        logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping>;

    /// Honey replacement - use in memory code str to load module and exec it!
    /// It might not be implemented - due to some embeded engine is not support.
    async fn process_honey(
        &self,
        mapping: Mapping,
        script: &str,
        _logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping> {
        tracing::debug!("mapping: {:?}\nscript:{}", mapping, script);
        unimplemented!()
    }
}

#[cfg(any(feature = "javascript", feature = "lua"))]
pub struct RunnerManager {
    runners: HashMap<ScriptRuntime, Box<dyn Runner>>,
    #[cfg(feature = "javascript")]
    script_dirs: ScriptDirs,
    extensions: Arc<dyn ScriptExtensionFactory>,
}

#[cfg(any(feature = "javascript", feature = "lua"))]
impl RunnerManager {
    pub fn new(script_dirs: ScriptDirs, extensions: Arc<dyn ScriptExtensionFactory>) -> Self {
        #[cfg(not(feature = "javascript"))]
        let _ = &script_dirs;
        Self {
            runners: HashMap::new(),
            #[cfg(feature = "javascript")]
            script_dirs,
            extensions,
        }
    }
    // If the script runner is not exist, it should be created.
    pub fn get_or_init_runner(
        &mut self,
        script_type: &ScriptRuntime,
    ) -> anyhow::Result<&dyn Runner> {
        if !self.runners.contains_key(script_type) {
            let runner = match script_type {
                #[cfg(feature = "javascript")]
                ScriptRuntime::JavaScript => Box::new(js::JSRunner::new(
                    &self.script_dirs,
                    self.extensions.clone(),
                )?) as Box<dyn Runner>,
                #[cfg(feature = "lua")]
                ScriptRuntime::Lua => {
                    let extensions =
                        self.extensions
                            .lua()
                            .ok_or(ScriptExecutionError::StartEngine {
                                runtime: *script_type,
                                operation: ScriptOperation::Transform,
                            })?;
                    Box::new(lua::LuaRunner::new(extensions)) as Box<dyn Runner>
                }
                #[cfg(not(feature = "javascript"))]
                ScriptRuntime::JavaScript => {
                    return Err(ScriptExecutionError::StartEngine {
                        runtime: *script_type,
                        operation: ScriptOperation::Transform,
                    }
                    .into());
                }
                #[cfg(not(feature = "lua"))]
                ScriptRuntime::Lua => {
                    return Err(ScriptExecutionError::StartEngine {
                        runtime: *script_type,
                        operation: ScriptOperation::Transform,
                    }
                    .into());
                }
            };
            self.runners.insert(*script_type, runner);
        }
        Ok(self.runners.get(script_type).unwrap().as_ref())
    }

    #[tracing::instrument(skip(self, script, config, logs), fields(script_kind = ?script.0))]
    pub(super) async fn process_script(
        &mut self,
        script: &ScriptWrapper,
        config: Mapping,
        logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping> {
        let runner = self.get_or_init_runner(&script.0)?;
        tracing::trace!(script_kind = ?script.0, script_code = %nyanpasu_log_preview::text(&script.1), "process config with script");
        runner.process_honey(config, script.1.as_str(), logs).await
    }
}
