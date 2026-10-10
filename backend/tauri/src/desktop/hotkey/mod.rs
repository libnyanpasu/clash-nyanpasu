//! Global shortcuts, owned by one actor behind a typed client.
//!
//! Callers never see a `ractor::ActorRef`: they hand a revision and the desired
//! bindings in and get one [`EffectStatus`] back. The actor never calls the
//! facade either — a pressed shortcut travels out through a channel and back in
//! through [`dispatch_hotkey_action`], so the two never form a
//! cycle.

mod actor;
pub mod adapters;
pub(crate) mod error;
pub mod ports;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use nyanpasu_config::{
    application::NyanpasuAppConfigPatch,
    clash::config::{
        ClashConfigPatch,
        overrides::{ClashGuardOverridesPatch, Mode},
    },
};
use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio_util::task::TaskTracker;

use self::{
    actor::{HotkeyActor, Message},
    error::HotkeyEffectError,
};
use nyanpasu_core::{
    NyanpasuClient,
    client::{Result, runtime::MutationOutcome},
    effects::{
        EffectKind,
        status::{EffectHealth, EffectRevision, EffectStatus},
    },
    hotkey::{HotkeyAction, HotkeyBindings},
};

pub use self::actor::Args as HotkeyArgs;

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
    pub async fn spawn(args: HotkeyArgs, tasks: &TaskTracker) -> anyhow::Result<Self> {
        let shutdown = args.shutdown.clone();
        let (actor, _handle) = Actor::spawn(None, HotkeyActor, args).await?;
        nyanpasu_core::tasks::drain_on_shutdown(tasks, shutdown, actor.get_cell());
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

/// Runs what a shortcut or a tray item asked for.
///
/// Every action but the dashboard toggle is an ordinary config mutation, so
/// it goes through the same commit-and-reconcile pipeline as a settings
/// change and reports the same degradations.
pub(crate) async fn dispatch_hotkey_action(
    client: &NyanpasuClient,
    window: &dyn ports::WindowControl,
    action: HotkeyAction,
) -> anyhow::Result<()> {
    match action {
        HotkeyAction::OpenOrCloseDashboard => {
            window.toggle_dashboard().await?;
            Ok(())
        }
        HotkeyAction::ClashModeRule => Ok(set_clash_mode(client, Mode::Rule).await?),
        HotkeyAction::ClashModeGlobal => Ok(set_clash_mode(client, Mode::Global).await?),
        HotkeyAction::ClashModeDirect => Ok(set_clash_mode(client, Mode::Direct).await?),
        HotkeyAction::ClashModeScript => Ok(set_clash_mode(client, Mode::Script).await?),
        HotkeyAction::ToggleSystemProxy => {
            let enabled = client.get_app_config().await?.enable_system_proxy;
            Ok(set_system_proxy(client, !enabled).await?)
        }
        HotkeyAction::EnableSystemProxy => Ok(set_system_proxy(client, true).await?),
        HotkeyAction::DisableSystemProxy => Ok(set_system_proxy(client, false).await?),
        HotkeyAction::ToggleTunMode => {
            let enabled = client.get_clash_config().await?.enable_tun_mode;
            Ok(set_tun_mode(client, !enabled).await?)
        }
        HotkeyAction::EnableTunMode => Ok(set_tun_mode(client, true).await?),
        HotkeyAction::DisableTunMode => Ok(set_tun_mode(client, false).await?),
    }
}

async fn set_clash_mode(client: &NyanpasuClient, mode: Mode) -> Result<()> {
    let outcome = client
        .patch_runtime_overrides(ClashGuardOverridesPatch {
            mode: Some(mode),
            ..Default::default()
        })
        .await?;
    log_degradations(&outcome);
    Ok(())
}

async fn set_system_proxy(client: &NyanpasuClient, enabled: bool) -> Result<()> {
    let outcome = client
        .patch_app_config(NyanpasuAppConfigPatch {
            enable_system_proxy: Some(enabled),
            ..Default::default()
        })
        .await?;
    log_degradations(&outcome);
    Ok(())
}

async fn set_tun_mode(client: &NyanpasuClient, enabled: bool) -> Result<()> {
    let outcome = client
        .patch_clash_config(ClashConfigPatch {
            enable_tun_mode: Some(enabled),
            ..Default::default()
        })
        .await?;
    log_degradations(&outcome);
    Ok(())
}

/// The action already committed, so a degraded side effect is reported rather
/// than turned into a failure the caller would retry.
fn log_degradations(outcome: &MutationOutcome<()>) {
    for degradation in outcome.degradations() {
        tracing::warn!(
            reason = ?degradation.reason,
            message = %degradation.message,
            "a hotkey action committed with a degraded side effect"
        );
    }
}
