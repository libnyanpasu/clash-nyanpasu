//! The typed client the facade and RPC commands call.
//!
//! It hides the actor behind ordinary async methods: callers never see a
//! `ractor::ActorRef`, and an in-process call waits for the real result.

use std::sync::Arc;

use nyanpasu_contracts::sync::{ProviderKind, ProviderSecrets, RemoteStoreFactory, SyncSettings};
use nyanpasu_runtime_values::sync::{ConflictChoice, SyncConflict, SyncStatus};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    actor::{Args, Message, SettingsReply, SyncActor, SyncFailure},
    engine::CycleReport,
    ports::SyncPorts,
};

#[derive(Debug, snafu::Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    #[snafu(display("failed to start the sync owner"))]
    Spawn { source: ractor::SpawnErr },
    #[snafu(display("the sync owner stopped before answering"))]
    Stopped,
}

#[derive(Clone)]
pub struct SyncClient {
    actor: ActorRef<Message>,
}

impl SyncClient {
    /// Starts the owner. `adapters` is `None` when the shell supplied no sync
    /// storage, which leaves every command answering `unsupported`.
    pub(crate) async fn spawn(
        ports: SyncPorts,
        settings: SyncSettings,
        adapters: Option<Arc<dyn RemoteStoreFactory>>,
        settings_tx: tokio::sync::watch::Sender<SyncSettings>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self, Error> {
        let token = shutdown.clone();
        let (actor, _handle) = Actor::spawn(
            None,
            SyncActor,
            Args {
                ports,
                settings,
                settings_tx,
                adapters,
                shutdown,
            },
        )
        .await
        .map_err(|source| Error::Spawn { source })?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, token, actor.get_cell());
        Ok(Self { actor })
    }

    pub async fn status(&self) -> Result<SyncStatus, Error> {
        match self.actor.call(Message::Status, None).await {
            Ok(CallResult::Success(status)) => Ok(status),
            Ok(CallResult::SenderError) | Err(_) => Err(Error::Stopped),
            Ok(CallResult::Timeout) => unreachable!("sync calls are made without a timeout"),
        }
    }

    pub async fn settings(&self) -> Result<SettingsReply, Error> {
        match self.actor.call(Message::Settings, None).await {
            Ok(CallResult::Success(settings)) => Ok(settings),
            Ok(CallResult::SenderError) | Err(_) => Err(Error::Stopped),
            Ok(CallResult::Timeout) => unreachable!("sync calls are made without a timeout"),
        }
    }

    pub async fn conflicts(&self) -> Result<Vec<SyncConflict>, Error> {
        match self.actor.call(Message::Conflicts, None).await {
            Ok(CallResult::Success(conflicts)) => Ok(conflicts),
            Ok(CallResult::SenderError) | Err(_) => Err(Error::Stopped),
            Ok(CallResult::Timeout) => unreachable!("sync calls are made without a timeout"),
        }
    }

    pub async fn configure(
        &self,
        settings: SyncSettings,
        secrets: Option<ProviderSecrets>,
    ) -> Result<(), SyncFailure> {
        self.call(|reply| Message::Configure {
            settings,
            secrets,
            reply,
        })
        .await
    }

    pub async fn enable(&self) -> Result<CycleReport, SyncFailure> {
        self.call(Message::Enable).await
    }

    pub async fn disable(&self) -> Result<(), SyncFailure> {
        self.call(Message::Disable).await
    }

    /// An explicit sync request; retries candidates the core refused.
    pub async fn sync_now(&self) -> Result<CycleReport, SyncFailure> {
        self.call(Message::SyncNow).await
    }

    /// A scheduled or change-triggered run; does not retry refusals.
    pub async fn sync_scheduled(&self) -> Result<CycleReport, SyncFailure> {
        self.call(Message::SyncScheduled).await
    }

    pub async fn test_connection(
        &self,
        settings: SyncSettings,
        secrets: Option<ProviderSecrets>,
    ) -> Result<(), SyncFailure> {
        self.call(|reply| Message::TestConnection {
            settings,
            secrets,
            reply,
        })
        .await
    }

    pub async fn resolve_conflict(
        &self,
        conflict_id: String,
        choice: ConflictChoice,
    ) -> Result<CycleReport, SyncFailure> {
        self.call(|reply| Message::ResolveConflict {
            conflict_id,
            choice,
            reply,
        })
        .await
    }

    pub async fn restore_snapshot(&self, snapshot_id: String) -> Result<(), SyncFailure> {
        self.call(|reply| Message::RestoreSnapshot { snapshot_id, reply })
            .await
    }

    /// Every fallible command answers `Result<T, SyncFailure>` through its own
    /// reply port, so one helper reads the outer call result and returns the
    /// inner domain result unchanged.
    async fn call<T, F>(&self, make: F) -> Result<T, SyncFailure>
    where
        T: Send + 'static,
        F: FnOnce(RpcReplyPort<Result<T, SyncFailure>>) -> Message,
    {
        match self.actor.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) | Err(_) => Err(SyncFailure::Failed {
                reason: "the sync owner stopped before answering".to_owned(),
            }),
            Ok(CallResult::Timeout) => unreachable!("sync calls are made without a timeout"),
        }
    }
}

impl Drop for SyncClient {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

/// The provider kinds this build can use, from the injected factory.
#[must_use]
pub fn provider_kinds(adapters: Option<&Arc<dyn RemoteStoreFactory>>) -> Vec<ProviderKind> {
    adapters
        .map(|adapters| adapters.kinds())
        .unwrap_or_default()
}
