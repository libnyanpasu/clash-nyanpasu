//! Managed profile files, external watchers and materialization failures.

use crate::service::profile_file::fs_service::ProfileFileService;
use camino::Utf8PathBuf;
use nyanpasu_config::profile::{
    ExternalMode, LocalBinding, ManagedProfilePath, MaterializedFile, ProfileDefinition, ProfileId,
    ProfileMetadata, ProfileSource, ProfileValidationError, Profiles, TransformDefinition,
};
use nyanpasu_core::{
    client::{
        convergence::ConvergenceHealth,
        profiles::ProfilesClient,
        test_support::{
            profiles::{
                add_placeholder_request, cast, external_item, external_path, file_config_item,
                ok_fetch, remote_config_item, round_trip, seeded_profiles, wait_for_source, yaml,
            },
            startup_recovery::{evidence, migrate},
        },
    },
    migration::modules::profiles::ProfilesFormat,
    service::profile_file::ports::SelfProxyPortSource,
    state::{
        mutation::CommitAborted,
        profiles::{
            NewProfileRequest, ProfilesActorMessage, ProfilesError,
            error::ProfileFileError,
            ports::{
                MaterializationReconcileReport, MockProfileMaterializationPort,
                MockSubscriptionFetcher, PreparedMaterialization, ProfileFsPort,
                ProfileMaterializationPort,
            },
            sources::{SourceOrigin, SourceOutcome, SourceStatus},
        },
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tempfile::{TempDir, tempdir};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
fn assert_no_materialization_artifact_files(config_dir: &std::path::Path) {
    let root = config_dir
        .join("profiles")
        .join(".profile-materialization-v1");
    if !root.exists() {
        return;
    }
    let mut leftovers = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                stack.push(path);
            } else {
                leftovers.push(path);
            }
        }
    }
    assert!(
        leftovers.is_empty(),
        "successful materialization must not leave private artifacts: {leftovers:?}"
    );
}

async fn wait_for_updated_at(client: &ProfilesClient, uid: &str) -> Arc<Profiles> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let snapshot = client.snapshot();
        let updated = snapshot
            .items
            .get(&ProfileId(uid.into()))
            .and_then(|item| item.definition.source())
            .and_then(|source| source.materialized().updated_at);
        if updated.is_some() {
            return snapshot;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "profile {uid} was not updated before timeout"
        );
        tokio::task::yield_now().await;
    }
}

/// Counts the commits that staged content and refuses to promote them,
/// so a test can tell content refused before the source transaction from
/// content the transaction itself refused.
fn refusing_materialization_port(staged: Arc<AtomicUsize>) -> Arc<dyn ProfileMaterializationPort> {
    let mut materialization = MockProfileMaterializationPort::new();
    materialization
        .expect_reconcile()
        .returning(|_| Ok(MaterializationReconcileReport::default()));
    materialization
        .expect_prepare_file_first()
        .returning(move |_, _, _| {
            staged.fetch_add(1, Ordering::SeqCst);
            Ok(PreparedMaterialization::new("file".into()))
        });
    materialization
        .expect_promote()
        .returning(|_| Err(ProfileFileError::mock("disk full")));
    materialization.expect_compensate().returning(|_| Ok(()));
    Arc::new(materialization)
}

fn rejected_external(status: &SourceStatus) -> bool {
    matches!(
        &status.outcome,
        SourceOutcome::Rejected { code, .. } if code == SourceOutcome::EXTERNAL_SOURCE_REJECTED
    )
}

struct NoProxyPort;

impl SelfProxyPortSource for NoProxyPort {
    fn mixed_port(&self) -> Option<u16> {
        None
    }
}

/// A Mirror profile over a real file service. The watcher is live once
/// the producers start, so a test writes the target only while the
/// outcome of every event is the same.
struct ExternalFixture {
    client: ProfilesClient,
    target: std::path::PathBuf,
    mirrored: std::path::PathBuf,
    _dirs: [TempDir; 3],
}

impl ExternalFixture {
    async fn new(materialization: Option<Arc<dyn ProfileMaterializationPort>>) -> Self {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target = target_dir.path().join("external.yaml");
        std::fs::write(&target, "proxies: []\n").unwrap();
        let fs = Arc::new(ProfileFileService::new(
            nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path()),
            Arc::new(NoProxyPort),
            nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
        ));
        let client = ProfilesClient::new(
            nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            Arc::new(MockSubscriptionFetcher::new()),
            materialization.unwrap_or(fs as Arc<dyn ProfileMaterializationPort>),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "ext1",
            ExternalMode::Mirror,
            external_path(&target),
        ));
        client.replace(profiles).await.unwrap();
        Self {
            client,
            target,
            mirrored: config_dir.path().join("profiles").join("ext1.yaml"),
            _dirs: [config_dir, data_dir, target_dir],
        }
    }
}

#[tokio::test]
async fn refresh_downloads_writes_and_commits_subscription() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = Arc::new(ProfileFileService::new(
        paths,
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        Arc::new(ok_fetch("proxies: []\n")),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(remote_config_item("r1"));
    client.replace(profiles).await.unwrap();

    let report = client
        .refresh(ProfileId("r1".into()), None)
        .await
        .expect("refresh ok");
    let item = &report.snapshot.items[&ProfileId("r1".into())];
    let source = item.definition.source().unwrap();
    assert!(source.materialized().updated_at.is_some());
    match source {
        ProfileSource::Remote { subscription, .. } => {
            assert_eq!(subscription.upload, Some(1));
        }
        _ => unreachable!(),
    }
    assert_eq!(
        fs.read(&ManagedProfilePath::new("r1.yaml").unwrap())
            .unwrap(),
        "proxies: []\n"
    );
}

#[tokio::test]
async fn external_mirror_change_syncs_copy_and_bumps_updated_at() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let target_dir = tempdir().unwrap();
    let target_path = target_dir.path().join("external.yaml");
    std::fs::write(&target_path, "proxies: []\n").unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = std::sync::Arc::new(ProfileFileService::new(
        paths,
        std::sync::Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        std::sync::Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    client.start_producers().unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(external_item(
        "ext1",
        ExternalMode::Mirror,
        external_path(&target_path),
    ));
    client.replace(profiles).await.unwrap();

    // Production path: OS watcher casts ExternalFileChanged after the
    // external binding target changes. Poll with yields (no sleep).
    std::fs::write(&target_path, "proxies: []\n# touched\n").unwrap();

    let snapshot = wait_for_updated_at(&client, "ext1").await;
    assert!(
        snapshot.items[&ProfileId("ext1".into())]
            .definition
            .source()
            .unwrap()
            .materialized()
            .updated_at
            .is_some()
    );
    let mirrored = config_dir.path().join("profiles").join("ext1.yaml");
    let content = std::fs::read_to_string(mirrored).unwrap();
    assert!(content.contains("# touched"));
}

#[tokio::test]
async fn external_symlink_change_bumps_updated_at_without_copy() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let target_dir = tempdir().unwrap();
    let target_path = target_dir.path().join("external.yaml");
    std::fs::write(&target_path, "proxies: []\n").unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = std::sync::Arc::new(ProfileFileService::new(
        paths,
        std::sync::Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        std::sync::Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    client.start_producers().unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(external_item(
        "ext1",
        ExternalMode::Symlink,
        external_path(&target_path),
    ));
    client.replace(profiles).await.unwrap();

    std::fs::write(&target_path, "proxies: []\n# touched\n").unwrap();
    wait_for_updated_at(&client, "ext1").await;
}

#[tokio::test]
async fn external_background_commit_does_not_bypass_the_mutation_participant() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let target_dir = tempdir().unwrap();
    let target_path = target_dir.path().join("external.yaml");
    std::fs::write(&target_path, "proxies: []\n").unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = std::sync::Arc::new(ProfileFileService::new(
        paths,
        std::sync::Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        std::sync::Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    client.start_producers().unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(external_item(
        "ext1",
        ExternalMode::Mirror,
        external_path(&target_path),
    ));
    profiles.set_current(Some(ProfileId("ext1".into())));
    client.replace(profiles).await.unwrap();

    std::fs::write(&target_path, "proxies: []\n# touched\n").unwrap();
    wait_for_updated_at(&client, "ext1").await;
}

#[tokio::test]
async fn external_watcher_smoke_mirror_real_file_event() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let target_dir = tempdir().unwrap();
    let target_path = target_dir.path().join("external.yaml");
    std::fs::write(&target_path, "proxies: []\n").unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = std::sync::Arc::new(ProfileFileService::new(
        paths,
        std::sync::Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let profiles_path =
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap();
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        profiles_path,
        fs.clone() as Arc<dyn ProfileFsPort>,
        std::sync::Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    client.start_producers().unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(external_item(
        "ext1",
        ExternalMode::Mirror,
        external_path(&target_path),
    ));
    client.replace(profiles).await.unwrap();

    tokio::fs::write(&target_path, "mode: rule\nproxies: []\n")
        .await
        .unwrap();

    let snapshot = wait_for_updated_at(&client, "ext1").await;
    assert!(
        snapshot.items[&ProfileId("ext1".into())]
            .definition
            .source()
            .unwrap()
            .materialized()
            .updated_at
            .is_some()
    );
    let content = std::fs::read_to_string(config_dir.path().join("profiles").join("ext1.yaml"))
        .expect("real external watcher event should mirror the changed file");
    assert!(content.contains("mode: rule"));
}

#[tokio::test]
async fn add_generates_uid_canonical_path_and_writes_initial_file() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = Arc::new(ProfileFileService::new(
        paths,
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();

    let report = client
        .add(
            NewProfileRequest {
                metadata: ProfileMetadata {
                    name: "New".into(),
                    desc: None,
                    custom_name: true,
                },
                definition: file_config_item("placeholder").definition,
                folder: None,
            },
            Some("proxies: []\n".to_string()),
        )
        .await
        .expect("add should succeed");

    assert!(report.degradations.is_empty());
    let created = report
        .created
        .clone()
        .expect("add must report the server-generated uid");
    assert!(report.snapshot.items.contains_key(&created));
    let snapshot = report.snapshot;
    assert_eq!(snapshot.items.len(), 1);
    let (uid, item) = snapshot.items.first().unwrap();
    assert!(uid.0.starts_with('c'), "config uid prefixed with c: {uid}");
    let file = item
        .definition
        .source()
        .unwrap()
        .materialized()
        .file
        .as_str();
    assert_eq!(file, format!("{uid}.yaml"));
    assert_eq!(
        std::fs::read_to_string(config_dir.path().join("profiles").join(file)).unwrap(),
        "proxies: []\n"
    );
}

#[tokio::test]
async fn add_success_leaves_no_materialization_staging_leftovers() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = Arc::new(ProfileFileService::new(
        paths,
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();

    let report = client
        .add(add_placeholder_request(), Some("proxies: []\n".into()))
        .await
        .expect("add success");
    assert!(report.degradations.is_empty());
    let created = report.created.expect("created uid");
    let file = report.snapshot.items[&created]
        .definition
        .source()
        .unwrap()
        .materialized()
        .file
        .as_str();
    assert_eq!(
        std::fs::read_to_string(config_dir.path().join("profiles").join(file)).unwrap(),
        "proxies: []\n"
    );
    assert_no_materialization_artifact_files(config_dir.path());
}

#[tokio::test]
async fn refresh_persist_failure_restores_previous_materialized_bytes() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = Arc::new(ProfileFileService::new(
        paths,
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let profiles_path =
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap();
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        profiles_path.clone(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        Arc::new(ok_fetch("proxies:\n  - name: new\n")),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(remote_config_item("r1"));
    client.replace(profiles).await.unwrap();
    let path = ManagedProfilePath::new("r1.yaml").unwrap();
    fs.write_atomic(&path, "proxies:\n  - name: old\n").unwrap();

    std::fs::remove_file(&profiles_path).unwrap();
    std::fs::create_dir(&profiles_path).unwrap();
    assert!(matches!(
        client.refresh(ProfileId("r1".into()), None).await,
        Err(ProfilesError::Commit {
            source: CommitAborted::WriteConfig { .. },
            ..
        })
    ));
    assert_eq!(
        fs.read(&path).unwrap(),
        "proxies:\n  - name: old\n",
        "file-first compensation must restore old bytes after metadata CAS failure"
    );
}

#[tokio::test]
async fn an_activation_that_does_not_commit_names_the_activated_profile() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = Arc::new(ProfileFileService::new(
        paths,
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let profiles_path =
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap();
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        profiles_path.clone(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        Arc::new(ok_fetch(
            "proxies: []
",
        )),
        fs as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(remote_config_item("r1"));
    client.replace(profiles).await.unwrap();

    std::fs::remove_file(&profiles_path).unwrap();
    std::fs::create_dir(&profiles_path).unwrap();
    let error = client
        .set_current(Some(ProfileId("r1".into())))
        .await
        .expect_err("the state write fails");
    assert!(
        matches!(
            &error,
            ProfilesError::Commit {
                uid: Some(uid),
                path: Some(path),
                source: CommitAborted::WriteConfig { .. },
                ..
            } if uid.0 == "r1" && path.as_str() == "r1.yaml"
        ),
        "{error:?}"
    );
    assert!(
        error.to_string().ends_with("(profile r1, file r1.yaml)"),
        "{error}"
    );
}

#[tokio::test]
async fn delete_unreferenced_managed_profile_removes_file() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = Arc::new(ProfileFileService::new(
        paths,
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(file_config_item("cfg2"));
    client.replace(profiles).await.unwrap();
    let path = ManagedProfilePath::new("cfg2.yaml").unwrap();
    fs.write_atomic(&path, "proxies: []\n").unwrap();

    let report = client
        .delete(ProfileId("cfg2".into()))
        .await
        .expect("delete cfg2");
    assert!(
        report
            .snapshot
            .items
            .get(&ProfileId("cfg2".into()))
            .is_none()
    );
    assert!(
        fs.read(&path).is_err(),
        "cleanup must remove the managed file"
    );
}

#[tokio::test]
async fn replace_definition_kind_switch_rewrites_path_and_cleans_orphan() {
    let config_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(config_dir.path(), data_dir.path());
    let fs = Arc::new(ProfileFileService::new(
        paths,
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let client = ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
        fs.clone() as Arc<dyn ProfileFsPort>,
        Arc::new(MockSubscriptionFetcher::new()),
        fs.clone() as Arc<dyn ProfileMaterializationPort>,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let mut profiles = Profiles::default();
    profiles.append_item(file_config_item("cfg2"));
    client.replace(profiles).await.unwrap();
    let old_path = ManagedProfilePath::new("cfg2.yaml").unwrap();
    fs.write_atomic(&old_path, "proxies: []\n").unwrap();

    let definition = ProfileDefinition::Transform {
        transform: TransformDefinition::Script(nyanpasu_config::profile::ScriptTransform {
            source: ProfileSource::Local {
                binding: LocalBinding::Managed {
                    materialized: MaterializedFile {
                        file: ManagedProfilePath::new("client-supplied.lua").unwrap(),
                        updated_at: Some(time::OffsetDateTime::UNIX_EPOCH),
                    },
                },
            },
            runtime: nyanpasu_config::profile::ScriptRuntime::Lua,
        }),
    };
    let report = client
        .replace_definition(ProfileId("cfg2".into()), definition)
        .await
        .unwrap();
    let item = &report.snapshot.items[&ProfileId("cfg2".into())];
    let source = item.definition.source().unwrap();
    assert_eq!(source.materialized().file.as_str(), "cfg2.lua");
    assert!(source.materialized().updated_at.is_none());
    assert!(
        fs.read(&old_path).is_err(),
        "old managed path must be cleaned up"
    );
    assert_eq!(
        fs.read(&ManagedProfilePath::new("cfg2.lua").unwrap())
            .unwrap(),
        ""
    );
}

/// P1: an external file event is dropped while held and applied once the
/// producers run.
#[tokio::test]
async fn external_file_events_wait_for_start() {
    let fixture = ExternalFixture::new(None).await;
    std::fs::write(&fixture.target, "proxies: []\n# edited\n").unwrap();
    let before = yaml(&fixture.client.snapshot());

    cast(
        &fixture.client,
        ProfilesActorMessage::ExternalFileChanged {
            uid: ProfileId("ext1".into()),
        },
    );
    round_trip(&fixture.client).await;
    assert_eq!(yaml(&fixture.client.snapshot()), before);
    assert!(!fixture.mirrored.exists());
    assert!(fixture.client.sources().entries.is_empty());

    fixture.client.start_producers().unwrap();
    cast(
        &fixture.client,
        ProfilesActorMessage::ExternalFileChanged {
            uid: ProfileId("ext1".into()),
        },
    );
    let status = wait_for_source(&fixture.client, "ext1", |status| {
        matches!(status.outcome, SourceOutcome::Committed { .. })
    })
    .await;
    assert_eq!(status.origin, SourceOrigin::ExternalFile);
    assert!(
        std::fs::read_to_string(&fixture.mirrored)
            .unwrap()
            .contains("# edited")
    );
}

/// R4 (V33): Mirror content that fails validation is Rejected before it
/// reaches the source transaction; the external file keeps what the user
/// wrote and nothing is applied.
#[tokio::test]
async fn invalid_external_content_is_rejected_and_left_untouched() {
    let staged = Arc::new(AtomicUsize::new(0));
    let fixture =
        ExternalFixture::new(Some(refusing_materialization_port(Arc::clone(&staged)))).await;
    let before = yaml(&fixture.client.snapshot());
    fixture.client.start_producers().unwrap();

    std::fs::write(&fixture.target, "mode: rule\n").unwrap();
    cast(
        &fixture.client,
        ProfilesActorMessage::ExternalFileChanged {
            uid: ProfileId("ext1".into()),
        },
    );
    let status = wait_for_source(&fixture.client, "ext1", rejected_external).await;
    assert!(matches!(
        &status.outcome,
        SourceOutcome::Rejected { message, .. } if message.contains("proxies")
    ));
    assert_eq!(status.health, ConvergenceHealth::Blocked);
    assert_eq!(
        staged.load(Ordering::SeqCst),
        0,
        "invalid content never reaches the source transaction"
    );
    assert_eq!(
        std::fs::read_to_string(&fixture.target).unwrap(),
        "mode: rule\n"
    );
    assert_eq!(yaml(&fixture.client.snapshot()), before);
}

/// R5 (V33): valid external content that the source transaction refuses
/// is Rejected the same way, with the external file untouched.
#[tokio::test]
async fn external_content_refused_by_the_source_transaction_is_rejected() {
    let staged = Arc::new(AtomicUsize::new(0));
    let fixture =
        ExternalFixture::new(Some(refusing_materialization_port(Arc::clone(&staged)))).await;
    let before = yaml(&fixture.client.snapshot());
    fixture.client.start_producers().unwrap();

    std::fs::write(&fixture.target, "proxies: []\n# edited\n").unwrap();
    cast(
        &fixture.client,
        ProfilesActorMessage::ExternalFileChanged {
            uid: ProfileId("ext1".into()),
        },
    );
    let status = wait_for_source(&fixture.client, "ext1", rejected_external).await;
    assert_eq!(status.health, ConvergenceHealth::Blocked);
    assert!(
        staged.load(Ordering::SeqCst) >= 1,
        "valid content reaches the source transaction before it is refused"
    );
    assert_eq!(
        std::fs::read_to_string(&fixture.target).unwrap(),
        "proxies: []\n# edited\n"
    );
    assert_eq!(yaml(&fixture.client.snapshot()), before);
}

async fn startup_client(
    paths: &nyanpasu_paths::PathResolver,
    shutdown: &CancellationToken,
    tasks: &TaskTracker,
) -> std::result::Result<ProfilesClient, nyanpasu_core::client::profiles::Error> {
    let service = Arc::new(ProfileFileService::new(
        paths.clone(),
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    ProfilesClient::new(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        paths.profiles_path(),
        service.clone(),
        service.clone(),
        service,
        shutdown.child_token(),
        tasks,
    )
    .await
}

#[tokio::test]
async fn current_not_config_still_fails_without_rewriting_profiles() {
    let root = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(
        root.path().join("config"),
        root.path().join("data"),
    );
    paths.create_base_dirs().unwrap();
    let mut profiles = seeded_profiles();
    profiles.current = Some(ProfileId("ovl1".into()));
    std::fs::write(
        paths.profiles_path(),
        serde_yaml::to_string(&profiles).unwrap(),
    )
    .unwrap();
    migrate(&paths);
    migrate(&paths);
    let before = std::fs::read(paths.profiles_path()).unwrap();
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let result = startup_client(&paths, &shutdown, &tasks).await;
    assert!(
        matches!(result, Err(nyanpasu_core::client::profiles::Error::ValidateProfiles { ref errors })
        if errors.iter().any(|error| matches!(error, ProfileValidationError::CurrentNotConfig(id) if id.0 == "ovl1")))
    );
    assert_eq!(std::fs::read(paths.profiles_path()).unwrap(), before);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn dangling_current_does_not_sanitize_invalid_global_transforms() {
    let root = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(
        root.path().join("config"),
        root.path().join("data"),
    );
    paths.create_base_dirs().unwrap();
    let mut profiles = seeded_profiles();
    profiles.current = Some(ProfileId("missing".into()));
    profiles.global_transforms = vec![ProfileId("missing-transform".into())];
    std::fs::write(
        paths.profiles_path(),
        serde_yaml::to_string(&profiles).unwrap(),
    )
    .unwrap();
    migrate(&paths);
    migrate(&paths);
    let before = std::fs::read(paths.profiles_path()).unwrap();
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let result = startup_client(&paths, &shutdown, &tasks).await;
    assert!(
        matches!(result, Err(nyanpasu_core::client::profiles::Error::ValidateProfiles { ref errors })
        if errors.iter().any(|error| matches!(error,
            ProfileValidationError::TransformTargetNotFound { target, .. } if target.0 == "missing-transform")))
    );
    assert_eq!(std::fs::read(paths.profiles_path()).unwrap(), before);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn dangling_current_is_persisted_without_removing_global_transforms() {
    let root = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(
        root.path().join("config"),
        root.path().join("data"),
    );
    paths.create_base_dirs().unwrap();
    let mut profiles = seeded_profiles();
    profiles.current = Some(ProfileId("missing".into()));
    profiles.global_transforms = vec![ProfileId("ovl1".into())];
    std::fs::write(
        paths.profiles_path(),
        serde_yaml::to_string(&profiles).unwrap(),
    )
    .unwrap();
    migrate(&paths);
    migrate(&paths);
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = startup_client(&paths, &shutdown, &tasks).await.unwrap();
    assert_eq!(client.snapshot().current, None);
    assert_eq!(
        client.snapshot().global_transforms,
        profiles.global_transforms
    );
    use nyanpasu_core::format::Format as _;
    let saved: Profiles = ProfilesFormat::default()
        .deserialize(std::fs::read(paths.profiles_path()).unwrap().as_slice())
        .unwrap();
    assert_eq!(saved.current, None);
    assert_eq!(saved.global_transforms, profiles.global_transforms);
    assert_eq!(
        serde_yaml::to_value(&saved.items).unwrap(),
        serde_yaml::to_value(&profiles.items).unwrap()
    );
    assert_eq!(saved.revision(), profiles.revision() + 1);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn legacy_dangling_current_is_repaired_before_migration_validation() {
    for revision in [None, Some(3)] {
        let root = tempdir().unwrap();
        let paths = nyanpasu_core::utils::test_support::test_paths(
            root.path().join("config"),
            root.path().join("data"),
        );
        paths.create_base_dirs().unwrap();
        let legacy = "current: [missing]\nchain: []\nitems:\n  - {uid: cfg, type: local, name: CFG, file: cfg.yaml}\n";
        if let Some(revision) = revision {
            nyanpasu_core::migration::fs::write_document(
                paths.profiles_path().as_std_path(),
                "profiles",
                revision,
                serde_yaml::from_str(legacy).unwrap(),
                None,
            )
            .unwrap();
        } else {
            std::fs::write(paths.profiles_path(), legacy).unwrap();
        }
        migrate(&paths);
        migrate(&paths);
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = startup_client(&paths, &shutdown, &tasks).await.unwrap();
        assert_eq!(client.snapshot().current, None);
        assert!(
            client
                .snapshot()
                .items
                .contains_key(&ProfileId("cfg".into()))
        );
        use nyanpasu_core::format::Format as _;
        let saved: Profiles = ProfilesFormat::default()
            .deserialize(std::fs::read(paths.profiles_path()).unwrap().as_slice())
            .unwrap();
        assert_eq!(saved.current, None);
        saved.validate().unwrap();
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }
}

#[tokio::test]
async fn materialization_directory_node_problems_rebuild_before_client_spawn() {
    for relative in ["", "staging", "journal/state-first/prepared"] {
        let root = tempdir().unwrap();
        let paths = nyanpasu_core::utils::test_support::test_paths(
            root.path().join("config"),
            root.path().join("data"),
        );
        paths.create_base_dirs().unwrap();
        migrate(&paths);
        migrate(&paths);
        let private_root = paths.app_profiles_dir().join(".profile-materialization-v1");
        let blocked = if relative.is_empty() {
            private_root
        } else {
            private_root.join(relative)
        };
        std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
        std::fs::write(&blocked, b"preserve this node").unwrap();
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = startup_client(&paths, &shutdown, &tasks).await.unwrap();
        let preserved = evidence(blocked.as_std_path());
        assert_eq!(preserved.len(), 1);
        assert_eq!(std::fs::read(&preserved[0]).unwrap(), b"preserve this node");
        let service = ProfileFileService::new(
            paths.clone(),
            Arc::new(NoProxyPort),
            nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
        );
        let path = ManagedProfilePath::new("usable.yaml").unwrap();
        let prepared = service
            .prepare_state_first(
                &path,
                nyanpasu_core::state::profiles::ports::MaterializationResource::File {
                    content: "usable: true\n".into(),
                },
                client.snapshot().revision() + 1,
            )
            .unwrap();
        service.promote(&prepared).unwrap();
        assert_eq!(service.read(&path).unwrap(), "usable: true\n");
        service.complete(&prepared).unwrap();
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }
}

#[tokio::test]
async fn materialization_preparation_failure_keeps_its_typed_source() {
    let root = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(
        root.path().join("config"),
        root.path().join("data"),
    );
    std::fs::create_dir_all(paths.app_config_dir()).unwrap();
    let blocked = paths.app_profiles_dir();
    std::fs::write(&blocked, "a file blocks the private directory").unwrap();
    let service = Arc::new(ProfileFileService::new(
        paths.clone(),
        Arc::new(NoProxyPort),
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let jobs = nyanpasu_core::client::jobs::test_client_with_owner(shutdown.clone(), &tasks).await;
    let result = ProfilesClient::new_with_jobs(
        nyanpasu_core::state::mutation::MutationCoordinator::isolated(),
        paths.profiles_path(),
        jobs,
        service.clone(),
        service.clone(),
        service,
        shutdown.child_token(),
        &tasks,
    )
    .await;
    assert!(matches!(
        result,
        Err(
            nyanpasu_core::client::profiles::Error::PrepareMaterialization {
                source: ProfileFileError::UnexpectedNode {
                    expected: nyanpasu_core::state::profiles::error::ExpectedNode::RealDirectory,
                    ..
                },
            }
        )
    ));
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn materialization_symlink_is_preserved_without_touching_target() {
    let root = tempdir().unwrap();
    let paths = nyanpasu_core::utils::test_support::test_paths(
        root.path().join("config"),
        root.path().join("data"),
    );
    paths.create_base_dirs().unwrap();
    migrate(&paths);
    migrate(&paths);
    std::fs::create_dir_all(paths.app_profiles_dir()).unwrap();
    let blocked = paths.app_profiles_dir().join(".profile-materialization-v1");
    let target = root.path().join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("kept"), b"target data").unwrap();
    #[cfg(unix)]
    let link = std::os::unix::fs::symlink(&target, &blocked);
    #[cfg(windows)]
    let link = std::os::windows::fs::symlink_dir(&target, &blocked);
    if let Err(error) = &link
        && error.kind() == std::io::ErrorKind::PermissionDenied
    {
        eprintln!("SKIPPED materialization symlink fixture: symlink privilege unavailable");
        return;
    }
    link.unwrap();
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let _client = startup_client(&paths, &shutdown, &tasks).await.unwrap();
    assert!(blocked.is_dir());
    assert!(
        !std::fs::symlink_metadata(&blocked)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let evidence = evidence(blocked.as_std_path());
    assert_eq!(evidence.len(), 1);
    assert!(
        std::fs::symlink_metadata(&evidence[0])
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(target.join("kept")).unwrap(), b"target data");
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}
