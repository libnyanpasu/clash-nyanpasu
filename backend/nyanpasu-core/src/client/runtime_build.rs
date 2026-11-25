//! Application orchestration for content capture, blocking builds, and runtime publication.

use std::sync::Arc;

use async_trait::async_trait;
use nyanpasu_config::{
    application::ClashCore,
    profile::Profiles,
    runtime::executor::{ProfileContentSource, RuntimeArtifact, ScriptRunner},
};
use nyanpasu_profile_transform::{
    BuildFailure, RuntimeBuildInput, RuntimeBuilder, StartScriptRunnerSnafu,
};
use snafu::ResultExt;

use crate::client::{
    artifact_snapshot::{render_final_config, runtime_snapshot_data_from_artifact},
    reconciler::{
        inputs,
        ports::{RuntimeBuildFailure, RuntimeBuildPort, ScriptRunnerFactory},
    },
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
    ) -> Result<Arc<runtime::RuntimeSnapshot>, RuntimeBuildFailure> {
        let script_runner = self.scripts.clone();
        let built = tokio::task::spawn_blocking(move || {
            let scripts = script_runner()
                .context(StartScriptRunnerSnafu)
                .map_err(RuntimeBuildFailure::from)?;
            build_snapshot(
                revision,
                inputs,
                resolved_ports,
                strict_transforms,
                scripts.as_ref(),
            )
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

/// Builds a snapshot on the calling blocking thread. A failure keeps the
/// candidate the build had produced.
pub(in crate::client) fn build_snapshot(
    revision: runtime::RuntimeRevision,
    inputs: inputs::RuntimeInputs,
    resolved_ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
    strict_transforms: bool,
    scripts: &dyn ScriptRunner,
) -> Result<Arc<runtime::RuntimeSnapshot>, RuntimeBuildFailure> {
    let inputs::RuntimeInputs {
        app,
        clash,
        profiles,
        content,
    } = inputs;
    let core = app.core;
    let builtin_enabled = app.enable_builtin_enhanced;
    let input = RuntimeBuildInput {
        profiles: profiles.clone(),
        clash,
        app,
        resolved_ports,
    };
    let artifact = RuntimeBuilder::build(&input, &content, scripts)
        .map_err(|failure| RuntimeBuildFailure::of_build(revision, core, failure))?;
    finish_snapshot(
        revision,
        core,
        artifact,
        &profiles,
        builtin_enabled,
        strict_transforms,
    )
}

fn finish_snapshot(
    revision: runtime::RuntimeRevision,
    core: ClashCore,
    artifact: RuntimeArtifact,
    profiles: &Profiles,
    builtin_enabled: bool,
    strict_transforms: bool,
) -> Result<Arc<runtime::RuntimeSnapshot>, RuntimeBuildFailure> {
    let rendered = if strict_transforms {
        RuntimeBuilder::validate_transforms(&artifact)
    } else {
        Ok(())
    }
    .and_then(|()| render_final_config(&artifact));
    let config_text = match rendered {
        Ok(config_text) => config_text,
        Err(error) => {
            return Err(RuntimeBuildFailure::of_build(
                revision,
                core,
                BuildFailure::complete(error, artifact),
            ));
        }
    };
    let data =
        runtime_snapshot_data_from_artifact(artifact, config_text, profiles, core, builtin_enabled);
    Ok(Arc::new(runtime::RuntimeSnapshot::from_data(
        revision, core, data,
    )))
}

#[cfg(test)]
mod tests;
