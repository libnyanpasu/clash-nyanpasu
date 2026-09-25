//! Peripheral effects are queued after commit and never vote on source state.
use self::{plan::ApplicationEffectInputs, status::degradation_of};
use super::{NyanpasuClient, Result, runtime};

pub(crate) mod actor;
pub mod executor;
pub mod plan;
pub mod ports;
pub mod status;

impl NyanpasuClient {
    fn effect_inputs(&self) -> ApplicationEffectInputs {
        ApplicationEffectInputs::project(
            &self.inner.application.snapshot().state,
            &self.inner.clash_config.snapshot().state,
            self.inner.ports.confirmed(),
        )
    }

    pub async fn reconcile_application_effects(&self) -> Result<runtime::MutationOutcome<()>> {
        self.inner.effects.reconcile(self.effect_inputs());
        Ok(runtime::MutationOutcome::from_parts((), Vec::new()))
    }

    pub async fn shutdown_application_effects(&self) -> Vec<runtime::Degradation> {
        self.inner
            .effects
            .shutdown()
            .await
            .iter()
            .filter_map(degradation_of)
            .collect()
    }
}

#[cfg(test)]
mod tests;
