use crate::state::config_error::ConfigError;
use nyanpasu_runtime_values::mutation::ConfigDomain;
use nyanpasu_state_engine::{ReplaceIfVersionError, Version, error::StateChangedError};

#[test]
fn publication_conflicts_become_config_version_conflicts() {
    for domain in [ConfigDomain::Application, ConfigDomain::Clash] {
        let error = ConfigError::commit_failure(
            domain,
            ReplaceIfVersionError::State(StateChangedError::StateCasMismatch {
                expected: Version::new(1),
                actual: Version::new(2),
            }),
        );
        let ConfigError::VersionConflict {
            domain: actual_domain,
            expected,
            actual,
            candidate_snapshot_id: None,
        } = error
        else {
            panic!("expected a version conflict: {error:?}");
        };
        assert_eq!(actual_domain, domain);
        assert_eq!(expected, 1);
        assert_eq!(actual, 2);
    }
}

/// A source that fails to commit a checked change names the candidate the
/// check passed: a version conflict, a write or a materialization failure.
/// Other failures, and changes that were never checked, keep nothing.
#[tokio::test]
async fn a_failed_commit_of_a_checked_change_names_its_candidate() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use nyanpasu_runtime_values::{candidate::CandidateSnapshotId, profiles::ProfilesError};

    use super::ports::{CheckPass, KeepChecked};

    const CHECKED: &str = "checkedcandidate00001";
    struct Keeper(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl KeepChecked for Keeper {
        async fn keep(self: Box<Self>) -> Option<CandidateSnapshotId> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(CandidateSnapshotId::parse(CHECKED).unwrap())
        }
    }
    let kept = Arc::new(AtomicUsize::new(0));
    let pass = || CheckPass::new(Keeper(kept.clone()));
    let conflict = || ConfigError::VersionConflict {
        domain: ConfigDomain::Clash,
        expected: 1,
        actual: 2,
        candidate_snapshot_id: None,
    };

    let named = pass().keep_for(conflict()).await;
    assert_eq!(
        named.candidate_snapshot_id().map(|id| id.as_str()),
        Some(CHECKED)
    );
    let profiles = pass()
        .keep_for(ProfilesError::VersionConflict {
            expected: 1,
            actual: 2,
            cleanup_failures: Vec::new(),
            candidate_snapshot_id: None,
        })
        .await;
    assert_eq!(
        profiles.candidate_snapshot_id().map(|id| id.as_str()),
        Some(CHECKED)
    );
    assert_eq!(kept.load(Ordering::SeqCst), 2);

    let stopped = pass()
        .keep_for(ConfigError::OwnerStopped {
            domain: ConfigDomain::Clash,
        })
        .await;
    assert!(stopped.candidate_snapshot_id().is_none());
    let unchecked = CheckPass::default().keep_for(conflict()).await;
    assert!(unchecked.candidate_snapshot_id().is_none());
    assert_eq!(kept.load(Ordering::SeqCst), 2);
}
