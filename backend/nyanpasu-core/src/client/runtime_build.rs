//! Application orchestration for content capture, blocking builds, and runtime publication.

use std::sync::Arc;

use async_trait::async_trait;
use nyanpasu_config::runtime::executor::ProfileContentSource;
use nyanpasu_profile_transform::{
    RuntimeBuildError, RuntimeBuildInput, RuntimeBuilder, StartScriptRunnerSnafu,
};
use snafu::ResultExt;

use crate::client::{
    application_workflow::{
        inputs,
        ports::{RuntimeBuildPort, ScriptRunnerFactory},
    },
    artifact_snapshot::runtime_snapshot_data_from_artifact,
    runtime,
};
use nyanpasu_runtime_values::{core_spec::CoreSpecError, publish::PublishRuntimeError};

/// Resolves the process specification for the selected core using shell-provided paths.
pub type CoreSpecResolver = dyn Fn(
        &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, CoreSpecError>
    + Send
    + Sync;

/// Builds application runtime snapshots using injected content and script execution ports.
pub struct RuntimeBuildService {
    content: Arc<dyn ProfileContentSource + Send + Sync>,
    core_specs: Arc<CoreSpecResolver>,
    paths: runtime::RuntimePaths,
    scripts: Arc<ScriptRunnerFactory>,
}

impl RuntimeBuildService {
    /// Injects content capture, publication paths, core resolution and a blocking runner factory.
    pub fn new(
        content: Arc<dyn ProfileContentSource + Send + Sync>,
        core_specs: Arc<CoreSpecResolver>,
        paths: runtime::RuntimePaths,
        scripts: Arc<ScriptRunnerFactory>,
    ) -> Self {
        Self {
            content,
            core_specs,
            paths,
            scripts,
        }
    }
}

#[async_trait]
impl RuntimeBuildPort for RuntimeBuildService {
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> inputs::FrozenProfileContent {
        let mut content = inputs::FrozenProfileContent::default();
        for uid in crate::state::profiles::ProfilesActor::current_closure(profiles) {
            if let Some(source_path) = profiles
                .items
                .get(&uid)
                .and_then(|item| item.definition.source())
            {
                let path = &source_path.materialized().file;
                let text = self.content.read(path).map_err(|error| error.to_string());
                content.0.insert(path.to_string(), text);
            }
        }
        content
    }

    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, CoreSpecError> {
        (self.core_specs)(core)
    }

    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: inputs::RuntimeInputs,
        resolved_ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<Arc<runtime::RuntimeSnapshot>, RuntimeBuildError> {
        let script_runner = self.scripts.clone();
        let built = tokio::task::spawn_blocking(move || {
            let inputs::RuntimeInputs {
                app,
                clash,
                profiles,
                content,
            } = inputs;
            let core = app.core;
            let builtin_enabled = app.enable_builtin_enhanced;
            let scripts = script_runner().context(StartScriptRunnerSnafu)?;
            let input = RuntimeBuildInput {
                profiles: profiles.clone(),
                clash,
                app,
                resolved_ports,
            };
            let artifact = RuntimeBuilder::build(&input, &content, scripts.as_ref())?;
            if strict_transforms {
                RuntimeBuilder::validate_transforms(&artifact)?;
            }
            let data =
                runtime_snapshot_data_from_artifact(artifact, &profiles, core, builtin_enabled)?;
            Ok(Arc::new(runtime::RuntimeSnapshot::from_data(
                revision, core, data,
            )))
        })
        .await;
        nyanpasu_owner_support::blocking::join(built)
    }

    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), PublishRuntimeError> {
        runtime::write_product(self.paths.product().as_std_path(), snapshot.product_bytes()).await
    }
}

#[cfg(test)]
mod tests;
