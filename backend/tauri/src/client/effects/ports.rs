//! The single dispatch seam between the facade and the owners of each effect.
//!
//! One trait, not one per effect: the facade holds a single dependency and the
//! fan-out to actor clients and adapters stays inside the executor. There is no
//! lookup API here, so this is not a service locator.

#[cfg(test)]
use super::status::EffectHealth;
use super::{
    plan::ApplicationEffectPlan,
    status::{EffectRevision, EffectStatus},
};

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ApplicationEffectsPort: Send + Sync + 'static {
    /// Applies the plan in its own order and reports one status per effect.
    ///
    /// Infallible by design: the configuration is already committed, so a
    /// failure has to surface as health rather than as an error that would
    /// suggest nothing was written. Each owner compares `revision` against what
    /// it last applied and reports `Superseded` for anything older.
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus>;

    /// Shutdown path: restores the system state the app found and drops its OS
    /// registrations. Never fails.
    async fn shutdown(&self) -> Vec<EffectStatus>;
}

/// Accepts every plan and changes nothing. The composition root now assembles
/// a real executor, so this is only for tests that care about the pipeline
/// rather than about any particular effect.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct NoopApplicationEffects;

#[cfg(test)]
#[async_trait::async_trait]
impl ApplicationEffectsPort for NoopApplicationEffects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        plan.effects()
            .iter()
            .map(|effect| EffectStatus {
                kind: effect.kind(),
                desired_revision: revision,
                applied_revision: revision,
                health: EffectHealth::Healthy,
            })
            .collect()
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::effects::{plan::ApplicationEffectInputs, status::degradation_of};
    use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};

    #[tokio::test]
    async fn noop_port_reports_healthy_for_every_effect() {
        let inputs = ApplicationEffectInputs::project(
            &NyanpasuAppConfig::default(),
            &ClashConfig::default(),
            None,
        );
        let plan = ApplicationEffectPlan::full(&inputs);
        let revision = EffectRevision::new(3);

        let statuses = NoopApplicationEffects.apply(revision, plan.clone()).await;

        assert_eq!(statuses.len(), plan.effects().len());
        for (status, effect) in statuses.iter().zip(plan.effects()) {
            assert_eq!(status.kind, effect.kind());
            assert_eq!(status.desired_revision, revision);
            assert_eq!(status.applied_revision, revision);
            assert_eq!(status.health, EffectHealth::Healthy);
        }
        assert!(
            statuses
                .iter()
                .all(|status| degradation_of(status).is_none()),
            "a no-op port never degrades"
        );
        assert!(NoopApplicationEffects.shutdown().await.is_empty());
    }
}
