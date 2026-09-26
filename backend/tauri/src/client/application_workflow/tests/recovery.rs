//! T4: a restore is verified against the receipt, not against the name the
//! lower control plane gave its own transaction result (v2 C4, V04).

use std::sync::Arc;

use super::dirty_graph_with_clients;
use crate::{
    client::{
        runtime,
        runtime_recovery::{
            ObservedRuntime, RecoveryMismatch, RecoveryVerification, verify_recovery_target,
        },
        tests::{IdleServiceAdapter, TestControlEndpoint},
    },
    core::actor_v2::{CoreClient, service_actor::ServiceClient},
};

/// The audited scenario: the app was running A, applied B, then asked to be
/// put back on A. The runtime answers `RolledBack`, which only says that *its*
/// restore request failed and it returned to its own previous state -- still B.
/// Reading that as "recovered to A" is the bug; the receipt for A is what
/// settles it.
#[tokio::test]
async fn a_rolled_back_restore_that_left_the_core_on_b_is_not_a_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let store = runtime::RuntimeSnapshotStore::default();
    let endpoint = TestControlEndpoint::succeeding();
    // The host publishes a running core of the kind the app builds for, so the
    // only thing left for the verification to decide is the document identity.
    endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 4, pid: 91 }),
        Some(nyanpasu_core_manager::CoreKind::Mihomo),
    );
    endpoint.set_source_hash("source-a");
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let service = ServiceClient::spawn(Arc::new(IdleServiceAdapter), 0)
        .await
        .unwrap();
    let (client, _notifier, builder, _, _) = dirty_graph_with_clients(
        &dir,
        core.clone(),
        service,
        false,
        Arc::new(crate::client::SessionPortResolver::new(store.clone())),
    )
    .await;
    builder.release.notify_one();

    client.reconcile().await.unwrap();
    let baseline = store
        .last_confirmed_runtime_receipt()
        .expect("the core confirmed the apply we will later restore");
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    // The apply that produced this receipt is the submission that confirmed the
    // settings it carries, which is the third input the verification needs: no
    // host publishes local-IPC settings back, so only a confirmation of the
    // submission that carried them can settle that half of the target.
    assert_eq!(
        verify_recovery_target(&baseline, &observed, Some(&baseline.binding)),
        RecoveryVerification::Verified,
        "the receipt describes what is actually running right now"
    );

    // A restart of the very same configuration lands in a new epoch, and the
    // runtime stamps that epoch into the managed controller endpoint of the
    // effective document it builds. The restored core therefore reports a
    // different effective hash for the document the receipt describes, and the
    // receipt is still what is running.
    endpoint.set_effective_hash("effective-a-epoch-2");
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    assert_eq!(
        verify_recovery_target(&baseline, &observed, Some(&baseline.binding)),
        RecoveryVerification::Mismatch {
            reasons: vec![RecoveryMismatch::Submission {
                confirmed: Some(baseline.binding.clone())
            }]
        },
        "a historical binding cannot prove a newly observed instance"
    );

    // The core moves to another document and answers every further request
    // with its own rollback.
    endpoint.set_source_hash("source-b");
    endpoint.set_rolls_back(true);
    let error = client
        .reconcile()
        .await
        .expect_err("a rolled back restore is not an applied one");
    assert_eq!(
        error.kind,
        Some(nyanpasu_core_manager::CoreErrorKind::ApplyFailed)
    );

    // A rolled back restore confirmed nothing, so it is passed no confirmation.
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    let RecoveryVerification::Mismatch { reasons } =
        verify_recovery_target(&baseline, &observed, None)
    else {
        panic!("the core is still running the other document, so nothing was recovered");
    };
    assert_eq!(
        reasons,
        vec![
            RecoveryMismatch::Content {
                expected: baseline.binding.revision.source_hash.clone(),
                observed: Some("source-b".into()),
            },
            RecoveryMismatch::Submission { confirmed: None },
        ]
    );

    // The IPC-only case. Local-IPC settings travel with the submission rather
    // than inside the YAML, so the document the core is left on can share the
    // receipt's source hash, its core, its host and its run intent while
    // listening on a different control channel. Every observable comparison
    // passes; the restore that rolled back is what refuses it.
    endpoint.set_source_hash(&baseline.binding.revision.source_hash);
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    let RecoveryVerification::Mismatch { reasons } =
        verify_recovery_target(&baseline, &observed, None)
    else {
        panic!("a rolled back restore never proves the settings it carried");
    };
    assert_eq!(
        reasons,
        vec![RecoveryMismatch::Submission { confirmed: None }],
        "the document matches and the control channel it was submitted with does not"
    );
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .expect("a baseline still exists")
            .revision
            .get(),
        baseline.revision.get(),
        "a failed apply does not move the recovery baseline"
    );
    client.shutdown().await.unwrap();
}
