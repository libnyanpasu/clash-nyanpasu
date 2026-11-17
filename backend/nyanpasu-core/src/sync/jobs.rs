//! The scheduled sync cycle, registered as a job.
//!
//! A cycle appears in the existing jobs UI with its run history and logs, and
//! the jobs subsystem's no-overlap rule turns a scheduled cycle that meets a
//! manual one into a skip instead of two concurrent cycles.

use std::sync::Arc;

use nyanpasu_contracts::sync::SyncSettings;
use nyanpasu_jobs::{Job, JobDefinition, JobError, JobsClient, Schedule};

use super::client::SyncClient;

/// The one job every device registers.
pub const JOB_KEY: &str = "sync.config";
const SCOPE: &str = "sync";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct CycleInput {}

#[derive(serde::Serialize)]
struct CycleSummary {
    outcome: Option<nyanpasu_runtime_values::sync::SyncOutcome>,
    pushed: u32,
    pulled: u32,
    merged: u32,
}

/// Registers the interval cycle from the committed settings.
#[derive(Clone)]
pub struct SyncJobs {
    client: JobsClient,
}

impl SyncJobs {
    #[must_use]
    pub fn new(client: JobsClient) -> Self {
        Self { client }
    }

    /// Replaces the registered job with one whose schedule follows `settings`.
    ///
    /// `interval_minutes == 0` registers a manual job, as for remote profiles.
    ///
    /// # Errors
    ///
    /// [`nyanpasu_jobs::Error`] when the registration is refused.
    pub async fn reconcile(
        &self,
        settings: &SyncSettings,
        sync: SyncClient,
    ) -> Result<(), nyanpasu_jobs::Error> {
        let mut definition = JobDefinition::manual(JOB_KEY);
        definition.scope = SCOPE.into();
        definition.name = "Configuration sync".into();
        if settings.enabled && settings.interval_minutes > 0 {
            definition.schedule = Schedule::Interval {
                every_ms: settings.interval_minutes.saturating_mul(60_000),
            };
        }
        let sync_client: Arc<SyncClient> = Arc::new(sync);
        let job = Job::new(
            definition,
            CycleInput::default(),
            move |_context, _input| {
                let sync_client = Arc::clone(&sync_client);
                async move {
                    match sync_client.sync_now().await {
                        Ok(report) => Ok(CycleSummary {
                            outcome: report.outcome,
                            pushed: report.pushed,
                            pulled: report.pulled,
                            merged: report.merged,
                        }),
                        Err(failure) => Err(JobError::new("sync_failed", failure.to_string())),
                    }
                }
            },
        )?;
        self.client.reconcile(SCOPE, 1, vec![job]).await
    }

    /// Runs the registered cycle now, as a manual trigger through the jobs UI.
    ///
    /// # Errors
    ///
    /// [`nyanpasu_jobs::Error`] when the job is not registered.
    pub async fn trigger(&self) -> Result<(), nyanpasu_jobs::Error> {
        self.client.run_now(JOB_KEY).await.map(drop)
    }
}
