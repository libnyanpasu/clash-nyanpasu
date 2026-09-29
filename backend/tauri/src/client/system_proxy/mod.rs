//! System proxy, PAC and auto-launch, owned by one actor behind a typed client.
//!
//! Callers never see a `ractor::ActorRef`: they hand a revision and the desired
//! values in, and get one [`EffectStatus`] per effect back.

mod actor;
pub mod adapters;
pub mod ports;

#[cfg(test)]
mod tests;

use std::time::Duration;

use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio_util::task::TaskTracker;

use self::{
    actor::{Message, SystemProxyActor, requested_kinds},
    ports::OsProxyConfig,
};
use crate::client::effects::{
    plan::{ProxyGuardDesired, SystemProxyDesired},
    status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
};

pub use self::actor::Args as SystemProxyArgs;

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
    /// The actor puts back the proxy settings it found once `args.shutdown`
    /// is cancelled, and `tasks` waits for that.
    pub async fn spawn(args: SystemProxyArgs, tasks: &TaskTracker) -> anyhow::Result<Self> {
        let shutdown = args.shutdown.clone();
        let (actor, _handle) = Actor::spawn(None, SystemProxyActor, args).await?;
        crate::client::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self { actor })
    }

    /// One plan's system-owned effects in a single round trip. `None` means the
    /// plan did not ask for that effect, so it is left exactly as it is.
    ///
    /// Unbounded on purpose: the only caller is the effects group, which must
    /// stay occupied until the actor has settled this request. Giving up early
    /// would free the group and let a retry queue behind work still running.
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
                None,
            )
            .await;

        match call {
            Ok(CallResult::Success(statuses)) => statuses,
            other => {
                tracing::warn!(
                    revision = revision.get(),
                    "the system proxy actor stopped before answering: {other:?}"
                );
                kinds
                    .into_iter()
                    .map(|kind| EffectStatus {
                        kind,
                        desired_revision: revision,
                        applied_revision: EffectRevision::default(),
                        health: EffectHealth::Degraded {
                            code: EffectFailureCode::SystemProxyStopped,
                            message: "the system proxy actor stopped before answering".to_owned(),
                            retryable: false,
                        },
                    })
                    .collect()
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn status(&self) -> SystemProxyStatus {
        match self.actor.call(Message::Status, None).await {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the system proxy actor did not report its status: {other:?}");
                SystemProxyStatus {
                    applied_revision: EffectRevision::default(),
                    health: EffectHealth::Degraded {
                        code: EffectFailureCode::SystemProxyStopped,
                        message: "the system proxy actor stopped before answering".to_owned(),
                        retryable: false,
                    },
                    desired: None,
                    applied_os_proxy: None,
                    guard_active: false,
                    guard_interval: None,
                    pac_active: false,
                }
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
