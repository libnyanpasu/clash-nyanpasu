//! The capture switches written through the configuration owners, so a rule
//! and a settings change commit, reconcile and degrade the same way.

use nyanpasu_config::{
    application::{MechanismAction, MechanismActions, NyanpasuAppConfigPatch},
    clash::config::{ClashConfigPatch, TransparentProxyMode},
};
use nyanpasu_runtime_values::network_automation::{MechanismOutcome, MechanismOutcomes};

use super::ports::CaptureSwitches;
use crate::{
    actor_v2::CoreClient,
    client::{application::ApplicationClient, clash_config::ClashConfigClient, transparent_proxy},
};

pub(crate) struct ConfigCaptureSwitches {
    pub(crate) application: ApplicationClient,
    pub(crate) clash_config: ClashConfigClient,
    pub(crate) core: CoreClient,
}

#[async_trait::async_trait]
impl CaptureSwitches for ConfigCaptureSwitches {
    async fn apply(&self, actions: MechanismActions) -> MechanismOutcomes {
        let system_proxy = self.apply_system_proxy(actions.system_proxy).await;
        let (tun, transparent_proxy) = self
            .apply_capture(actions.tun, actions.transparent_proxy)
            .await;

        MechanismOutcomes {
            system_proxy,
            tun,
            transparent_proxy,
        }
    }
}

impl ConfigCaptureSwitches {
    async fn apply_system_proxy(&self, action: MechanismAction) -> MechanismOutcome {
        let enabled = self.application.snapshot().state.enable_system_proxy;
        let Some(target) = requested(action).filter(|target| *target != enabled) else {
            return MechanismOutcome::Unchanged;
        };

        let patch = NyanpasuAppConfigPatch {
            enable_system_proxy: Some(target),
            ..Default::default()
        };
        match self.application.patch(patch).await {
            Ok(_) => switched(target),
            Err(error) => failed(&error),
        }
    }

    /// TUN and transparent proxy share the clash config and exclude each
    /// other, so both switches travel in one patch: enabling one turns the
    /// other off even when its own action is `keep`.
    async fn apply_capture(
        &self,
        tun: MechanismAction,
        transparent_proxy: MechanismAction,
    ) -> (MechanismOutcome, MechanismOutcome) {
        let config = self.clash_config.snapshot().state;
        let tun_enabled = config.enable_tun_mode;
        let transparent_enabled = config.transparent_proxy.mode != TransparentProxyMode::Disabled;

        let mut tun_target = requested(tun).unwrap_or(tun_enabled);
        let mut transparent_target = requested(transparent_proxy).unwrap_or(transparent_enabled);
        if tun == MechanismAction::Enable {
            transparent_target = false;
        }
        if transparent_proxy == MechanismAction::Enable {
            tun_target = false;
        }

        let mut transparent_refusal = None;
        if transparent_target && !transparent_enabled {
            let refusal = match transparent_proxy::status(&self.core).await {
                Ok(status) if status.supported => None,
                Ok(_) => Some("the service does not support transparent capture".to_owned()),
                Err(error) => Some(format!("{error:#}")),
            };
            if let Some(message) = refusal {
                // Nothing replaces TUN, so it stays as its own action left it.
                transparent_refusal = Some(MechanismOutcome::Failed { message });
                transparent_target = transparent_enabled;
                tun_target = requested(tun).unwrap_or(tun_enabled);
            }
        }

        let tun_changes = tun_target != tun_enabled;
        let transparent_changes = transparent_target != transparent_enabled;
        if !tun_changes && !transparent_changes {
            return (
                MechanismOutcome::Unchanged,
                transparent_refusal.unwrap_or(MechanismOutcome::Unchanged),
            );
        }

        let mut patch = if transparent_changes {
            transparent_proxy::switch_patch(&config, transparent_target)
        } else {
            ClashConfigPatch::default()
        };
        patch.enable_tun_mode = tun_changes.then_some(tun_target);

        let application = self.application.snapshot().state;
        let refusal = match self.clash_config.patch_validated(&application, patch).await {
            Ok(_) => None,
            Err(error) => Some(failed(&error)),
        };
        let outcome = |changes: bool, target: bool| match (&refusal, changes) {
            (_, false) => MechanismOutcome::Unchanged,
            (Some(refusal), true) => refusal.clone(),
            (None, true) => switched(target),
        };

        (
            outcome(tun_changes, tun_target),
            transparent_refusal.unwrap_or_else(|| outcome(transparent_changes, transparent_target)),
        )
    }
}

fn requested(action: MechanismAction) -> Option<bool> {
    match action {
        MechanismAction::Keep => None,
        MechanismAction::Enable => Some(true),
        MechanismAction::Disable => Some(false),
    }
}

fn switched(enabled: bool) -> MechanismOutcome {
    if enabled {
        MechanismOutcome::Enabled
    } else {
        MechanismOutcome::Disabled
    }
}

fn failed(error: &dyn std::error::Error) -> MechanismOutcome {
    MechanismOutcome::Failed {
        message: error.to_string(),
    }
}
