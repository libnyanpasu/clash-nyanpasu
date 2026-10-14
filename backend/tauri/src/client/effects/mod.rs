//! Peripheral effects are queued after commit and never vote on source state.
use self::plan::TrayView;
use super::NyanpasuClient;
use nyanpasu_core::effects::plan::ApplicationEffectInputs;

pub mod plan;
pub mod presentation;

impl NyanpasuClient {
    /// What the tray renders from the committed configuration, for the tray
    /// to start from before the first tray effect reaches it.
    pub fn tray_view(&self) -> TrayView {
        let inputs = ApplicationEffectInputs::project(
            &self.inner.application.snapshot().state,
            &self.inner.clash_config.snapshot().state,
            self.inner.ports.confirmed(),
        );
        presentation::tray_view(&inputs.app, &inputs.clash)
    }
}

#[cfg(test)]
mod tests;
