//! Composition of the shared job owner; business registrations stay with their domain.
use std::path::{Path, PathBuf};

use nyanpasu_jobs::{JobsClient, JobsService, Limits, LogCapture, LogPolicy, RedbJobStore};
use nyanpasu_utils::{
    io::IoFailure,
    redb_ext::{RedbFailure, find_failure},
};
use snafu::ResultExt as _;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::instrument::WithSubscriber;

pub(crate) const SYNC_LOG_TARGET: &str = "nyanpasu::profile_sync";
pub(crate) const DEFAULT_HISTORY_LIMIT: usize = 10;

pub fn capture() -> LogCapture {
    let mut policy = LogPolicy::default();
    policy.targets.insert(
        SYNC_LOG_TARGET.into(),
        ["message", "stage", "code"]
            .into_iter()
            .map(String::from)
            .collect(),
    );
    LogCapture::new(policy)
}

pub async fn start(
    path: PathBuf,
    capture: LogCapture,
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> Result<JobsClient, Error> {
    let mut service = match start_service(&path, capture.clone()).await {
        Err(error) if error.unusable_journal() => {
            let journal = path.clone();
            nyanpasu_owner_support::blocking::join(
                tokio::task::spawn_blocking(move || {
                    nyanpasu_persistence::preserve(&journal, &error)
                })
                .await,
            )
            .context(PreserveStoreSnafu)?;
            tracing::warn!(path = %path.display(), action = "reset_jobs_journal",
                "resetting the jobs journal discards run history; schedules are registered again");
            start_service(&path, capture).await?
        }
        result => result?,
    };
    let client = service.client();
    tasks.spawn(async move {
        shutdown.cancelled().await;
        // The owner must stay alive through finalization; an incomplete drain
        // continues on the same service instead of dropping active work.
        loop {
            match service.shutdown().await {
                Ok(report) if report.closed => break,
                Ok(_) => continue,
                Err(nyanpasu_jobs::Error::TimedOut) => {
                    tracing::warn!("jobs shutdown acknowledgement timed out; retaining the owner");
                }
                Err(error) => panic!("jobs owner stopped before its cleanup completed: {error}"),
            }
        }
    });
    Ok(client)
}

async fn start_service(path: &Path, capture: LogCapture) -> Result<JobsService, Error> {
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let store_path = path.to_owned();
    let store = nyanpasu_owner_support::blocking::join(
        tokio::task::spawn_blocking(move || open_store(&store_path)).await,
    )?;
    let mut limits = Limits::default();
    limits.retention.per_job = DEFAULT_HISTORY_LIMIT;
    // Profile sync coalesces elapsed interval slots after suspend; do not
    // discard an overdue sync.
    limits.lateness = std::time::Duration::MAX;
    JobsService::start(Box::new(store), capture, limits)
        .with_subscriber(dispatch)
        .await
        .context(StartOwnerSnafu)
}

fn open_store(path: &Path) -> Result<RedbJobStore, Error> {
    use crate::corrupt;
    match nyanpasu_persistence::retry_io(|| std::fs::symlink_metadata(path)) {
        Ok(metadata) if !metadata.is_file() || nyanpasu_persistence::is_link(&metadata) => {
            return NotAFileSnafu { path }.fail();
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(error).context(InspectStoreSnafu { path });
        }
        _ => {}
    }
    nyanpasu_persistence::retry(
        || RedbJobStore::open(path),
        |error| corrupt::error_io(error) == Some(IoFailure::Transient),
    )
    .context(OpenStoreSnafu { path })
}

pub(crate) async fn wait(
    client: &JobsClient,
    id: nyanpasu_jobs::RunId,
) -> Result<nyanpasu_jobs::Completion, nyanpasu_jobs::Error> {
    loop {
        match client.wait(id, None).await {
            Err(nyanpasu_jobs::Error::WaitTimedOut(_)) => continue,
            result => return result,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub async fn test_client_with_owner(
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> JobsClient {
    let directory = tempfile::tempdir().unwrap();
    let client = start(
        directory.path().join("jobs.redb"),
        capture(),
        shutdown.clone(),
        tasks,
    )
    .await
    .unwrap();
    tokio::spawn({
        let tasks = tasks.clone();
        async move {
            let _directory = directory;
            shutdown.cancelled().await;
            tasks.wait().await;
        }
    });
    client
}

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to inspect jobs at {}", path.display()))]
    InspectStore {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("jobs at {} is not a regular file", path.display()))]
    NotAFile { path: PathBuf },
    #[snafu(display("failed to open jobs at {}", path.display()))]
    OpenStore {
        path: PathBuf,
        source: nyanpasu_jobs::Error,
    },
    #[snafu(display("failed to start jobs owner"))]
    StartOwner { source: nyanpasu_jobs::Error },
    #[snafu(display("failed to preserve the unusable jobs journal"))]
    PreserveStore { source: nyanpasu_persistence::Error },
}

impl Error {
    /// The journal only holds run history, so a node that is not a regular
    /// file, or a file confirmed corrupt or written in an incompatible layout,
    /// is preserved and rebuilt. Permission, lock and other IO failures are
    /// reported instead.
    fn unusable_journal(&self) -> bool {
        let source = match self {
            Self::NotAFile { .. } => return true,
            Self::OpenStore { source, .. } | Self::StartOwner { source } => source,
            Self::InspectStore { .. } | Self::PreserveStore { .. } => return false,
        };
        matches!(
            source,
            nyanpasu_jobs::Error::JournalRecoveryDecode(_)
                | nyanpasu_jobs::Error::JournalPruneDecode(_)
        ) || matches!(
            find_failure(source),
            Some(RedbFailure::Corrupt | RedbFailure::Incompatible)
        )
    }
}

#[derive(Debug, serde::Serialize, specta::Type)]
pub struct ProfileSyncStatus {
    pub scheduled: bool,
    pub next_run_at: Option<String>,
    pub active: Vec<nyanpasu_jobs::dto::RunDto>,
    pub journal_degraded: bool,
    pub registration_error: Option<String>,
    pub history_limit: usize,
}

/// The secret-free view of [`ProfileSyncStatus`]. The registration error is owner text that can
/// carry the failing request's URL, so it is a flag.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct ProfileSyncSummary {
    pub scheduled: bool,
    pub next_run_at: Option<String>,
    pub active: Vec<SyncRunSummary>,
    pub journal_degraded: bool,
    pub has_registration_error: bool,
    pub history_limit: usize,
}

impl From<&ProfileSyncStatus> for ProfileSyncSummary {
    fn from(status: &ProfileSyncStatus) -> Self {
        let ProfileSyncStatus {
            scheduled,
            next_run_at,
            active,
            journal_degraded,
            registration_error,
            history_limit,
        } = status;
        Self {
            scheduled: *scheduled,
            next_run_at: next_run_at.clone(),
            active: active.iter().map(Into::into).collect(),
            journal_degraded: *journal_degraded,
            has_registration_error: registration_error.is_some(),
            history_limit: *history_limit,
        }
    }
}

/// One sync run without its job key, completion output, failure code and message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct SyncRunSummary {
    pub id: String,
    pub admission_sequence: String,
    pub trigger: String,
    pub scheduled_at: Option<String>,
    pub admitted_at: String,
    pub finished_at: Option<String>,
    pub state: SyncRunStateSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncRunStateSummary {
    Admitted,
    Running,
    Cancelling,
    Finalizing,
    Finished {
        outcome: SyncOutcomeKind,
        journal_degraded: bool,
    },
}

/// A failed run is only `Failed`: its code and message are owner text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncOutcomeKind {
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    Skipped,
}

impl From<&nyanpasu_jobs::dto::RunDto> for SyncRunSummary {
    fn from(run: &nyanpasu_jobs::dto::RunDto) -> Self {
        use nyanpasu_jobs::dto::{JournalDto, OutcomeDto, RunDto, RunStateDto};

        let RunDto {
            id,
            // The job key embeds the profile uid; the caller already knows it.
            job: _,
            definition_version: _,
            admission_sequence,
            // A fixed identifier of the scheduler, not free text.
            trigger,
            scheduled_at,
            admitted_at,
            finished_at,
            state,
            last_log_sequence: _,
            dropped_log_count: _,
        } = run;
        let state = match state {
            RunStateDto::Admitted => SyncRunStateSummary::Admitted,
            RunStateDto::Running => SyncRunStateSummary::Running,
            RunStateDto::Cancelling => SyncRunStateSummary::Cancelling,
            RunStateDto::Finalizing => SyncRunStateSummary::Finalizing,
            RunStateDto::Finished { completion } => SyncRunStateSummary::Finished {
                outcome: match completion.outcome {
                    OutcomeDto::Succeeded => SyncOutcomeKind::Succeeded,
                    OutcomeDto::Failed { .. } => SyncOutcomeKind::Failed,
                    OutcomeDto::Cancelled => SyncOutcomeKind::Cancelled,
                    OutcomeDto::Interrupted => SyncOutcomeKind::Interrupted,
                    OutcomeDto::Skipped => SyncOutcomeKind::Skipped,
                },
                journal_degraded: matches!(completion.journal, JournalDto::Degraded { .. }),
            },
        };
        Self {
            id: id.clone(),
            admission_sequence: admission_sequence.clone(),
            trigger: trigger.clone(),
            scheduled_at: scheduled_at.clone(),
            admitted_at: admitted_at.clone(),
            finished_at: finished_at.clone(),
            state,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SyncRunPageSummary {
    pub items: Vec<SyncRunSummary>,
    pub next: Option<nyanpasu_jobs::dto::RunCursorDto>,
}

impl From<&nyanpasu_jobs::dto::RunPageDto> for SyncRunPageSummary {
    fn from(page: &nyanpasu_jobs::dto::RunPageDto) -> Self {
        let nyanpasu_jobs::dto::RunPageDto { items, next } = page;
        Self {
            items: items.iter().map(Into::into).collect(),
            next: next.clone(),
        }
    }
}

impl super::NyanpasuClient {
    fn profile_sync_target(
        &self,
        uid: &nyanpasu_config::profile::ProfileId,
    ) -> Result<String, super::ProfileSyncReadError> {
        let snapshot = self.inner.profiles.snapshot();
        let item = snapshot
            .items
            .get(uid)
            .ok_or(super::ProfileSyncReadError::UnknownProfile)?;
        if !matches!(
            item.definition.source(),
            Some(nyanpasu_config::profile::ProfileSource::Remote { .. })
        ) {
            return Err(super::ProfileSyncReadError::NotRemote);
        }
        Ok(crate::state::profiles::jobs::sync_key(uid))
    }

    fn profile_sync_key(&self, uid: &nyanpasu_config::profile::ProfileId) -> super::Result<String> {
        self.profile_sync_target(uid).map_err(|error| match error {
            super::ProfileSyncReadError::NotRemote => {
                nyanpasu_runtime_values::profiles::NotARemoteProfileSnafu { uid: uid.clone() }
                    .build()
                    .into()
            }
            _ => super::ClientError::Profiles(
                nyanpasu_runtime_values::profiles::ProfileNotFoundSnafu { uid: uid.clone() }
                    .build(),
            ),
        })
    }

    async fn profile_sync_status_of(
        &self,
        key: String,
    ) -> Result<ProfileSyncStatus, nyanpasu_jobs::Error> {
        let view = self.inner.jobs.inspect().await?;
        let definition = view.jobs.iter().find(|job| job.definition.key == key);
        Ok(ProfileSyncStatus {
            scheduled: definition.is_some_and(|job| {
                !matches!(job.definition.schedule, nyanpasu_jobs::Schedule::Manual)
            }),
            next_run_at: definition.and_then(|job| job.next_run_at.map(|time| time.to_string())),
            active: view
                .active
                .into_iter()
                .filter(|run| run.job == key)
                .map(Into::into)
                .collect(),
            journal_degraded: view.journal_degraded,
            registration_error: view
                .scopes
                .get(crate::state::profiles::jobs::SCOPE)
                .and_then(|scope| scope.last_error.clone()),
            history_limit: DEFAULT_HISTORY_LIMIT,
        })
    }

    pub async fn profile_sync_status(
        &self,
        uid: nyanpasu_config::profile::ProfileId,
    ) -> super::Result<ProfileSyncStatus> {
        let key = self.profile_sync_key(&uid)?;
        Ok(self.profile_sync_status_of(key).await?)
    }

    pub async fn profile_sync_runs(
        &self,
        uid: nyanpasu_config::profile::ProfileId,
        after: Option<nyanpasu_jobs::dto::RunCursorDto>,
    ) -> super::Result<nyanpasu_jobs::dto::RunPageDto> {
        let key = self.profile_sync_key(&uid)?;
        Ok(self
            .inner
            .jobs
            .runs(key, after.map(TryInto::try_into).transpose()?, 10)
            .await?
            .into())
    }

    pub async fn profile_sync_status_summary(
        &self,
        uid: nyanpasu_config::profile::ProfileId,
    ) -> Result<ProfileSyncSummary, super::ProfileSyncReadError> {
        let key = self.profile_sync_target(&uid)?;
        let status = self
            .profile_sync_status_of(key)
            .await
            .map_err(|_| super::ProfileSyncReadError::Unavailable)?;
        Ok((&status).into())
    }

    pub async fn profile_sync_runs_summary(
        &self,
        uid: nyanpasu_config::profile::ProfileId,
        after: Option<nyanpasu_jobs::dto::RunCursorDto>,
    ) -> Result<SyncRunPageSummary, super::ProfileSyncReadError> {
        let key = self.profile_sync_target(&uid)?;
        let after = after
            .map(TryInto::try_into)
            .transpose()
            .map_err(|_: nyanpasu_jobs::Error| super::ProfileSyncReadError::InvalidCursor)?;
        let page: nyanpasu_jobs::dto::RunPageDto = self
            .inner
            .jobs
            .runs(key, after, 10)
            .await
            .map_err(|_| super::ProfileSyncReadError::Unavailable)?
            .into();
        Ok((&page).into())
    }

    pub async fn profile_sync_logs(
        &self,
        uid: nyanpasu_config::profile::ProfileId,
        run: String,
        after: Option<String>,
    ) -> super::Result<nyanpasu_jobs::dto::LogPageDto> {
        let key = self.profile_sync_key(&uid)?;
        let id = run
            .parse()
            .map_err(|_| nyanpasu_jobs::Error::Invalid("invalid run ID".into()))?;
        let client = self.inner.jobs.clone();
        if client.get_run(id).await?.job != key {
            return Err(nyanpasu_jobs::Error::NotFound.into());
        }
        let after = after
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_| nyanpasu_jobs::Error::Invalid("invalid log cursor".into()))?
            .unwrap_or(0);
        Ok(client.logs(id, after, 100).await?.into())
    }
}

#[cfg(test)]
pub(crate) struct ExplicitTestTime(tokio::task::JoinHandle<()>);
#[cfg(test)]
impl Drop for ExplicitTestTime {
    fn drop(&mut self) {
        self.0.abort();
    }
}
#[cfg(test)]
pub(crate) fn explicit_test_time() -> ExplicitTestTime {
    // Blocking journal IO must not make Tokio automatically advance a paused
    // clock while an acknowledgement is still coming from an OS thread.
    ExplicitTestTime(tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    }))
}

#[cfg(test)]
pub(crate) async fn settled(client: &JobsClient) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !client.inspect().await.unwrap().active.is_empty() {
        assert!(std::time::Instant::now() < deadline, "jobs did not settle");
        tokio::task::yield_now().await;
    }
}

#[cfg(test)]
mod summary_tests {
    /// The read error is a fixed enum: nothing a journal or adapter reports can reach its text.
    #[test]
    fn the_sync_read_error_is_fixed_text() {
        use crate::client::ProfileSyncReadError as Reason;
        for (reason, tag, text) in [
            (
                Reason::UnknownProfile,
                "unknown_profile",
                "the profile does not exist",
            ),
            (
                Reason::NotRemote,
                "not_remote",
                "the profile has no remote source to sync",
            ),
            (
                Reason::InvalidCursor,
                "invalid_cursor",
                "the run history cursor is invalid",
            ),
            (
                Reason::Unavailable,
                "unavailable",
                "the sync status is unavailable",
            ),
        ] {
            assert_eq!(reason.to_string(), text);
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                serde_json::json!({"kind": tag})
            );
        }
    }

    use nyanpasu_jobs::dto::{
        CompletionDto, JournalDto, OutcomeDto, OutputDto, RunCursorDto, RunDto, RunPageDto,
        RunStateDto,
    };

    use super::*;

    const URL_SECRET: &str = "https://alice:hunter2@sub-7f3a91c2.example-panel.invalid/tok_7f3a91c2d4e85b06/sub?token=query-secret-31337";

    fn run(state: RunStateDto) -> RunDto {
        RunDto {
            id: "7".into(),
            job: "profile-sync:job-key-sentinel".into(),
            definition_version: "3".into(),
            admission_sequence: "9".into(),
            trigger: "interval".into(),
            scheduled_at: Some("2026-11-13T00:00:00Z".into()),
            admitted_at: "2026-11-13T00:00:01Z".into(),
            finished_at: Some("2026-11-13T00:00:02Z".into()),
            state,
            last_log_sequence: "12".into(),
            dropped_log_count: "0".into(),
        }
    }

    fn finished(outcome: OutcomeDto, journal: JournalDto) -> RunDto {
        run(RunStateDto::Finished {
            completion: CompletionDto {
                outcome,
                output: OutputDto::JsonText {
                    value: format!("{{\"url\":\"{URL_SECRET}\"}}"),
                },
                journal,
            },
        })
    }

    #[test]
    fn a_failed_run_keeps_its_kind_and_loses_the_code_message_and_output() {
        let failed = finished(
            OutcomeDto::Failed {
                code: "code-sentinel".into(),
                message: format!("GET {URL_SECRET} failed"),
            },
            JournalDto::Degraded {
                code: "journal-code-sentinel".into(),
            },
        );
        let summary = SyncRunSummary::from(&failed);
        assert_eq!(
            summary.state,
            SyncRunStateSummary::Finished {
                outcome: SyncOutcomeKind::Failed,
                journal_degraded: true,
            }
        );
        let wire = serde_json::to_string(&summary).unwrap();
        for sentinel in [
            "alice",
            "hunter2",
            "tok_7f3a91c2d4e85b06",
            "sub-7f3a91c2",
            "query-secret-31337",
            "code-sentinel",
            "job-key-sentinel",
        ] {
            assert!(!wire.contains(sentinel), "{sentinel} leaked: {wire}");
        }
        assert_eq!(
            serde_json::to_value(&summary).unwrap(),
            serde_json::json!({
                "id": "7",
                "admission_sequence": "9",
                "trigger": "interval",
                "scheduled_at": "2026-11-13T00:00:00Z",
                "admitted_at": "2026-11-13T00:00:01Z",
                "finished_at": "2026-11-13T00:00:02Z",
                "state": {
                    "kind": "finished",
                    "outcome": "failed",
                    "journal_degraded": true
                }
            })
        );
    }

    #[test]
    fn every_outcome_and_live_state_maps_to_its_own_kind() {
        let kinds = [
            (OutcomeDto::Succeeded, SyncOutcomeKind::Succeeded),
            (OutcomeDto::Cancelled, SyncOutcomeKind::Cancelled),
            (OutcomeDto::Interrupted, SyncOutcomeKind::Interrupted),
            (OutcomeDto::Skipped, SyncOutcomeKind::Skipped),
        ];
        for (outcome, expected) in kinds {
            assert_eq!(
                SyncRunSummary::from(&finished(outcome, JournalDto::Durable)).state,
                SyncRunStateSummary::Finished {
                    outcome: expected,
                    journal_degraded: false,
                }
            );
        }
        for (state, expected) in [
            (RunStateDto::Admitted, SyncRunStateSummary::Admitted),
            (RunStateDto::Running, SyncRunStateSummary::Running),
            (RunStateDto::Cancelling, SyncRunStateSummary::Cancelling),
            (RunStateDto::Finalizing, SyncRunStateSummary::Finalizing),
        ] {
            assert_eq!(SyncRunSummary::from(&run(state)).state, expected);
        }
    }

    #[test]
    fn the_status_summary_flags_the_registration_error_and_keeps_the_counts() {
        let status = ProfileSyncStatus {
            scheduled: true,
            next_run_at: Some("2026-11-13T01:00:00Z".into()),
            active: vec![run(RunStateDto::Running)],
            journal_degraded: true,
            registration_error: Some(format!("cannot register {URL_SECRET}")),
            history_limit: 10,
        };
        let summary = ProfileSyncSummary::from(&status);
        assert!(summary.has_registration_error);
        assert!(summary.scheduled && summary.journal_degraded);
        assert_eq!(summary.active.len(), 1);
        assert_eq!(summary.history_limit, 10);
        let wire = serde_json::to_string(&summary).unwrap();
        assert!(
            !wire.contains("hunter2") && !wire.contains("cannot register"),
            "{wire}"
        );
        assert!(!wire.contains("registration_error\":\""), "{wire}");

        let healthy = ProfileSyncStatus {
            registration_error: None,
            ..status
        };
        assert!(!ProfileSyncSummary::from(&healthy).has_registration_error);
    }

    #[test]
    fn the_page_summary_keeps_the_cursor() {
        let page = RunPageDto {
            items: vec![run(RunStateDto::Admitted)],
            next: Some(RunCursorDto {
                sequence: "5".into(),
                id: "4".into(),
            }),
        };
        let summary = SyncRunPageSummary::from(&page);
        assert_eq!(summary.items.len(), 1);
        let next = summary.next.expect("cursor kept");
        assert_eq!((next.sequence.as_str(), next.id.as_str()), ("5", "4"));
    }
}
