//! Profiles source checks precede resource preparation and document publication.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use camino::Utf8PathBuf;
use nyanpasu_config::profile::{ProfileId, Profiles};
use nyanpasu_profile_io::fs_service::ProfileFileService;
use nyanpasu_runtime_values::{
    mutation::{CommitAborted, ConfigDomain},
    profiles::ProfilesError,
    runtime_error::{CoreRejectedConfigSnafu, RuntimeError},
};
use nyanpasu_state_engine::{StateSnapshot, Version};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    client::{
        effects::{
            plan::{ApplicationEffectFields, ClashEffectFields},
            ports::CommitNotifications,
        },
        profiles::ProfilesClient,
        test_support::profiles,
    },
    state::mutation::{
        MutationCoordinator,
        ports::{RuntimeCandidate, RuntimeGate},
    },
};

fn assert_no_prepared_files(directory: &std::path::Path) {
    let mut pending = vec![directory.join("profiles/.profile-materialization-v1")];
    while let Some(path) = pending.pop() {
        if !path.exists() {
            continue;
        }
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            assert!(
                entry.file_type().unwrap().is_dir(),
                "checks must precede resource preparation"
            );
            pending.push(entry.path());
        }
    }
}

#[derive(Default)]
struct Gate {
    reject: AtomicBool,
    staged: Mutex<Vec<std::collections::BTreeMap<String, String>>>,
    events: Mutex<Vec<&'static str>>,
    snapshot: Mutex<Option<StateSnapshot<Profiles>>>,
    document: Mutex<Option<Utf8PathBuf>>,
}
#[async_trait::async_trait]
impl RuntimeGate for Gate {
    async fn check(&self, candidate: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
        let crate::state::mutation::DomainChange::Profiles {
            candidate: next, ..
        } = &candidate.change
        else {
            panic!("profile check carries its candidate document")
        };
        let committed = self.snapshot.lock().unwrap().as_ref().unwrap().load();
        assert_eq!(
            committed.state.revision() + 1,
            next.revision(),
            "check precedes publication"
        );
        let document =
            std::fs::read_to_string(self.document.lock().unwrap().as_ref().unwrap()).unwrap();
        assert!(
            document.contains(&format!("revision: {}", committed.state.revision())),
            "check precedes document persistence"
        );
        assert_no_prepared_files(
            self.document
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .parent()
                .unwrap()
                .as_std_path(),
        );
        self.events.lock().unwrap().push("check");
        self.staged.lock().unwrap().push(candidate.staged);
        if self.reject.load(Ordering::SeqCst) {
            Err(Arc::new(
                CoreRejectedConfigSnafu {
                    core_kind: Some(nyanpasu_core_manager::CoreErrorKind::ConfigCheckFailed),
                    message: "invalid candidate",
                }
                .build(),
            ))
        } else {
            Ok(())
        }
    }
    fn committed(&self, domain: ConfigDomain, version: Version) {
        assert_eq!(domain, ConfigDomain::Profiles);
        let snapshot = self.snapshot.lock().unwrap();
        assert_eq!(snapshot.as_ref().unwrap().load().version, version);
        let document =
            std::fs::read_to_string(self.document.lock().unwrap().as_ref().unwrap()).unwrap();
        assert!(document.contains(&format!(
            "revision: {}",
            snapshot.as_ref().unwrap().load().state.revision()
        )));
        self.events.lock().unwrap().push("committed");
    }
}
impl CommitNotifications for Gate {
    fn application_committed(
        &self,
        _: ApplicationEffectFields,
        _: Vec<nyanpasu_runtime_values::configuration::EffectKind>,
    ) {
        panic!("profiles only")
    }
    fn clash_committed(&self, _: ClashEffectFields) {
        panic!("profiles only")
    }
    fn profiles_committed(&self, _: Vec<ProfileId>) {
        self.events.lock().unwrap().push("effects");
    }
    fn runtime_bound(
        &self,
        _: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>,
        _: bool,
    ) {
        panic!("source cannot publish bindings")
    }
    fn publish_full(&self, _: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>) {
        panic!("source cannot publish sibling domains")
    }
}

async fn fixture() -> (ProfilesClient, Arc<Gate>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let fs = Arc::new(ProfileFileService::new(
        crate::utils::test_support::test_paths(dir.path(), dir.path().join("data")),
    ));
    let coordinator = MutationCoordinator::pending();
    let client = ProfilesClient::new(
        coordinator.clone(),
        Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap(),
        fs.clone(),
        Arc::new(profiles::ok_fetch("proxies: []\n# refreshed\n")),
        fs,
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    let gate = Arc::new(Gate::default());
    *gate.snapshot.lock().unwrap() = Some(client.snapshot_handle());
    *gate.document.lock().unwrap() =
        Some(Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap());
    coordinator.connect(gate.clone(), gate.clone());
    let mut initial = Profiles::default();
    initial.append_item(profiles::file_config_item("selected"));
    initial.append_item(profiles::file_config_item("spare"));
    initial.append_item(profiles::remote_config_item("remote"));
    client.replace(initial).await.unwrap();
    for id in ["selected", "spare", "remote"] {
        std::fs::write(
            dir.path().join("profiles").join(format!("{id}.yaml")),
            "proxies: []\n",
        )
        .unwrap();
    }
    gate.events.lock().unwrap().clear();
    (client, gate, dir)
}

#[tokio::test]
async fn rejected_file_check_receives_new_bytes_and_leaves_no_artifacts() {
    let (client, gate, dir) = fixture().await;
    client
        .set_current(Some(ProfileId("selected".into())))
        .await
        .unwrap();
    gate.reject.store(true, Ordering::SeqCst);
    gate.events.lock().unwrap().clear();
    let document = std::fs::read(dir.path().join("profiles.yaml")).unwrap();
    let error = client
        .save_file(ProfileId("selected".into()), "proxies: []\n# new\n".into())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ProfilesError::Commit {
            source: CommitAborted::RuntimeRefused { .. },
            ..
        }
    ));
    assert_eq!(
        gate.staged.lock().unwrap().last().unwrap()["selected.yaml"],
        "proxies: []\n# new\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("profiles/selected.yaml")).unwrap(),
        "proxies: []\n"
    );
    assert_eq!(
        std::fs::read(dir.path().join("profiles.yaml")).unwrap(),
        document
    );
    assert_eq!(*gate.events.lock().unwrap(), ["check"]);
    assert_no_prepared_files(dir.path());
}

#[tokio::test]
async fn saving_outside_the_closure_skips_checks() {
    let (client, gate, dir) = fixture().await;
    client
        .set_current(Some(ProfileId("selected".into())))
        .await
        .unwrap();
    gate.reject.store(true, Ordering::SeqCst);
    gate.events.lock().unwrap().clear();
    let checks = gate.staged.lock().unwrap().len();
    let report = client
        .save_file(ProfileId("spare".into()), "proxies: []\n# spare\n".into())
        .await
        .unwrap();
    assert!(!report.reaches_runtime);
    assert_eq!(gate.staged.lock().unwrap().len(), checks);
    assert_eq!(*gate.events.lock().unwrap(), ["effects"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("profiles/spare.yaml")).unwrap(),
        "proxies: []\n# spare\n"
    );
}

#[tokio::test]
async fn activation_checks_before_publishing_and_notifying() {
    let (client, gate, _dir) = fixture().await;
    let report = client
        .set_current(Some(ProfileId("selected".into())))
        .await
        .unwrap();
    assert!(report.reaches_runtime);
    assert_eq!(
        client.snapshot().current,
        Some(ProfileId("selected".into()))
    );
    assert_eq!(client.snapshot().revision(), report.snapshot.revision());
    assert_eq!(report.receipt.source_version, *report.version.as_ref());
    assert_eq!(
        *gate.events.lock().unwrap(),
        ["check", "effects", "committed"]
    );
}

#[tokio::test]
async fn subscription_check_rejection_is_recorded_at_its_source() {
    let (client, gate, dir) = fixture().await;
    client
        .set_current(Some(ProfileId("remote".into())))
        .await
        .unwrap();
    gate.reject.store(true, Ordering::SeqCst);
    let document = std::fs::read(dir.path().join("profiles.yaml")).unwrap();
    assert!(matches!(
        client.refresh(ProfileId("remote".into()), None).await,
        Err(ProfilesError::Commit {
            source: CommitAborted::RuntimeRefused { .. },
            ..
        })
    ));
    assert_eq!(
        std::fs::read(dir.path().join("profiles.yaml")).unwrap(),
        document
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("profiles/remote.yaml")).unwrap(),
        "proxies: []\n"
    );
    assert!(
        matches!(&client.sources().entries.iter().find(|entry| entry.profile.0 == "remote").unwrap().outcome, nyanpasu_runtime_values::configuration::SourceOutcome::Rejected { message, .. } if message.contains("invalid candidate"))
    );
}
