//! Source-owned boundaries for independent checks and committed-state notifications.

use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};
use nyanpasu_runtime_values::{
    candidate::CandidateSnapshotId, configuration::BackgroundChangeOrigin,
};

/// A candidate is checked without changing the running core or publishing source state.
pub(crate) struct RuntimeCandidate {
    pub change: super::DomainChange,
    pub staged: std::collections::BTreeMap<String, String>,
    /// The profile a refresh or a watched file is updating: such a source can
    /// fail the same way on every round, so a candidate it keeps replaces
    /// the one it kept before.
    pub background: Option<nyanpasu_config::profile::ProfileId>,
}

/// Who asked a source owner for a change. A caller that does not wait on
/// the answer names its origin, so the owner reports the outcome under it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Attribution {
    /// The origin a failure of this change is reported under; `None` for
    /// a caller that receives the error itself.
    pub refused: Option<BackgroundChangeOrigin>,
    /// The origin whose failures this change's commit concludes, besides
    /// the settings the commit changes.
    pub concludes: Option<BackgroundChangeOrigin>,
}

impl Attribution {
    /// A change from `origin`, whose commit concludes that origin's
    /// earlier failures.
    pub(crate) fn from(origin: BackgroundChangeOrigin) -> Self {
        Self {
            refused: Some(origin),
            concludes: Some(origin),
        }
    }
}

/// The setting a tray item or a shortcut changes, as the owner of that
/// setting compares a commit's states to tell whether the commit changed
/// it, and so concluded the setting's earlier refusals.
#[derive(Clone, Copy)]
pub(crate) enum TraySetting {
    Application(fn(&NyanpasuAppConfig, &NyanpasuAppConfig) -> bool),
    Clash(fn(&ClashConfig, &ClashConfig) -> bool),
}

impl TraySetting {
    /// Every origin a tray item or a shortcut reports under. The hotkey
    /// tests hold each action's origin to this list.
    pub(crate) const ORIGINS: [BackgroundChangeOrigin; 4] = [
        BackgroundChangeOrigin::ActionClashMode,
        BackgroundChangeOrigin::ActionSystemProxy,
        BackgroundChangeOrigin::ActionTunMode,
        BackgroundChangeOrigin::ActionTransparentProxy,
    ];

    /// The setting `origin` refers to; a new origin must say which commit
    /// clears its row.
    pub(crate) fn of(origin: BackgroundChangeOrigin) -> Option<Self> {
        match origin {
            BackgroundChangeOrigin::SyncApplication
            | BackgroundChangeOrigin::SyncClash
            | BackgroundChangeOrigin::SyncProfiles => None,
            BackgroundChangeOrigin::ActionClashMode => Some(Self::Clash(|before, after| {
                before.overrides.mode() != after.overrides.mode()
            })),
            BackgroundChangeOrigin::ActionSystemProxy => {
                Some(Self::Application(|before, after| {
                    before.enable_system_proxy != after.enable_system_proxy
                }))
            }
            BackgroundChangeOrigin::ActionTunMode => Some(Self::Clash(|before, after| {
                before.enable_tun_mode != after.enable_tun_mode
            })),
            BackgroundChangeOrigin::ActionTransparentProxy => Some(Self::Clash(|before, after| {
                before.transparent_proxy.mode != after.transparent_proxy.mode
            })),
        }
    }

    /// The tray settings an application commit changed.
    pub(crate) fn changed_application(
        before: &NyanpasuAppConfig,
        after: &NyanpasuAppConfig,
    ) -> impl Iterator<Item = BackgroundChangeOrigin> {
        Self::ORIGINS.into_iter().filter(|origin| {
            matches!(Self::of(*origin), Some(Self::Application(changed)) if changed(before, after))
        })
    }

    /// The tray settings a Clash commit changed.
    pub(crate) fn changed_clash(
        before: &ClashConfig,
        after: &ClashConfig,
    ) -> impl Iterator<Item = BackgroundChangeOrigin> {
        Self::ORIGINS.into_iter().filter(|origin| {
            matches!(Self::of(*origin), Some(Self::Clash(changed)) if changed(before, after))
        })
    }
}

/// Where source owners report the changes they decide, as they decide them.
pub(crate) trait OutcomeRecord: Send + Sync + 'static {
    /// The commit at `version` concluded `origins`.
    fn committed(&self, origins: &[BackgroundChangeOrigin], version: u64);
    /// A change from `origin` failed after `version`, as its owner's
    /// `refusal`-th failure, and kept `candidate`.
    fn refused(
        &self,
        origin: BackgroundChangeOrigin,
        version: u64,
        refusal: u64,
        message: String,
        candidate: CandidateSnapshotId,
    );
}

/// An owner's side of [`OutcomeRecord`]: it numbers the failures it
/// reports, so they stay in the order it decided them.
#[derive(Default)]
pub(crate) struct OwnerOutcomes {
    refusals: u64,
}

impl OwnerOutcomes {
    /// The commit at `version` changed `settings`.
    pub(crate) fn committed(
        &self,
        record: &dyn OutcomeRecord,
        attribution: Attribution,
        settings: impl IntoIterator<Item = BackgroundChangeOrigin>,
        version: nyanpasu_state_engine::Version,
    ) {
        let origins: Vec<_> = settings.into_iter().chain(attribution.concludes).collect();
        if !origins.is_empty() {
            record.committed(&origins, *version.as_ref());
        }
    }

    /// The change failed while the owner stood at `version`; only a failure
    /// a caller does not wait on and that kept a candidate is reported.
    pub(crate) fn failed(
        &mut self,
        record: &dyn OutcomeRecord,
        attribution: Attribution,
        version: nyanpasu_state_engine::Version,
        error: &(dyn std::error::Error + 'static),
        candidate: Option<&CandidateSnapshotId>,
    ) {
        let (Some(origin), Some(candidate)) = (attribution.refused, candidate) else {
            return;
        };
        self.refusals += 1;
        record.refused(
            origin,
            *version.as_ref(),
            self.refusals,
            snafu::Report::from_error(error).to_string(),
            candidate.clone(),
        );
    }
}

/// Reports nothing, for owners whose outcomes no one records.
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct NoOutcomes;

#[cfg(any(test, feature = "test-support"))]
impl OutcomeRecord for NoOutcomes {
    fn committed(&self, _: &[BackgroundChangeOrigin], _: u64) {}
    fn refused(
        &self,
        _: BackgroundChangeOrigin,
        _: u64,
        _: u64,
        _: String,
        _: CandidateSnapshotId,
    ) {
    }
}

/// Source-owned boundary for independent checks and committed-state notifications.
#[async_trait::async_trait]
pub(crate) trait RuntimeGate: Send + Sync + 'static {
    async fn check(
        &self,
        candidate: RuntimeCandidate,
    ) -> Result<CheckPass, std::sync::Arc<nyanpasu_runtime_values::runtime_error::RuntimeError>>;
    fn committed(
        &self,
        domain: nyanpasu_runtime_values::mutation::ConfigDomain,
        version: nyanpasu_state_engine::Version,
    );
}

/// What a passing check hands its source: the checked candidate, kept for
/// inspection only when the source then fails to commit the change. It
/// lives as long as that commit.
#[must_use]
#[derive(Default)]
pub(crate) struct CheckPass {
    checked: Option<Box<dyn KeepChecked>>,
}

/// Keeps the candidate a check passed; the check that built it implements it.
#[async_trait::async_trait]
pub(crate) trait KeepChecked: Send + Sync + 'static {
    async fn keep(
        self: Box<Self>,
    ) -> Option<nyanpasu_runtime_values::candidate::CandidateSnapshotId>;
}

/// A source's failure to commit a change, which can name the checked
/// candidate.
pub(crate) trait CommitFailure: Sized {
    fn carries_candidate_snapshot_id(&self) -> bool;
    fn with_candidate_snapshot_id(
        self,
        id: nyanpasu_runtime_values::candidate::CandidateSnapshotId,
    ) -> Self;
}

impl CheckPass {
    pub(crate) fn new(checked: impl KeepChecked) -> Self {
        Self {
            checked: Some(Box::new(checked)),
        }
    }

    /// Keeps the checked candidate, for a committed change whose completion
    /// was deferred.
    pub(crate) async fn keep(
        self,
    ) -> Option<nyanpasu_runtime_values::candidate::CandidateSnapshotId> {
        self.checked?.keep().await
    }

    /// `error`, naming the checked candidate when it is a failure to commit
    /// the change: a version conflict, a write or a materialization failure.
    pub(crate) async fn keep_for<E: CommitFailure>(self, error: E) -> E {
        let Some(checked) = self
            .checked
            .filter(|_| error.carries_candidate_snapshot_id())
        else {
            return error;
        };
        match checked.keep().await {
            Some(id) => error.with_candidate_snapshot_id(id),
            None => error,
        }
    }
}

impl CommitFailure for crate::state::config_error::ConfigError {
    fn carries_candidate_snapshot_id(&self) -> bool {
        self.carries_candidate_snapshot_id()
    }
    fn with_candidate_snapshot_id(
        self,
        id: nyanpasu_runtime_values::candidate::CandidateSnapshotId,
    ) -> Self {
        self.with_candidate_snapshot_id(id)
    }
}

impl CommitFailure for nyanpasu_runtime_values::profiles::ProfilesError {
    fn carries_candidate_snapshot_id(&self) -> bool {
        self.carries_candidate_snapshot_id()
    }
    fn with_candidate_snapshot_id(
        self,
        id: nyanpasu_runtime_values::candidate::CandidateSnapshotId,
    ) -> Self {
        self.with_candidate_snapshot_id(id)
    }
}
