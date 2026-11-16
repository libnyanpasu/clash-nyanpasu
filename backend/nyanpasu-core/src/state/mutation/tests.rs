use crate::state::mutation;
use nyanpasu_runtime_values::{mutation::CommitAborted, profiles::ProfilesError};
use nyanpasu_state_engine::{ReplaceIfVersionError, error::StateChangedError};
use std::error::Error as _;

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
            source: mutation::classify_commit_aborted(cause),
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

    let recovery = mutation::classify_commit_aborted(ReplaceIfVersionError::ResourceRecovery {
        cause: std::io::Error::other("write failed").into(),
        recovery_error: std::io::Error::other("recovery failed").into(),
    });
    assert!(matches!(
        recovery,
        CommitAborted::RecoverAfterWriteFailure { .. }
    ));
    assert_eq!(
        recovery.to_string(),
        "failed to write the config and failed to recover afterwards"
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

    let validation = mutation::classify_commit_aborted(ReplaceIfVersionError::State(
        StateChangedError::Validation(anyhow::anyhow!("invalid")),
    ));
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
