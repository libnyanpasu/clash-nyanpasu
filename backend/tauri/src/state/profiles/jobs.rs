//! Job registrations derived from the profiles serial owner's committed slice.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use nyanpasu_config::profile::{ProfileId, ProfileSource, Profiles, RemoteProfileOptionsPatch};
use nyanpasu_jobs::{Job, JobDefinition, JobError, JobsClient, RunId, Schedule, Trigger};
use ractor::{ActorRef, rpc::CallResult};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::{CommitReport, ProfilesActorMessage, ProfilesError};

pub(crate) const SCOPE: &str = "profiles";
pub(crate) fn sync_key(uid: &ProfileId) -> String {
    format!("profiles/sync/{uid}")
}

type SyncReply = oneshot::Sender<Result<CommitReport, ProfilesError>>;

#[derive(Clone)]
pub(crate) struct ProfileJobs {
    pub client: JobsClient,
    // Only transports non-serializable RPC replies; domain state and execution
    // remain in ProfilesActor. The handler takes its sender before doing work.
    replies: Arc<Mutex<HashMap<RunId, SyncReply>>>,
}
struct SyncWaiter {
    id: RunId,
    replies: Arc<Mutex<HashMap<RunId, SyncReply>>>,
}
impl Drop for SyncWaiter {
    fn drop(&mut self) {
        self.replies.lock().unwrap().remove(&self.id);
    }
}

pub struct ProfileSyncContext(pub nyanpasu_jobs::JobContext);
impl std::fmt::Debug for ProfileSyncContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileSyncContext")
            .field("run_id", &self.0.run_id)
            .finish()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SyncInput {
    request: Option<RunId>,
    patch: Option<RemoteProfileOptionsPatch>,
}
#[derive(Serialize)]
struct SyncSummary {
    commit: crate::client::runtime::CommitReceipt,
    degradation_count: usize,
}

impl ProfileJobs {
    pub fn new(client: JobsClient) -> Self {
        Self {
            client,
            replies: Arc::default(),
        }
    }

    pub async fn sync(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> crate::client::Result<CommitReport> {
        let id = RunId::new_v4();
        let (send, receive) = oneshot::channel();
        self.replies.lock().unwrap().insert(id, send);
        let _waiter = SyncWaiter {
            id,
            replies: self.replies.clone(),
        };
        let input = SyncInput {
            request: Some(id),
            patch,
        };
        let run = match self.client.run_with(sync_key(&uid), &input).await {
            Ok(run) => run.id,
            // Admission uncertainty is resolved with the same identity; it is
            // never retried as a new download.
            Err(nyanpasu_jobs::Error::AdmissionUnknown(run)) => run,
            Err(error) => {
                self.replies.lock().unwrap().remove(&id);
                return Err(error.into());
            }
        };
        let completion = crate::client::jobs::wait(&self.client, run).await?;
        let mut journal_unavailable = completion.journal != nyanpasu_jobs::JournalState::Durable;
        if !journal_unavailable {
            // Durable result publication precedes final maintenance. Do not
            // return while the same key is still reserved by that owner.
            loop {
                match self.client.inspect().await {
                    Ok(view) if view.active.iter().any(|active| active.id == run) => {
                        tokio::task::yield_now().await
                    }
                    Ok(_) => break,
                    Err(_) => {
                        journal_unavailable = true;
                        break;
                    }
                }
            }
        }
        let mut report = receive.await.map_err(|_| {
            crate::client::ClientError::Custom("profile sync result unavailable".into())
        })??;
        if journal_unavailable {
            report
                .runtime_degradations
                .push(crate::client::runtime::Degradation {
                    phase: crate::client::runtime::DegradationPhase::SystemEffect,
                    reason: crate::client::runtime::DegradationReason::JobsJournalUnavailable,
                    message: "Profile committed, but the sync journal is unavailable".into(),
                    retryable: true,
                });
        }
        Ok(report)
    }

    pub async fn reconcile(
        &self,
        profiles: &Profiles,
        actor: &ActorRef<ProfilesActorMessage>,
        revision: u64,
        running: bool,
    ) -> Result<(), nyanpasu_jobs::Error> {
        let mut registrations = Vec::new();
        for (uid, item) in &profiles.items {
            let Some(ProfileSource::Remote { option, .. }) = item.definition.source() else {
                continue;
            };
            let mut definition = JobDefinition::manual(sync_key(uid));
            definition.scope = SCOPE.into();
            definition.name = "Profile sync".into();
            if running && option.update_interval_minutes > 0 {
                definition.schedule = Schedule::Interval {
                    every_ms: option.update_interval_minutes.saturating_mul(60_000),
                };
            }
            let (actor, uid, replies) = (actor.clone(), uid.clone(), self.replies.clone());
            registrations.push(Job::new(definition, SyncInput::default(), move |context, input: SyncInput| {
                let (actor, uid, replies) = (actor.clone(), uid.clone(), replies.clone());
                async move {
                    let reply = input.request.and_then(|id| replies.lock().unwrap().remove(&id));
                    tracing::info!(target: "nyanpasu::profile_sync", stage = "started", "Profile sync started");
                    let result = match actor.call(|reply| ProfilesActorMessage::SyncRemote { uid, patch: input.patch, context: ProfileSyncContext(context.clone()), reply }, None).await {
                        Ok(CallResult::Success(result)) => result,
                        _ => Err(ProfilesError::ProfilesActorStopped),
                    };
                    let summary = match &result {
                        Ok(report) => {
                            let count = report.degradations.len() + report.runtime_degradations.len();
                            tracing::info!(target: "nyanpasu::profile_sync", stage = "committed", "Profile sync committed");
                            if count > 0 {
                                tracing::warn!(target: "nyanpasu::profile_sync", stage = "degraded", "Profile committed with degraded effects");
                            }
                            Ok(SyncSummary { commit: report.receipt.clone(), degradation_count: count })
                        }
                        Err(error) => {
                            let error = sync_error(error);
                            tracing::warn!(target: "nyanpasu::profile_sync", stage = "failed", code = %error.code, "Profile sync failed");
                            Err(error)
                        }
                    };
                    if let Some(reply) = reply { let _ = reply.send(result); }
                    summary
                }
            })?);
        }
        self.client.reconcile(SCOPE, revision, registrations).await
    }

    pub async fn catch_up(&self, profiles: &Profiles) {
        for (uid, item) in &profiles.items {
            let Some(ProfileSource::Remote {
                option,
                materialized,
                ..
            }) = item.definition.source()
            else {
                continue;
            };
            let minutes = option.update_interval_minutes;
            if minutes == 0 {
                continue;
            }
            let overdue = materialized.updated_at.is_none_or(|at| {
                (time::OffsetDateTime::now_utc() - at).whole_seconds() as i128
                    >= i128::from(minutes) * 60
            });
            if overdue
                && let Err(error) = self
                    .client
                    .submit(
                        sync_key(uid),
                        RunId::new_v4(),
                        None,
                        Trigger::StartupCatchUp,
                    )
                    .await
            {
                tracing::warn!(%error, "failed to admit profile startup catch-up");
            }
        }
    }
}

fn sync_error(error: &ProfilesError) -> JobError {
    use nyanpasu_core::profiles::error::SubscriptionFetchError;
    match error {
        ProfilesError::FetchSubscription {
            source: SubscriptionFetchError::SubscriptionHttpStatus { status },
            ..
        } => JobError::new(
            "subscription_http_status",
            format!("Subscription server returned HTTP {status}"),
        ),
        ProfilesError::FetchSubscription { .. } => JobError::new(
            "subscription_download_failed",
            "Could not download the subscription",
        ),
        ProfilesError::ProfileContentRejected { .. } => JobError::new(
            "profile_content_rejected",
            "Downloaded profile failed validation",
        ),
        ProfilesError::ProfileChangedDuringRefresh { .. } => JobError::new(
            "profile_changed",
            "Profile changed before sync could commit",
        ),
        ProfilesError::ProfileDeletedDuringRefresh { .. } => JobError::new(
            "profile_deleted",
            "Profile was deleted before sync could commit",
        ),
        ProfilesError::ShuttingDown => JobError::new(
            "shutting_down",
            "Application stopped before sync could commit",
        ),
        _ => JobError::new("profile_sync_failed", "Profile sync did not commit"),
    }
}
