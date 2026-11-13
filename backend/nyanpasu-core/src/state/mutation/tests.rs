use std::{error::Error as _, sync::Arc, time::Duration};

use nyanpasu_core_manager::OperationId;
use nyanpasu_state_engine::{
    AckPolicy, AckStatus, ReplaceIfVersionError,
    ack::{PrepareReport, SubscriberAck},
    error::{PrepareAckError, StateChangedError},
};

use crate::{
    client::runtime_error,
    state::mutation::{
        self,
        value::{CheckRecord, MutationConclusion, MutationOutcomeKind, MutationReceipt},
    },
};
use nyanpasu_runtime_values::{
    mutation::{CommitAborted, ConfigDomain},
    profiles::ProfilesError,
    runtime_error::RuntimeError,
};

#[test]
fn boxed_commit_cause_preserves_display_and_source_chain() {
    let yaml = yaml_serde::from_str::<yaml_serde::Value>("[").unwrap_err();
    let causes = [
        ReplaceIfVersionError::WriteConfig(std::io::Error::other("disk full").into()),
        ReplaceIfVersionError::LocalWrite(yaml.into()),
    ];
    for cause in causes {
        let display = cause.to_string();
        let is_io = matches!(cause, ReplaceIfVersionError::WriteConfig(_));
        let error = ProfilesError::Commit {
            uid: None,
            path: None,
            source: mutation::classify_commit_aborted(cause, None),
            cleanup_failures: Vec::new(),
        };
        assert_eq!(error.to_string(), "the profiles change was not committed");
        let aborted = error
            .source()
            .unwrap()
            .downcast_ref::<CommitAborted>()
            .unwrap();
        assert!(matches!(aborted, CommitAborted::WriteConfig { .. }));
        assert_eq!(aborted.to_string(), "failed to write the config");
        let engine = aborted
            .source()
            .unwrap()
            .downcast_ref::<ReplaceIfVersionError>()
            .unwrap();
        assert_eq!(engine.to_string(), display);
        let original = engine.source().unwrap();
        if is_io {
            assert_eq!(
                original
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .to_string(),
                "disk full"
            );
        } else {
            assert!(original.downcast_ref::<yaml_serde::Error>().is_some());
        }
    }

    let cancelled = MutationReceipt {
        degradations: Vec::new(),
        operation_id: OperationId::generate(),
        domain: ConfigDomain::Profiles,
        check: CheckRecord::NotOwed,
        outcome: MutationOutcomeKind::Rejected,
        conclusion: MutationConclusion::Cancelled,
        detail: None,
        cause: None,
    };
    let recovery = mutation::classify_commit_aborted(
        ReplaceIfVersionError::ResourceRecovery {
            cause: std::io::Error::other("write failed").into(),
            recovery_error: std::io::Error::other("recovery failed").into(),
        },
        Some(&cancelled),
    );
    assert!(matches!(
        recovery,
        CommitAborted::RecoverAfterWriteFailure { .. }
    ));
    assert_eq!(
        recovery.to_string(),
        "failed to write the config and failed to recover afterwards; \
         the runtime was rolled back to the previous configuration"
    );
    let Some(ReplaceIfVersionError::ResourceRecovery {
        cause,
        recovery_error,
    }) = recovery
        .source()
        .unwrap()
        .downcast_ref::<ReplaceIfVersionError>()
    else {
        panic!("{recovery:?}");
    };
    assert_eq!(
        cause.downcast_ref::<std::io::Error>().unwrap().to_string(),
        "write failed"
    );
    assert_eq!(
        recovery_error
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .to_string(),
        "recovery failed"
    );

    let runtime = Arc::new(RuntimeError::ShuttingDown);
    for status in [
        AckStatus::Rejected {
            reason: runtime_error::ack_of(runtime.clone()),
        },
        AckStatus::Failed {
            error: runtime_error::ack_of(runtime.clone()),
        },
    ] {
        let refused = matches!(status, AckStatus::Rejected { .. });
        let cause = ReplaceIfVersionError::State(StateChangedError::PrepareAck(PrepareAckError {
            report: PrepareReport {
                subscriber_acks: vec![SubscriberAck {
                    name: "runtime".into(),
                    policy: AckPolicy::Required,
                    elapsed: Duration::ZERO,
                    status,
                }],
            },
        }));
        let display = cause.to_string();
        let aborted = mutation::classify_commit_aborted(cause, None);
        let (errors, expected_display) = match &aborted {
            CommitAborted::RuntimeRefused { errors, .. } if refused => (
                errors,
                "the runtime refused the change: the application is shutting down",
            ),
            CommitAborted::RuntimeFailed { errors, .. } if !refused => (
                errors,
                "the runtime failed while applying the change: the application is shutting down",
            ),
            _ => panic!("{aborted:?}"),
        };
        assert_eq!(aborted.to_string(), expected_display);
        assert_eq!(errors.len(), 1);
        assert!(Arc::ptr_eq(&errors[0], &runtime));
        assert_eq!(
            aborted
                .source()
                .unwrap()
                .downcast_ref::<ReplaceIfVersionError>()
                .unwrap()
                .to_string(),
            display
        );
    }
    let validation = mutation::classify_commit_aborted(
        ReplaceIfVersionError::State(StateChangedError::Validation(anyhow::anyhow!("invalid"))),
        None,
    );
    assert!(matches!(validation, CommitAborted::ValidateState { .. }));
    assert_eq!(
        validation.to_string(),
        "the state change was refused before it was saved"
    );
    assert!(matches!(
        validation
            .source()
            .unwrap()
            .downcast_ref::<ReplaceIfVersionError>(),
        Some(ReplaceIfVersionError::State(StateChangedError::Validation(
            _
        )))
    ));
}
