//! Source ordering and failure semantics, independent of runtime execution.
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use nyanpasu_config::clash::config::{
    ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch,
};
use nyanpasu_migration::modules::clash_config::ClashConfigFormat;
use nyanpasu_runtime_values::{
    mutation::{CommitAborted, ConfigDomain, RuntimeAftermath},
    runtime_error::{CoreRejectedConfigSnafu, RuntimeError},
};
use nyanpasu_state_engine::{PersistentStateManagerSetup, StateSnapshot, Version};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    client::{
        clash_config::ClashConfigClient,
        effects::{
            plan::{ApplicationEffectFields, ClashEffectFields},
            ports::CommitNotifications,
        },
    },
    state::{
        config_error::ConfigError,
        mutation::{
            MutationCoordinator,
            ports::{RuntimeCandidate, RuntimeGate},
        },
    },
};

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Check,
    Effects,
    Committed,
}

struct Gate {
    events: Arc<Mutex<Vec<Step>>>,
    snapshot: StateSnapshot<ClashConfig>,
    path: Utf8PathBuf,
    previous_bytes: Vec<u8>,
    rejection: Option<Arc<RuntimeError>>,
    fail_write: std::sync::atomic::AtomicBool,
}
#[async_trait::async_trait]
impl RuntimeGate for Gate {
    async fn check(&self, _: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
        assert_eq!(
            std::fs::read(&self.path).unwrap(),
            self.previous_bytes,
            "check must precede persistence"
        );
        self.events.lock().unwrap().push(Step::Check);
        if self.fail_write.load(std::sync::atomic::Ordering::SeqCst) {
            std::fs::remove_file(&self.path).unwrap();
            std::fs::create_dir(&self.path).unwrap();
        }
        match &self.rejection {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
    fn committed(&self, domain: ConfigDomain, version: Version) {
        assert_eq!(domain, ConfigDomain::Clash);
        assert_eq!(
            self.snapshot.load().version,
            version,
            "publication precedes notification"
        );
        assert_ne!(std::fs::read(&self.path).unwrap(), self.previous_bytes);
        self.events.lock().unwrap().push(Step::Committed);
    }
}
struct Notifications {
    snapshot: StateSnapshot<ClashConfig>,
    events: Arc<Mutex<Vec<Step>>>,
    path: Utf8PathBuf,
}
impl CommitNotifications for Notifications {
    fn application_committed(
        &self,
        _: ApplicationEffectFields,
        _: Vec<nyanpasu_runtime_values::configuration::EffectKind>,
    ) {
        panic!("only the source's own slice is published")
    }
    fn clash_committed(&self, _: ClashEffectFields) {
        let persisted: ClashConfig =
            yaml_serde::from_slice(&std::fs::read(&self.path).unwrap()).unwrap();
        assert_eq!(
            persisted.enable_tun_mode,
            self.snapshot.load().state.enable_tun_mode
        );
        self.events.lock().unwrap().push(Step::Effects);
    }
    fn profiles_committed(&self, _: Vec<nyanpasu_config::profile::ProfileId>) {
        panic!("only the source's own slice is published")
    }
    fn runtime_bound(
        &self,
        _: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>,
        _: bool,
    ) {
        panic!("a source cannot publish runtime bindings")
    }
    fn publish_full(&self, _: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>) {
        panic!("a source cannot publish sibling snapshots")
    }
}

struct Fixture {
    client: ClashConfigClient,
    path: Utf8PathBuf,
    events: Arc<Mutex<Vec<Step>>>,
    gate: Arc<Gate>,
    _dir: tempfile::TempDir,
}
type ClashSetup = PersistentStateManagerSetup<ClashConfig, ClashConfigFormat>;

async fn fixture(rejection: Option<Arc<RuntimeError>>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap();
    let manager = ClashSetup::builder()
        .config_path(path.clone())
        .assemble()
        .from_state(ClashConfig::default());
    std::fs::write(
        &path,
        yaml_serde::to_string(&manager.snapshot_handle().load().state).unwrap(),
    )
    .unwrap();
    let snapshot = manager.snapshot_handle();
    let previous_bytes = std::fs::read(&path).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let coordinator = MutationCoordinator::pending();
    let gate = Arc::new(Gate {
        events: events.clone(),
        snapshot: snapshot.clone(),
        path: path.clone(),
        previous_bytes: previous_bytes.clone(),
        rejection,
        fail_write: Default::default(),
    });
    coordinator.connect(
        Arc::new(Notifications {
            snapshot,
            events: events.clone(),
            path: path.clone(),
        }),
        gate.clone(),
    );
    let client = ClashConfigClient::from_manager(
        coordinator,
        manager,
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    Fixture {
        client,
        path,
        events,
        gate,
        _dir: dir,
    }
}
fn runtime_patch(f: &Fixture) -> ClashConfigPatch {
    ClashConfigPatch {
        enable_tun_mode: Some(!f.client.snapshot().state.enable_tun_mode),
        ..Default::default()
    }
}

// Guards check-before-replace and effects-before-committed in ClashConfigActor::commit.
#[tokio::test]
async fn runtime_patch_checks_persists_publishes_effects_then_notifies() {
    let f = fixture(None).await;
    let before = f.client.snapshot().version;
    let saved = f.client.patch(runtime_patch(&f)).await.unwrap();
    assert!(saved.version.as_ref() > before.as_ref());
    assert!(saved.reaches_runtime);
    assert!(saved.operation_id.is_some());
    assert_eq!(
        *f.events.lock().unwrap(),
        [Step::Check, Step::Effects, Step::Committed]
    );
}

// Guards propagating gate rejection before replace_if_version.
#[tokio::test]
async fn rejected_check_preserves_bytes_version_and_notifications() {
    let error = Arc::new(
        CoreRejectedConfigSnafu {
            core_kind: Some(nyanpasu_core_manager::CoreErrorKind::InvalidConfig),
            message: "invalid candidate",
        }
        .build(),
    );
    let f = fixture(Some(error)).await;
    let before = f.client.snapshot();
    let bytes = std::fs::read(&f.path).unwrap();
    assert!(matches!(
        f.client.patch(runtime_patch(&f)).await,
        Err(ConfigError::Commit {
            source: CommitAborted::RuntimeRefused {
                runtime: RuntimeAftermath::Untouched,
                ..
            },
            ..
        })
    ));
    assert_eq!(f.client.snapshot().version, before.version);
    assert_eq!(std::fs::read(&f.path).unwrap(), bytes);
    assert_eq!(*f.events.lock().unwrap(), [Step::Check]);
}

// Guards restricting committed notifications to ReplaceIfVersionResult::Replaced.
#[tokio::test]
async fn failed_write_does_not_notify_runtime_or_effects() {
    let f = fixture(None).await;
    let before = f.client.snapshot().version;
    f.gate
        .fail_write
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let result = f.client.patch(runtime_patch(&f)).await;
    assert!(matches!(
        result,
        Err(ConfigError::Commit {
            source: CommitAborted::WriteConfig {
                runtime: RuntimeAftermath::Untouched,
                ..
            },
            ..
        })
    ));
    assert_eq!(f.client.snapshot().version, before);
    assert_eq!(*f.events.lock().unwrap(), [Step::Check]);
}

// Guards reaches_runtime gating both check and committed.
#[tokio::test]
async fn policy_patch_persists_without_runtime_work() {
    let f = fixture(None).await;
    let saved = f
        .client
        .patch(
            serde_json::from_value(serde_json::json!({
                "break_connection": {"on_mode_change": false}
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(!saved.state.break_connection.on_mode_change);
    assert!(!saved.reaches_runtime);
    assert_eq!(saved.operation_id, None);
    assert_eq!(*f.events.lock().unwrap(), [Step::Effects]);
}

// Guards classifying an unavailable check as RuntimeFailed without touching runtime or source.
#[tokio::test]
async fn unavailable_check_is_failed_and_preserves_the_source() {
    let error = Arc::new(
        nyanpasu_runtime_values::runtime_error::CheckUnavailableSnafu {
            reason: nyanpasu_runtime_values::check::RuntimeCheckUnavailable::Backend {
                kind: Some(nyanpasu_core_manager::CoreErrorKind::BackendUnavailable),
                message: "check unavailable".into(),
                retryable: true,
            },
        }
        .build(),
    );
    let f = fixture(Some(error)).await;
    let before = f.client.snapshot().version;
    let bytes = std::fs::read(&f.path).unwrap();
    assert!(matches!(
        f.client.patch(runtime_patch(&f)).await,
        Err(ConfigError::Commit {
            source: CommitAborted::RuntimeFailed {
                runtime: RuntimeAftermath::Untouched,
                ..
            },
            ..
        })
    ));
    assert_eq!(f.client.snapshot().version, before);
    assert_eq!(std::fs::read(&f.path).unwrap(), bytes);
    assert_eq!(*f.events.lock().unwrap(), [Step::Check]);
}

// Guards applying PatchOverrides and Replace through the same checked commit path.
#[tokio::test]
async fn overrides_and_replacement_check_before_persistence() {
    let f = fixture(None).await;
    let saved = f
        .client
        .patch_overrides(ClashGuardOverridesPatch {
            mode: Some(nyanpasu_config::clash::config::overrides::Mode::Global),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(saved.reaches_runtime);
    assert_eq!(
        *f.events.lock().unwrap(),
        [Step::Check, Step::Effects, Step::Committed]
    );

    let f = fixture(None).await;
    let mut next = f.client.snapshot().state;
    next.enable_tun_mode = !next.enable_tun_mode;
    assert!(f.client.replace(next).await.unwrap().reaches_runtime);
    assert_eq!(
        *f.events.lock().unwrap(),
        [Step::Check, Step::Effects, Step::Committed]
    );
}

// Guards comparing projected values rather than the fields present in the patch.
#[tokio::test]
async fn unchanged_runtime_field_does_not_check_or_notify() {
    let f = fixture(None).await;
    let saved = f
        .client
        .patch(ClashConfigPatch {
            enable_tun_mode: Some(f.client.snapshot().state.enable_tun_mode),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!saved.reaches_runtime);
    assert_eq!(saved.operation_id, None);
    assert_eq!(*f.events.lock().unwrap(), [Step::Effects]);
}
