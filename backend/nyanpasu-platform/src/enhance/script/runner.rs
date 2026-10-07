use async_trait::async_trait;
use nyanpasu_config::runtime::executor::{StepLogEntry, StepLogLevel};
use serde_yaml::Mapping;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use super::{ScriptDirs, js, lua};
use nyanpasu_application::enhance::{ScriptType, ScriptWrapper};

/// Collects the console output of one run. The engine's console callbacks
/// hold clones; the runner drains it into the caller's logs once the run
/// ends, whether the script succeeded or not.
#[derive(Debug, Clone, Default)]
pub struct ConsoleSink(Rc<RefCell<Vec<StepLogEntry>>>);

impl ConsoleSink {
    pub fn push(&self, level: StepLogLevel, message: String) {
        self.0.borrow_mut().push(StepLogEntry::new(level, message));
    }

    pub fn take(&self) -> Vec<StepLogEntry> {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

/// Everything a script logs goes to `logs`, including what it logged before
/// failing.
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

pub struct RunnerManager {
    runners: HashMap<ScriptType, Box<dyn Runner>>,
    script_dirs: ScriptDirs,
}

impl RunnerManager {
    pub fn new(script_dirs: ScriptDirs) -> Self {
        Self {
            runners: HashMap::new(),
            script_dirs,
        }
    }
    // If the script runner is not exist, it should be created.
    pub fn get_or_init_runner(&mut self, script_type: &ScriptType) -> anyhow::Result<&dyn Runner> {
        if !self.runners.contains_key(script_type) {
            let runner = match script_type {
                ScriptType::JavaScript => {
                    Box::new(js::JSRunner::new(&self.script_dirs)?) as Box<dyn Runner>
                }
                ScriptType::Lua => Box::new(lua::LuaRunner) as Box<dyn Runner>,
            };
            self.runners.insert(*script_type, runner);
        }
        Ok(self.runners.get(script_type).unwrap().as_ref())
    }

    #[tracing::instrument(skip(self, script, config, logs), fields(script_kind = ?script.0))]
    pub async fn process_script(
        &mut self,
        script: &ScriptWrapper,
        config: Mapping,
        logs: &mut Vec<StepLogEntry>,
    ) -> anyhow::Result<Mapping> {
        let runner = self.get_or_init_runner(&script.0)?;
        tracing::trace!(script_kind = ?script.0, script_code = script.1.as_str(), "process config with script");
        runner.process_honey(config, script.1.as_str(), logs).await
    }
}
