//! Receipts for background profile sources (T10 §3). Tauri-free.
//!
//! ProfilesActor keeps the latest result per profile and publishes the whole
//! set through the injected watch sender; readers never reach actor state.

use indexmap::IndexMap;
use nyanpasu_config::profile::{ProfileId, Profiles};
use tokio::sync::watch;

use crate::client::convergence::ConvergenceHealth;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    ScheduledRefresh,
    ManualRefresh,
    ExternalFile,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOutcome {
    Committed {
        operation_id: Option<String>,
    },
    /// Its result no longer applied to the profile and was dropped (V31).
    Superseded {
        reason: String,
    },
    Failed {
        message: String,
    },
    /// The content reached the source transaction and was refused; nothing
    /// was committed and an external file is never rewritten (V33).
    Rejected {
        code: String,
        message: String,
    },
}

impl SourceOutcome {
    pub(crate) const SUBSCRIPTION_REJECTED: &'static str = "subscription_rejected";
    pub(crate) const EXTERNAL_SOURCE_REJECTED: &'static str = "external_source_rejected";

    pub(crate) fn rejected(code: &str, error: impl std::fmt::Display) -> Self {
        Self::Rejected {
            code: code.into(),
            message: error.to_string(),
        }
    }

    pub fn health(&self) -> ConvergenceHealth {
        match self {
            Self::Committed { .. } | Self::Superseded { .. } => ConvergenceHealth::Healthy,
            Self::Failed { .. } | Self::Rejected { .. } => ConvergenceHealth::Blocked,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct SourceStatus {
    pub profile: ProfileId,
    pub name: String,
    pub origin: SourceOrigin,
    pub outcome: SourceOutcome,
    pub health: ConvergenceHealth,
    /// Unix milliseconds.
    pub at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourcesSnapshot {
    pub event_seq: u64,
    pub entries: Vec<SourceStatus>,
}

/// The actor-owned ledger behind [`SourcesSnapshot`]. It only holds entries
/// for profiles that exist, so a deleted profile leaves no row behind.
pub(super) struct SourceLedger {
    entries: IndexMap<ProfileId, SourceStatus>,
    event_seq: u64,
    publish: watch::Sender<SourcesSnapshot>,
}

impl SourceLedger {
    pub(super) fn new(publish: watch::Sender<SourcesSnapshot>) -> Self {
        Self {
            entries: IndexMap::new(),
            event_seq: 0,
            publish,
        }
    }

    pub(super) fn record(
        &mut self,
        profiles: &Profiles,
        profile: &ProfileId,
        origin: SourceOrigin,
        outcome: SourceOutcome,
    ) {
        let Some(item) = profiles.items.get(profile) else {
            return;
        };
        self.entries.insert(
            profile.clone(),
            SourceStatus {
                profile: profile.clone(),
                name: item.metadata.name.clone(),
                origin,
                health: outcome.health(),
                outcome,
                at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64,
            },
        );
        self.publish();
    }

    /// Drops the entries of deleted profiles and follows renames.
    pub(super) fn reconcile(&mut self, profiles: &Profiles) {
        let mut changed = false;
        self.entries
            .retain(|uid, status| match profiles.items.get(uid) {
                None => {
                    changed = true;
                    false
                }
                Some(item) => {
                    if status.name != item.metadata.name {
                        status.name = item.metadata.name.clone();
                        changed = true;
                    }
                    true
                }
            });
        if changed {
            self.publish();
        }
    }

    fn publish(&mut self) {
        self.event_seq += 1;
        self.publish.send_replace(SourcesSnapshot {
            event_seq: self.event_seq,
            entries: self.entries.values().cloned().collect(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhance::golden_support::file_config;

    fn profiles(uids: &[&str]) -> Profiles {
        let mut profiles = Profiles::default();
        for uid in uids {
            profiles.append_item(file_config(uid, &format!("{uid}.yaml"), &[]));
        }
        profiles
    }

    fn failed() -> SourceOutcome {
        SourceOutcome::Failed {
            message: "download failed".into(),
        }
    }

    #[test]
    fn health_marks_failed_and_rejected_as_blocked() {
        assert_eq!(
            SourceOutcome::Committed { operation_id: None }.health(),
            ConvergenceHealth::Healthy
        );
        assert_eq!(
            SourceOutcome::Superseded {
                reason: "changed".into()
            }
            .health(),
            ConvergenceHealth::Healthy
        );
        assert_eq!(failed().health(), ConvergenceHealth::Blocked);
        assert_eq!(
            SourceOutcome::Rejected {
                code: SourceOutcome::SUBSCRIPTION_REJECTED.into(),
                message: "refused".into()
            }
            .health(),
            ConvergenceHealth::Blocked
        );
    }

    #[test]
    fn ledger_keeps_the_latest_result_per_profile_and_advances_its_sequence() {
        let (tx, rx) = watch::channel(SourcesSnapshot::default());
        let mut ledger = SourceLedger::new(tx);
        let profiles = profiles(&["r1", "r2"]);
        let r1 = ProfileId("r1".into());

        ledger.record(&profiles, &r1, SourceOrigin::ScheduledRefresh, failed());
        ledger.record(
            &profiles,
            &ProfileId("r2".into()),
            SourceOrigin::ManualRefresh,
            failed(),
        );
        ledger.record(
            &profiles,
            &r1,
            SourceOrigin::ManualRefresh,
            SourceOutcome::Committed {
                operation_id: Some("op".into()),
            },
        );

        let snapshot = rx.borrow().clone();
        assert_eq!(snapshot.event_seq, 3);
        assert_eq!(snapshot.entries.len(), 2);
        assert_eq!(snapshot.entries[0].profile, r1);
        assert_eq!(snapshot.entries[0].origin, SourceOrigin::ManualRefresh);
        assert_eq!(snapshot.entries[0].health, ConvergenceHealth::Healthy);
        assert_eq!(snapshot.entries[0].name, "r1");
    }

    #[test]
    fn ledger_ignores_unknown_profiles_and_prunes_deleted_ones() {
        let (tx, rx) = watch::channel(SourcesSnapshot::default());
        let mut ledger = SourceLedger::new(tx);
        let before = profiles(&["r1", "r2"]);

        ledger.record(
            &before,
            &ProfileId("ghost".into()),
            SourceOrigin::ExternalFile,
            failed(),
        );
        assert_eq!(rx.borrow().event_seq, 0, "a missing profile gets no row");

        ledger.record(
            &before,
            &ProfileId("r1".into()),
            SourceOrigin::ScheduledRefresh,
            failed(),
        );
        ledger.record(
            &before,
            &ProfileId("r2".into()),
            SourceOrigin::ScheduledRefresh,
            failed(),
        );
        ledger.reconcile(&before);
        assert_eq!(
            rx.borrow().event_seq,
            2,
            "an unchanged set is not republished"
        );

        let mut after = profiles(&["r2", "cfg"]);
        after
            .items
            .get_mut(&ProfileId("r2".into()))
            .unwrap()
            .metadata
            .name = "Renamed".into();
        ledger.reconcile(&after);
        let snapshot = rx.borrow().clone();
        assert_eq!(snapshot.event_seq, 3);
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].profile, ProfileId("r2".into()));
        assert_eq!(snapshot.entries[0].name, "Renamed");
    }
}
