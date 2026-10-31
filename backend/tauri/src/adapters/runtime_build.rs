//! Filesystem content capture, blocking script execution, and runtime product publication.

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use nyanpasu_core::enhance::{
    RuntimeBuildError, RuntimeBuildInput, RuntimeBuilder, StartScriptRunnerSnafu,
};
use nyanpasu_platform::enhance::{EnhanceScriptRunner, FsProfileContentSource, ScriptDirs};
use snafu::ResultExt;

use nyanpasu_core::{
    actor_v2::local_host::CoreSpecError,
    client::{
        application_workflow::{inputs, ports::RuntimeBuildPort},
        runtime,
        runtime::PublishRuntimeError,
    },
    enhance::runtime_snapshot_data_from_artifact,
};

pub(crate) type CoreSpecResolver = dyn Fn(
        &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, CoreSpecError>
    + Send
    + Sync;

pub(crate) struct FsRuntimeBuildAdapter {
    pub profiles_dir: PathBuf,
    pub core_specs: Arc<CoreSpecResolver>,
    pub paths: runtime::RuntimePaths,
    pub scripts: ScriptDirs,
}

#[async_trait]
impl RuntimeBuildPort for FsRuntimeBuildAdapter {
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> inputs::FrozenProfileContent {
        use nyanpasu_config::runtime::executor::ProfileContentSource;
        let mut content = inputs::FrozenProfileContent::default();
        let source = FsProfileContentSource::new(self.profiles_dir.clone());
        for uid in nyanpasu_core::state::profiles::ProfilesActor::current_closure(profiles) {
            if let Some(source_path) = profiles
                .items
                .get(&uid)
                .and_then(|item| item.definition.source())
            {
                let path = &source_path.materialized().file;
                let text = source.read(path).map_err(|error| error.to_string());
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
        let script_dirs = self.scripts.clone();
        let built = tokio::task::spawn_blocking(move || {
            let inputs::RuntimeInputs {
                app,
                clash,
                profiles,
                content,
            } = inputs;
            let core = app.core;
            let builtin_enabled = app.enable_builtin_enhanced;
            let scripts = EnhanceScriptRunner::new(script_dirs).context(StartScriptRunnerSnafu)?;
            let input = RuntimeBuildInput {
                profiles: profiles.clone(),
                clash,
                app,
                resolved_ports,
            };
            let artifact = RuntimeBuilder::build(&input, &content, &scripts)?;
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
        nyanpasu_core::utils::blocking::join(built)
    }

    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), PublishRuntimeError> {
        runtime::write_product(self.paths.product().as_std_path(), snapshot.product_bytes()).await
    }
}

#[cfg(test)]
mod golden;

#[cfg(test)]
mod tests;
