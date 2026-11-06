//! Global shortcuts, owned by one actor behind a typed client.
//!
//! Callers never see a `ractor::ActorRef`: they hand a revision and the desired
//! bindings in and get one [`EffectStatus`] back. The actor never calls the
//! facade either: a pressed shortcut travels out through a channel and back in
//! through `NyanpasuClient::dispatch_hotkey_action`, so the two never form a
//! cycle. Every accelerator is handed back to the OS on application shutdown.

mod actor;
pub(crate) mod error;
pub mod ports;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use nyanpasu_core::client::{
    effects::{
        plan::EffectKind,
        status::{EffectHealth, EffectRevision, EffectStatus},
    },
    hotkey::ports::{HotkeyAction, HotkeyBindings},
};
use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio_util::task::TaskTracker;

use self::{
    actor::{HotkeyActor, Message},
    error::HotkeyEffectError,
};

pub use self::actor::Args as HotkeyArgs;

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}

/// The actor's private state, observable for tests and diagnostics. The effect
/// protocol itself travels as [`EffectStatus`].
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct HotkeyStatus {
    pub applied_revision: EffectRevision,
    pub health: EffectHealth,
    /// Accelerators the OS confirmed, and what each one runs.
    pub registered: BTreeMap<String, HotkeyAction>,
}

#[derive(Clone)]
pub struct HotkeyClient {
    actor: ActorRef<Message>,
}

impl HotkeyClient {
    /// The actor hands every accelerator back to the OS once `args.shutdown`
    /// is cancelled, and `tasks` waits for that.
    pub async fn spawn(args: HotkeyArgs, tasks: &TaskTracker) -> std::result::Result<Self, Error> {
        let shutdown = args.shutdown.clone();
        let (actor, _handle) = Actor::spawn(None, HotkeyActor, args)
            .await
            .map_err(|source| Error::SpawnActor { source })?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self { actor })
    }

    pub async fn reconcile(
        &self,
        revision: EffectRevision,
        desired: HotkeyBindings,
    ) -> EffectStatus {
        match self
            .actor
            .call(
                |reply| Message::Reconcile {
                    revision,
                    desired,
                    reply,
                },
                None,
            )
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!(
                    revision = revision.get(),
                    "the hotkey actor stopped before answering: {other:?}"
                );
                EffectStatus {
                    kind: EffectKind::Hotkeys,
                    desired_revision: revision,
                    applied_revision: EffectRevision::default(),
                    health: stopped_health(),
                }
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn status(&self) -> HotkeyStatus {
        match self.actor.call(Message::Status, None).await {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the hotkey actor did not report its status: {other:?}");
                HotkeyStatus {
                    applied_revision: EffectRevision::default(),
                    health: stopped_health(),
                    registered: BTreeMap::new(),
                }
            }
        }
    }
}

/// Not retryable: an actor that is gone never answers a retry either.
fn stopped_health() -> EffectHealth {
    HotkeyEffectError::Stopped.health()
}
