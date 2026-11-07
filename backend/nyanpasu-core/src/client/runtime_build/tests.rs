//! In-memory content and a script double preserve lenient and strict build policies.

use std::{collections::BTreeMap, sync::Arc};

use nyanpasu_config::{
    application::NyanpasuAppConfig,
    clash::config::ClashConfig,
    profile::{ManagedProfilePath, Profiles, ScriptRuntime},
    runtime::{
        executor::{PortError, ProfileContentSource, ScriptRunner, StepLogEntry},
        value::ConfigValue,
    },
};

use super::RuntimeBuildService;
use crate::client::{application_workflow::ports::RuntimeBuildPort, runtime};

#[derive(Default)]
struct MemoryContent(BTreeMap<String, String>);

impl ProfileContentSource for MemoryContent {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        self.0
            .get(&path.to_string())
            .cloned()
            .ok_or_else(|| format!("missing content: {path}").into())
    }
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

struct Fixture {
    builder: RuntimeBuildService,
    _dir: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("temporary runtime directory");
    let paths = runtime::RuntimePaths::from_resolver(&crate::utils::test_support::test_paths(
        dir.path(),
        dir.path().join("data"),
    ));
    Fixture {
        builder: RuntimeBuildService::new(
            Arc::new(MemoryContent::default()),
            Arc::new(crate::client::test_support::runtime_core_spec),
            paths,
            Arc::new(|| Ok(Box::new(EchoScripts))),
        ),
        _dir: dir,
    }
}

#[tokio::test]
async fn frozen_content_preserves_lenient_build_and_strict_candidate_policy() {
    let f = fixture().await;
    let item = nyanpasu_profile_transform::test_support::overlay("missing", "missing.yaml");
    let mut profiles = Profiles::default();
    profiles.global_transforms.push(item.uid.clone());
    profiles.items.insert(item.uid.clone(), item);
    let input = nyanpasu_profile_transform::RuntimeBuildInput {
        profiles: Arc::new(profiles.clone()),
        clash: ClashConfig::default(),
        app: NyanpasuAppConfig::default(),
        resolved_ports: nyanpasu_config::runtime::executor::ResolvedPortBindings {
            mixed_port: 7890,
            ..Default::default()
        },
    };
    let old_content = f.builder.content.clone();
    let script_runner = f.builder.scripts.clone();
    let built = tokio::task::spawn_blocking(move || {
        let scripts = script_runner().unwrap();
        nyanpasu_profile_transform::RuntimeBuilder::build(
            &input,
            old_content.as_ref(),
            scripts.as_ref(),
        )
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
