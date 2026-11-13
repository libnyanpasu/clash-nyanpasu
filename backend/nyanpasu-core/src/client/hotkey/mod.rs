//! Pure hotkey validation and application action dispatch.

pub mod ports;
#[cfg(test)]
mod tests;

use nyanpasu_config::{
    application::NyanpasuAppConfigPatch,
    clash::config::{
        ClashConfig, ClashConfigPatch, TransparentProxyConfigPatch, TransparentProxyMode,
        clash_strategy::{PortStrategy, PortStrategyKind},
        overrides::{ClashGuardOverridesPatch, Mode},
    },
};
use nyanpasu_runtime_values::{hotkey::HotkeyAction, runtime::MutationOutcome};
use snafu::ResultExt as _;

use self::ports::{HotkeyBindings, WindowError};
use super::{NyanpasuClient, Result};
use crate::state::config_error::{ConfigError, ValidateHotkeysSnafu};

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
            HotkeyAction::ClashModeRule => self.set_clash_mode(Mode::Rule).await,
            HotkeyAction::ClashModeGlobal => self.set_clash_mode(Mode::Global).await,
            HotkeyAction::ClashModeDirect => self.set_clash_mode(Mode::Direct).await,
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
            HotkeyAction::ToggleTransparentProxy => {
                let enabled = self.get_clash_config().await?.transparent_proxy.mode
                    != TransparentProxyMode::Disabled;
                self.set_transparent_proxy(!enabled).await
            }
            HotkeyAction::EnableTransparentProxy => self.set_transparent_proxy(true).await,
            HotkeyAction::DisableTransparentProxy => self.set_transparent_proxy(false).await,
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

    async fn set_transparent_proxy(&self, enabled: bool) -> Result<()> {
        let config = self.get_clash_config().await?;
        if enabled && config.transparent_proxy.mode == TransparentProxyMode::Disabled {
            let status = self.get_transparent_proxy_status().await?;
            if !status.supported {
                return Err("the service does not support transparent capture".into());
            }
        }
        let outcome = self
            .patch_clash_config(transparent_proxy_patch(&config, enabled))
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

fn transparent_proxy_patch(config: &ClashConfig, enabled: bool) -> ClashConfigPatch {
    let mode = if enabled {
        match config.transparent_proxy.mode {
            TransparentProxyMode::Disabled => TransparentProxyMode::Tproxy,
            mode => mode,
        }
    } else {
        TransparentProxyMode::Disabled
    };
    ClashConfigPatch {
        transparent_proxy: TransparentProxyConfigPatch {
            mode: Some(mode),
            ..Default::default()
        },
        tproxy_port: (mode == TransparentProxyMode::Tproxy && config.tproxy_port.is_none())
            .then_some(Some(PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: 7894,
            })),
        ..Default::default()
    }
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
