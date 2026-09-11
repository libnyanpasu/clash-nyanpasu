//! System proxy, PAC and auto-launch, owned by one actor behind a typed client.
//!
//! Callers never see a `ractor::ActorRef`: they hand a revision and the desired
//! values in, and get one [`EffectStatus`] per effect back. Every call is
//! bounded, because the effect owner is reached from an IPC command that has a
//! user waiting on it.

mod actor;
pub mod adapters;
pub mod ports;

#[cfg(test)]
mod tests;

use std::time::Duration;

use ractor::{Actor, ActorRef, rpc::CallResult};

use self::{
    actor::{Message, SystemProxyActor, requested_kinds},
    ports::OsProxyConfig,
};
use crate::client::effects::{
    plan::{EffectKind, ProxyGuardDesired, SystemProxyDesired},
    status::{EffectHealth, EffectRevision, EffectStatus},
};

pub use self::actor::Args as SystemProxyArgs;

/// Long enough for an OS write plus one PAC download attempt, short enough that
/// a stuck platform API does not hold an IPC command open indefinitely.
const SYSTEM_PROXY_RPC_TIMEOUT: Duration = Duration::from_secs(15);
/// Shorter: the exit path cannot hang on a proxy that will not answer.
const SYSTEM_PROXY_RESTORE_TIMEOUT: Duration = Duration::from_secs(5);

/// What the actor currently holds. Nothing in the effect protocol needs it —
/// that travels as [`EffectStatus`] — so today only the tests observe the
/// actor's private state through it.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct SystemProxyStatus {
    pub applied_revision: EffectRevision,
    pub health: EffectHealth,
    /// The last value asked for, whether or not the OS accepted it.
    pub desired: Option<SystemProxyDesired>,
    /// The last value the OS accepted, which is what a restore puts back.
    pub applied_os_proxy: Option<OsProxyConfig>,
    pub guard_active: bool,
    pub guard_interval: Option<Duration>,
    pub pac_active: bool,
}

#[derive(Clone)]
pub struct SystemProxyClient {
    actor: ActorRef<Message>,
}

impl SystemProxyClient {
    pub async fn spawn(args: SystemProxyArgs) -> anyhow::Result<Self> {
        let (actor, _handle) = Actor::spawn(None, SystemProxyActor, args).await?;
        Ok(Self { actor })
    }

    /// One plan's system-owned effects in a single round trip. `None` means the
    /// plan did not ask for that effect, so it is left exactly as it is.
    pub async fn reconcile(
        &self,
        revision: EffectRevision,
        proxy: Option<SystemProxyDesired>,
        guard: Option<ProxyGuardDesired>,
        auto_launch: Option<bool>,
    ) -> Vec<EffectStatus> {
        let kinds = requested_kinds(proxy.is_some(), guard.is_some(), auto_launch.is_some());
        if kinds.is_empty() {
            return Vec::new();
        }

        let call = self
            .actor
            .call(
                |reply| Message::Reconcile {
                    revision,
                    proxy,
                    guard,
                    auto_launch,
                    reply,
                },
                Some(SYSTEM_PROXY_RPC_TIMEOUT),
            )
            .await;

        match call {
            Ok(CallResult::Success(statuses)) => statuses,
            other => {
                // The actor keeps working through the message and will update
                // its own applied revision, so this is retryable and says so.
                tracing::warn!(
                    revision = revision.get(),
                    "the system proxy actor did not answer within its bound: {other:?}"
                );
                kinds
                    .into_iter()
                    .map(|kind| timed_out(kind, revision))
                    .collect()
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn status(&self) -> SystemProxyStatus {
        match self
            .actor
            .call(Message::Status, Some(SYSTEM_PROXY_RPC_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the system proxy actor did not report its status: {other:?}");
                SystemProxyStatus {
                    applied_revision: EffectRevision::default(),
                    health: timeout_health(),
                    desired: None,
                    applied_os_proxy: None,
                    guard_active: false,
                    guard_interval: None,
                    pac_active: false,
                }
            }
        }
    }

    pub async fn restore(&self) -> EffectStatus {
        match self
            .actor
            .call(Message::Restore, Some(SYSTEM_PROXY_RESTORE_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the system proxy actor did not restore in time: {other:?}");
                timed_out(EffectKind::SystemProxy, EffectRevision::default())
            }
        }
    }

    /// Delivers one guard tick and waits for it to be handled, so a guard test
    /// synchronises on the mailbox instead of on a clock.
    #[cfg(test)]
    pub async fn tick_guard(&self) {
        let _ = self.actor.cast(Message::GuardTick);
        let _ = self.status().await;
    }
}

fn timed_out(kind: EffectKind, revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: EffectRevision::default(),
        health: timeout_health(),
    }
}

fn timeout_health() -> EffectHealth {
    EffectHealth::Degraded {
        code: "system_proxy_timeout",
        message: "the system proxy actor did not answer within its bound".to_owned(),
        retryable: true,
    }
}
