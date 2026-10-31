//! Real content capture and script execution preserve lenient and strict build policies.

use super::FsRuntimeBuildAdapter;
use crate::client::{
    application_workflow::{inputs, ports::RuntimeBuildPort},
    runtime,
};
use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use std::sync::Arc;

struct Fixture {
    profiles_dir: std::path::PathBuf,
    builder: Builder,
    _dir: tempfile::TempDir,
}

struct Builder {
    delegate: FsRuntimeBuildAdapter,
}

#[async_trait::async_trait]
impl RuntimeBuildPort for Builder {
    async fn capture_content(&self, profiles: &Profiles) -> inputs::FrozenProfileContent {
        self.delegate.capture_content(profiles).await
    }
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, crate::core::actor_v2::local_host::CoreSpecError>
    {
        self.delegate.core_spec(core)
    }
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: inputs::RuntimeInputs,
        ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict: bool,
    ) -> Result<Arc<runtime::RuntimeSnapshot>, nyanpasu_core::enhance::RuntimeBuildError> {
        self.delegate.build(revision, inputs, ports, strict).await
    }
    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), runtime::PublishRuntimeError> {
        self.delegate.publish(snapshot).await
    }
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("temporary runtime directory");
    let paths = runtime::RuntimePaths::from_resolver(
        &nyanpasu_core::utils::test_support::test_paths(dir.path(), dir.path().join("data")),
    );
    let profiles_dir = dir.path().join("profiles");
    Fixture {
        builder: Builder {
            delegate: FsRuntimeBuildAdapter {
                profiles_dir: profiles_dir.clone(),
                core_specs: Arc::new(crate::client::test_support::runtime_core_spec),
                paths,
                scripts: nyanpasu_platform::enhance::ScriptDirs::under(dir.path()),
            },
        },
        profiles_dir,
        _dir: dir,
    }
}

#[tokio::test]
async fn frozen_content_preserves_lenient_build_and_strict_candidate_policy() {
    use crate::client::application_workflow::ports::RuntimeBuildPort;
    let f = fixture().await;
    let item = crate::enhance::test_support::overlay("missing", "missing.yaml");
    let mut profiles = Profiles::default();
    profiles.global_transforms.push(item.uid.clone());
    profiles.items.insert(item.uid.clone(), item);
    let input = nyanpasu_core::enhance::RuntimeBuildInput {
        profiles: Arc::new(profiles.clone()),
        clash: ClashConfig::default(),
        app: NyanpasuAppConfig::default(),
        resolved_ports: nyanpasu_config::runtime::executor::ResolvedPortBindings {
            mixed_port: 7890,
            ..Default::default()
        },
    };
    let old_content =
        nyanpasu_platform::enhance::FsProfileContentSource::new(f.profiles_dir.clone());
    let script_dirs = f.builder.delegate.scripts.clone();
    let built = tokio::task::spawn_blocking(move || {
        let scripts = nyanpasu_platform::enhance::EnhanceScriptRunner::new(script_dirs).unwrap();
        nyanpasu_core::enhance::RuntimeBuilder::build(&input, &old_content, &scripts)
    })
    .await
    .unwrap();
    assert!(built.is_ok());
    let captured = f.builder.capture_content(&profiles).await;
    let mut revisions = runtime::RuntimeRevisionAllocator::new();
    let build = |revision, strict| {
        f.builder.build(
            revision,
            crate::client::application_workflow::inputs::RuntimeInputs {
                profiles: Arc::new(profiles.clone()),
                clash: ClashConfig::default(),
                app: NyanpasuAppConfig::default(),
                content: captured.clone(),
            },
            nyanpasu_config::runtime::executor::ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
            strict,
        )
    };
    assert!(
        build(revisions.allocate(), false).await.is_ok(),
        "ordinary build keeps D7 passthrough"
    );
    assert!(
        build(revisions.allocate(), true).await.is_err(),
        "TCC candidate rejects missing transform"
    );
}
