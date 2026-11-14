//! Source ordering and failure semantics, independent of runtime execution.
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use nyanpasu_config::application::{
    I18nLanguage, NyanpasuAppConfig, NyanpasuAppConfigPatch, ReleaseChannel,
};
use nyanpasu_migration::modules::application::ApplicationFormat;
use nyanpasu_runtime_values::{
    mutation::{CommitAborted, ConfigDomain, RuntimeAftermath},
    runtime_error::{CoreRejectedConfigSnafu, RuntimeError},
};
use nyanpasu_state_engine::{PersistentStateManagerSetup, StateSnapshot, Version};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    client::{
        application::ApplicationClient,
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
    snapshot: StateSnapshot<NyanpasuAppConfig>,
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
        assert_eq!(domain, ConfigDomain::Application);
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
    events: Arc<Mutex<Vec<Step>>>,
    path: Utf8PathBuf,
    previous_bytes: Vec<u8>,
}
impl CommitNotifications for Notifications {
    fn application_committed(
        &self,
        _: ApplicationEffectFields,
        _: Vec<nyanpasu_runtime_values::configuration::EffectKind>,
    ) {
        assert_ne!(
            std::fs::read(&self.path).unwrap(),
            self.previous_bytes,
            "effects follow persistence"
        );
        self.events.lock().unwrap().push(Step::Effects);
    }
    fn clash_committed(&self, _: ClashEffectFields) {
        panic!("only the source's own slice is published")
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
    client: ApplicationClient,
    path: Utf8PathBuf,
    events: Arc<Mutex<Vec<Step>>>,
    gate: Arc<Gate>,
    _dir: tempfile::TempDir,
}
type ApplicationSetup = PersistentStateManagerSetup<NyanpasuAppConfig, ApplicationFormat>;

async fn fixture(rejection: Option<Arc<RuntimeError>>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("application.yaml")).unwrap();
    let manager = ApplicationSetup::builder()
        .config_path(path.clone())
        .assemble()
        .from_state(NyanpasuAppConfig::default());
    std::fs::write(
        &path,
        yaml_serde::to_string(&NyanpasuAppConfig::default()).unwrap(),
    )
    .unwrap();
    let snapshot = manager.snapshot_handle();
    let previous_bytes = std::fs::read(&path).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let coordinator = MutationCoordinator::pending();
    let gate = Arc::new(Gate {
        events: events.clone(),
        snapshot,
        path: path.clone(),
        previous_bytes: previous_bytes.clone(),
        rejection,
        fail_write: Default::default(),
    });
    coordinator.connect(
        Arc::new(Notifications {
            events: events.clone(),
            path: path.clone(),
            previous_bytes: previous_bytes.clone(),
        }),
        gate.clone(),
    );
    let client = ApplicationClient::from_manager(
        coordinator,
        manager,
        ReleaseChannel::Stable,
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
fn runtime_patch(f: &Fixture) -> NyanpasuAppConfigPatch {
    NyanpasuAppConfigPatch {
        enable_builtin_enhanced: Some(!f.client.snapshot().state.enable_builtin_enhanced),
        ..Default::default()
    }
}

// Guards check-before-replace and effects-before-committed in ApplicationActor::commit.
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
async fn language_patch_persists_without_runtime_work() {
    let f = fixture(None).await;
    let language = if f.client.snapshot().state.language == I18nLanguage::English {
        I18nLanguage::Korean
    } else {
        I18nLanguage::English
    };
    let saved = f
        .client
        .patch(NyanpasuAppConfigPatch {
            language: Some(language),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(saved.state.language, language);
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
