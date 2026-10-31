//! Runtime assembly for workflow tests, with script execution replaced by an echo port.

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use nyanpasu_config::{
    application::ClashCore,
    profile::{Profiles, ScriptRuntime},
    runtime::{
        executor::{PortError, ResolvedPortBindings, ScriptRunner, StepLogEntry},
        value::ConfigValue,
    },
};
use nyanpasu_core::enhance::{RuntimeBuildError, RuntimeBuildInput, RuntimeBuilder};
use nyanpasu_core_manager::CoreSpec;

use crate::{
    client::{
        application_workflow::{
            inputs::{FrozenProfileContent, RuntimeInputs},
            ports::RuntimeBuildPort,
        },
        runtime::{self, PublishRuntimeError, RuntimePaths, RuntimeRevision, RuntimeSnapshot},
    },
    core::actor_v2::local_host::CoreSpecError,
    state::profiles::ProfilesActor,
};

pub(crate) struct TestRuntimeBuildPort {
    pub profiles_dir: PathBuf,
    pub paths: RuntimePaths,
}

struct EchoScripts;

impl ScriptRunner for EchoScripts {
    fn run(
        &self,
        _runtime: ScriptRuntime,
        _source: &str,
        config: &ConfigValue,
        _logs: &mut Vec<StepLogEntry>,
    ) -> Result<ConfigValue, PortError> {
        Ok(config.clone())
    }

    fn eval_item_predicate(&self, _expr: &str, _item: &ConfigValue) -> Result<bool, PortError> {
        Err("script predicates are not configured in this fixture".into())
    }

    fn eval_item_expr(&self, _expr: &str, _item: &ConfigValue) -> Result<ConfigValue, PortError> {
        Err("script expressions are not configured in this fixture".into())
    }
}

#[async_trait]
impl RuntimeBuildPort for TestRuntimeBuildPort {
    async fn capture_content(&self, profiles: &Profiles) -> FrozenProfileContent {
        let mut content = FrozenProfileContent::default();
        for uid in ProfilesActor::current_closure(profiles) {
            if let Some(source) = profiles
                .items
                .get(&uid)
                .and_then(|item| item.definition.source())
            {
                let file = &source.materialized().file;
                let path = self.profiles_dir.join(file.as_path());
                let text = std::fs::read_to_string(&path)
                    .map_err(|error| format!("read profile content {}: {error}", path.display()));
                content.0.insert(file.to_string(), text);
            }
        }
        content
    }

    fn core_spec(&self, core: &ClashCore) -> Result<CoreSpec, CoreSpecError> {
        super::runtime_core_spec(core)
    }

    async fn build(
        &self,
        revision: RuntimeRevision,
        inputs: RuntimeInputs,
        resolved_ports: ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<Arc<RuntimeSnapshot>, RuntimeBuildError> {
        let built = tokio::task::spawn_blocking(move || {
            let core = inputs.app.core;
            let builtin_enabled = inputs.app.enable_builtin_enhanced;
            let input = RuntimeBuildInput {
                profiles: inputs.profiles.clone(),
                app: inputs.app,
                clash: inputs.clash,
                resolved_ports,
            };
            let artifact = RuntimeBuilder::build(&input, &inputs.content, &EchoScripts)?;
            if strict_transforms {
                RuntimeBuilder::validate_transforms(&artifact)?;
            }
            let data = crate::enhance::runtime_snapshot_data_from_artifact(
                artifact,
                &inputs.profiles,
                core,
                builtin_enabled,
            )?;
            Ok(Arc::new(RuntimeSnapshot::from_data(revision, core, data)))
        })
        .await;
        crate::utils::blocking::join(built)
    }

    async fn publish(&self, snapshot: &RuntimeSnapshot) -> Result<(), PublishRuntimeError> {
        runtime::write_product(self.paths.product().as_std_path(), snapshot.product_bytes()).await
    }
}
