use anyhow::Error;
use async_trait::async_trait;
use serde_yaml::Mapping;
use std::collections::HashMap;

use super::{js, lua};
use crate::enhance::{Logs, ScriptType};

/// The output of the process function is a tuple of the mapping and the logs.
/// Although the process fails, the logs should be returned.
pub type ProcessOutput = (Result<Mapping, anyhow::Error>, Logs);

/// warp a result and return the ProcessOutput
macro_rules! wrap_result {
    ($result:expr) => {
        match $result {
            Ok(inner) => inner,
            Err(e) => return (Err(e.into()), Vec::new()),
        }
    };

    ($result:expr, $logs:expr) => {
        match $result {
            Ok(inner) => inner,
            Err(e) => return (Err(e.into()), $logs),
        }
    };
}

pub(super) use wrap_result;

#[async_trait]
pub trait Runner: Send + Sync {
    fn try_new() -> Result<Self, Error>
    where
        Self: std::marker::Sized;
    #[allow(dead_code)]
    /// Process profiles by script file path
    async fn process(&self, mapping: Mapping, path: &str) -> ProcessOutput;

    /// Honey replacement - use in memory code str to load module and exec it!
    /// It might not be implemented - due to some embeded engine is not support.
    async fn process_honey(&self, mapping: Mapping, script: &str) -> ProcessOutput {
        tracing::debug!("mapping: {:?}\nscript:{}", mapping, script);
        unimplemented!()
    }
}

pub struct RunnerManager {
    runners: HashMap<ScriptType, Box<dyn Runner>>,
}

impl RunnerManager {
    pub fn new() -> Self {
        Self {
            runners: HashMap::new(),
        }
    }
    // If the script runner is not exist, it should be created.
    pub fn get_or_init_runner(&mut self, script_type: &ScriptType) -> anyhow::Result<&dyn Runner> {
        if !self.runners.contains_key(script_type) {
            let runner = match script_type {
                ScriptType::JavaScript => Box::new(js::JSRunner::try_new()?) as Box<dyn Runner>,
                ScriptType::Lua => Box::new(lua::LuaRunner::try_new()?) as Box<dyn Runner>,
            };
            self.runners.insert(script_type.clone(), runner);
        }
        Ok(self.runners.get(script_type).unwrap().as_ref())
    }

    pub async fn process_script(
        &mut self,
        script_type: ScriptType,
        script: &bytes::Bytes,
        config: Mapping,
    ) -> ProcessOutput {
        let runner = wrap_result!(self.get_or_init_runner(&script_type));
        // tracing::debug!("script: {:?}", script);
        runner
            .process_honey(config, &String::from_utf8_lossy(script))
            .await
    }
}
