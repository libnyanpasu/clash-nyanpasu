//! Global shortcuts, owned by one actor behind a typed client.
//!
//! Callers never see a `ractor::ActorRef`: they hand a revision and the desired
//! bindings in and get one [`EffectStatus`] back. The actor never calls the
//! facade either — a pressed shortcut travels out through a channel and back in
//! through [`NyanpasuClient::dispatch_hotkey_action`], so the two never form a
//! cycle.

mod actor;
pub mod adapters;
pub mod ports;

#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, time::Duration};

use nyanpasu_config::{
    application::NyanpasuAppConfigPatch,
    clash::config::{
        ClashConfigPatch,
        overrides::{ClashGuardOverridesPatch, Mode},
    },
};
use ractor::{Actor, ActorRef, rpc::CallResult};

use self::{
    actor::{HotkeyActor, Message},
    ports::{HotkeyAction, HotkeyBindings},
};
use super::{
    NyanpasuClient, Result,
    effects::{
        plan::EffectKind,
        status::{EffectHealth, EffectRevision, EffectStatus},
    },
    runtime::MutationOutcome,
};

pub use self::actor::Args as HotkeyArgs;

/// A grab is a window-server round trip, not a download: five seconds is far
/// more than it takes and still short enough that an IPC command with a user
/// waiting on it cannot hang.
const HOTKEY_RPC_TIMEOUT: Duration = Duration::from_secs(5);

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
    pub async fn spawn(args: HotkeyArgs) -> anyhow::Result<Self> {
        let (actor, _handle) = Actor::spawn(None, HotkeyActor, args).await?;
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
                Some(HOTKEY_RPC_TIMEOUT),
            )
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                // The actor keeps working through the message and updates its
                // own applied revision, so this is retryable and says so.
                tracing::warn!(
                    revision = revision.get(),
                    "the hotkey actor did not answer within its bound: {other:?}"
                );
                timed_out(revision)
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn status(&self) -> HotkeyStatus {
        match self
            .actor
            .call(Message::Status, Some(HOTKEY_RPC_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the hotkey actor did not report its status: {other:?}");
                HotkeyStatus {
                    applied_revision: EffectRevision::default(),
                    health: timeout_health(),
                    registered: BTreeMap::new(),
                }
            }
        }
    }

    pub async fn unregister_all(&self) -> EffectStatus {
        match self
            .actor
            .call(Message::UnregisterAll, Some(HOTKEY_RPC_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the hotkey actor did not release its shortcuts in time: {other:?}");
                timed_out(EffectRevision::default())
            }
        }
    }
}

fn timed_out(revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind: EffectKind::Hotkeys,
        desired_revision: revision,
        applied_revision: EffectRevision::default(),
        health: timeout_health(),
    }
}

fn timeout_health() -> EffectHealth {
    EffectHealth::Degraded {
        code: "hotkey_timeout",
        message: "the hotkey actor did not answer within its bound".to_owned(),
        retryable: true,
    }
}

/// Rejects a hotkey list before anything is written.
///
/// Validation belongs in front of the commit, not in the effect that follows
/// it: a typo would otherwise be persisted and then reported as a degraded side
/// effect, leaving the stored config holding bindings that can never register.
pub(crate) fn validate_bindings(raw: &[String]) -> Result<()> {
    HotkeyBindings::parse(raw).map_err(|error| super::ClientError::Anyhow(error.into()))?;
    Ok(())
}

impl NyanpasuClient {
    /// Runs what a shortcut or a tray item asked for.
    ///
    /// Every action but the dashboard toggle is an ordinary config mutation, so
    /// it goes through the same commit-and-reconcile pipeline as a settings
    /// change and reports the same degradations.
    pub async fn dispatch_hotkey_action(&self, action: HotkeyAction) -> Result<()> {
        match action {
            HotkeyAction::OpenOrCloseDashboard => {
                self.inner.window.toggle_dashboard().await?;
                Ok(())
            }
            HotkeyAction::ClashModeRule => self.set_clash_mode(Mode::Rule).await,
            HotkeyAction::ClashModeGlobal => self.set_clash_mode(Mode::Global).await,
            HotkeyAction::ClashModeDirect => self.set_clash_mode(Mode::Direct).await,
            HotkeyAction::ClashModeScript => self.set_clash_mode(Mode::Script).await,
            HotkeyAction::ToggleSystemProxy => {
                let enabled = self.get_app_config().await?.enable_system_proxy;
                self.set_system_proxy(!enabled).await
            }
            HotkeyAction::EnableSystemProxy => self.set_system_proxy(true).await,
            HotkeyAction::DisableSystemProxy => self.set_system_proxy(false).await,
            HotkeyAction::ToggleTunMode => {
                let enabled = self.get_clash_config().await?.enable_tun_mode;
                self.set_tun_mode(!enabled).await
            }
            HotkeyAction::EnableTunMode => self.set_tun_mode(true).await,
            HotkeyAction::DisableTunMode => self.set_tun_mode(false).await,
        }
    }

    async fn set_clash_mode(&self, mode: Mode) -> Result<()> {
        let outcome = self
            .patch_runtime_overrides(ClashGuardOverridesPatch {
                mode: Some(mode),
                ..Default::default()
            })
            .await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_system_proxy(&self, enabled: bool) -> Result<()> {
        let outcome = self
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_system_proxy: Some(enabled),
                ..Default::default()
            })
            .await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_tun_mode(&self, enabled: bool) -> Result<()> {
        let outcome = self
            .patch_clash_config(ClashConfigPatch {
                enable_tun_mode: Some(enabled),
                ..Default::default()
            })
            .await?;
        log_degradations(&outcome);
        Ok(())
    }
}

/// The action already committed, so a degraded side effect is reported rather
/// than turned into a failure the caller would retry.
fn log_degradations(outcome: &MutationOutcome<()>) {
    for degradation in outcome.degradations() {
        tracing::warn!(
            code = %degradation.code,
            message = %degradation.message,
            "a hotkey action committed with a degraded side effect"
        );
    }
}
