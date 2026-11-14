//! Build capture cannot observe a promoted file before its document commits or recovers.

use std::sync::{Arc, Mutex};

use futures_util::FutureExt as _;
use nyanpasu_config::profile::{ProfileId, Profiles};
use nyanpasu_contracts::profiles::ports::{
    MockProfileMaterializationPort, MockSubscriptionFetcher, ProfileMaterializationPort,
};
use nyanpasu_profile_io::fs_service::ProfileFileService;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::super::preparation::RuntimePreparation;
use crate::{
    client::{
        SessionPortResolver,
        application::ApplicationClient,
        clash_config::ClashConfigClient,
        profiles::ProfilesClient,
        runtime::RuntimePaths,
        test_support::{profiles, runtime_build::TestRuntimeBuildPort},
    },
    state::mutation::MutationCoordinator,
};

enum DocumentWrite {
    Commit,
    Fail,
}

async fn capture_during_document_write(outcome: DocumentWrite) {
    let fails = matches!(outcome, DocumentWrite::Fail);
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::utils::test_support::test_paths(dir.path(), dir.path().join("data"));
    let document = paths.profiles_path();
    let file = paths.app_profiles_dir().join("selected.yaml");
    let fs = Arc::new(ProfileFileService::new(paths.clone()));
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let mut materialization = MockProfileMaterializationPort::new();
    let port = fs.clone();
    materialization
        .expect_reconcile()
        .returning(move |snapshot| port.reconcile(snapshot));
    let port = fs.clone();
    materialization
        .expect_prepare_file_first()
        .returning(move |path, resource, revision| {
            port.prepare_file_first(path, resource, revision)
        });
    let port = fs.clone();
    materialization.expect_promote().returning(move |prepared| {
        port.promote(prepared)?;
        entered.lock().unwrap().take().unwrap().send(()).unwrap();
        // The local write has published bytes; its document write is the next step.
        released.lock().unwrap().recv().unwrap();
        Ok(())
    });
    let port = fs.clone();
    materialization
        .expect_compensate()
        .returning(move |prepared| port.compensate(prepared));
    let port = fs.clone();
    materialization
        .expect_complete()
        .returning(move |prepared| port.complete(prepared));
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = ProfilesClient::new(
        MutationCoordinator::isolated(),
        document.clone(),
        fs,
        Arc::new(MockSubscriptionFetcher::new()),
        Arc::new(materialization),
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let mut initial = Profiles::default();
    initial.append_item(profiles::file_config_item("selected"));
    initial.current = Some(ProfileId("selected".into()));
    client.replace(initial).await.unwrap();
    std::fs::write(&file, "proxies: []\n# old\n").unwrap();
    let old_document = std::fs::read(&document).unwrap();
    let before = client.snapshot();
    let before_version = client.snapshot_handle().load().version;
    let application = ApplicationClient::new(
        MutationCoordinator::isolated(),
        nyanpasu_config::application::ReleaseChannel::Stable,
        paths.application_config_path(),
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let clash = ClashConfigClient::new(
        MutationCoordinator::isolated(),
        paths.clash_config_path(),
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let preparation = RuntimePreparation::new(
        application.snapshot_handle(),
        clash.snapshot_handle(),
        client.snapshot_handle(),
        client.content_lock(),
        Arc::new(TestRuntimeBuildPort::from_directory(
            paths.app_profiles_dir().into_std_path_buf(),
            RuntimePaths::from_resolver(&paths),
        )),
        Arc::new(SessionPortResolver::default()),
    );
    let write = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .save_file(ProfileId("selected".into()), "proxies: []\n# new\n".into())
                .await
        }
    });
    entered_rx.await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "proxies: []\n# new\n"
    );
    assert_eq!(client.snapshot().revision(), before.revision());
    let saved_document = document.with_extension("saved");
    if fails {
        std::fs::rename(&document, &saved_document).unwrap();
        std::fs::create_dir(&document).unwrap();
    }
    let mut capture = Box::pin(
        preparation.capture_committed_inputs(application.snapshot().state, clash.snapshot().state),
    );
    assert!(
        capture.as_mut().now_or_never().is_none(),
        "capture must wait for document publication or compensation"
    );
    release.send(()).unwrap();
    let result = write.await.unwrap();
    let (version, inputs) = capture.await;
    if fails {
        assert!(matches!(
            result,
            Err(nyanpasu_runtime_values::profiles::ProfilesError::Commit {
                source: nyanpasu_runtime_values::mutation::CommitAborted::WriteConfig { .. },
                ..
            })
        ));
        std::fs::remove_dir(&document).unwrap();
        std::fs::rename(saved_document, &document).unwrap();
        assert_eq!(version, before_version);
        assert_eq!(inputs.profiles.revision(), before.revision());
        assert_eq!(
            inputs.content.0["selected.yaml"].as_ref().unwrap(),
            "proxies: []\n# old\n"
        );
        assert_eq!(std::fs::read(&document).unwrap(), old_document);
    } else {
        let report = result.unwrap();
        assert_eq!(version, report.version);
        assert_eq!(inputs.profiles.revision(), report.snapshot.revision());
        assert_eq!(
            inputs.content.0["selected.yaml"].as_ref().unwrap(),
            "proxies: []\n# new\n"
        );
        assert_ne!(std::fs::read(&document).unwrap(), old_document);
    }
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capture_waits_for_document_commit_before_reading_promoted_content() {
    capture_during_document_write(DocumentWrite::Commit).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capture_waits_for_compensation_before_reading_committed_content() {
    capture_during_document_write(DocumentWrite::Fail).await;
}
