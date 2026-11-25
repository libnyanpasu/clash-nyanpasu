//! Pure hotkey validation and application action dispatch.

pub mod ports;
#[cfg(test)]
mod tests;

use nyanpasu_config::{
    application::NyanpasuAppConfigPatch,
    clash::config::{
        ClashConfigPatch, TransparentProxyMode,
        overrides::{ClashGuardOverridesPatch, Mode},
    },
};
use nyanpasu_runtime_values::{
    configuration::BackgroundChangeOrigin, hotkey::HotkeyAction, runtime::MutationOutcome,
};
use snafu::ResultExt as _;

use self::ports::{HotkeyBindings, WindowError};
use super::{NyanpasuClient, Result, transparent_proxy};
use crate::state::{
    config_error::{ConfigError, ValidateHotkeysSnafu},
    mutation::ports::Attribution,
};

/// Rejects a hotkey list before anything is written.
///
/// Validation belongs in front of the commit, not in the effect that follows
/// it: a typo would otherwise be persisted and then reported as a degraded side
/// effect, leaving the stored config holding bindings that can never register.
pub(crate) fn validate_bindings(
    raw: &[String],
    accelerators: Option<&dyn ports::AcceleratorValidator>,
) -> std::result::Result<(), ConfigError> {
    let accelerators = accelerators.ok_or(ConfigError::ValidateHotkeyCapability)?;
    HotkeyBindings::parse(raw, accelerators).context(ValidateHotkeysSnafu)?;
    Ok(())
}

impl NyanpasuClient {
    /// Runs what a shortcut or a tray item asked for.
    ///
    /// Every action but the dashboard toggle is an ordinary config mutation, so
    /// it goes through the same commit-and-reconcile pipeline as a settings
    /// change and reports the same degradations.
    pub async fn dispatch_hotkey_action(&self, action: HotkeyAction) -> Result<()> {
        // No caller waits on a tray item or a shortcut, so the owner that
        // refuses its change reports the candidate it kept, under the
        // setting the action changes.
        let by = Attribution {
            refused: action_origin(action),
            concludes: None,
        };
        match action {
            HotkeyAction::OpenOrCloseDashboard => {
                let window = self
                    .inner
                    .window
                    .as_ref()
                    .ok_or(WindowError::ToggleDashboardUnsupported)?;
                window.toggle_dashboard().await?;
                Ok(())
            }
            HotkeyAction::ClashModeRule => self.set_clash_mode(Mode::Rule, by).await,
            HotkeyAction::ClashModeGlobal => self.set_clash_mode(Mode::Global, by).await,
            HotkeyAction::ClashModeDirect => self.set_clash_mode(Mode::Direct, by).await,
            HotkeyAction::ToggleSystemProxy => {
                let enabled = self.get_app_config().await?.enable_system_proxy;
                self.set_system_proxy(!enabled, by).await
            }
            HotkeyAction::EnableSystemProxy => self.set_system_proxy(true, by).await,
            HotkeyAction::DisableSystemProxy => self.set_system_proxy(false, by).await,
            HotkeyAction::ToggleTunMode => {
                let enabled = self.get_clash_config().await?.enable_tun_mode;
                self.set_tun_mode(!enabled, by).await
            }
            HotkeyAction::EnableTunMode => self.set_tun_mode(true, by).await,
            HotkeyAction::DisableTunMode => self.set_tun_mode(false, by).await,
            HotkeyAction::ToggleTransparentProxy => {
                let enabled = self.get_clash_config().await?.transparent_proxy.mode
                    != TransparentProxyMode::Disabled;
                self.set_transparent_proxy(!enabled, by).await
            }
            HotkeyAction::EnableTransparentProxy => self.set_transparent_proxy(true, by).await,
            HotkeyAction::DisableTransparentProxy => self.set_transparent_proxy(false, by).await,
        }
    }

    async fn set_clash_mode(&self, mode: Mode, by: Attribution) -> Result<()> {
        let outcome = self
            .patch_runtime_overrides_for(
                ClashGuardOverridesPatch {
                    mode: Some(mode),
                    ..Default::default()
                },
                by,
            )
            .await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_system_proxy(&self, enabled: bool, by: Attribution) -> Result<()> {
        let outcome = self
            .patch_app_config_for(
                NyanpasuAppConfigPatch {
                    enable_system_proxy: Some(enabled),
                    ..Default::default()
                },
                by,
            )
            .await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_transparent_proxy(&self, enabled: bool, by: Attribution) -> Result<()> {
        let config = self.get_clash_config().await?;
        if enabled && config.transparent_proxy.mode == TransparentProxyMode::Disabled {
            let status = self.get_transparent_proxy_status().await?;
            if !status.supported {
                return Err("the service does not support transparent capture".into());
            }
        }
        let outcome = self
            .patch_clash_config_for(transparent_proxy::switch_patch(&config, enabled), by)
            .await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_tun_mode(&self, enabled: bool, by: Attribution) -> Result<()> {
        let outcome = self
            .patch_clash_config_for(
                ClashConfigPatch {
                    enable_tun_mode: Some(enabled),
                    ..Default::default()
                },
                by,
            )
            .await?;
        log_degradations(&outcome);
        Ok(())
    }
}

/// The setting `action` changes, which its refusals are reported under.
fn action_origin(action: HotkeyAction) -> Option<BackgroundChangeOrigin> {
    Some(match action {
        HotkeyAction::OpenOrCloseDashboard => return None,
        HotkeyAction::ClashModeRule
        | HotkeyAction::ClashModeGlobal
        | HotkeyAction::ClashModeDirect => BackgroundChangeOrigin::ActionClashMode,
        HotkeyAction::ToggleSystemProxy
        | HotkeyAction::EnableSystemProxy
        | HotkeyAction::DisableSystemProxy => BackgroundChangeOrigin::ActionSystemProxy,
        HotkeyAction::ToggleTunMode
        | HotkeyAction::EnableTunMode
        | HotkeyAction::DisableTunMode => BackgroundChangeOrigin::ActionTunMode,
        HotkeyAction::ToggleTransparentProxy
        | HotkeyAction::EnableTransparentProxy
        | HotkeyAction::DisableTransparentProxy => BackgroundChangeOrigin::ActionTransparentProxy,
    })
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
