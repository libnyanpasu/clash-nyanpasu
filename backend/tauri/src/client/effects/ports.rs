//! The single dispatch seam between the facade and the owners of each effect.
//!
//! One trait, not one per effect: the facade holds a single dependency and the
//! fan-out to actor clients and adapters stays inside the executor. There is no
//! lookup API here, so this is not a service locator.

use nyanpasu_config::runtime::executor::ResolvedPortBindings;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::effects::plan::ApplicationEffectInputs;
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
    }
}

/// Post-commit notification only: never a source-transaction vote.
///
/// One method per slice of [`ApplicationEffectInputs`], and each slice has
/// exactly one sender: the serial owner of that domain. Nobody reads or
/// forwards a sibling's slice; the effects owner combines them.
///
/// [`ApplicationEffectInputs`]: super::plan::ApplicationEffectInputs
pub(crate) trait CommitNotifications: Send + Sync + 'static {
    /// The application owner, once its commit has settled. `requested` names
    /// the owners the request asked for even when their inputs did not move.
    fn application_committed(
        &self,
        fields: super::plan::ApplicationEffectFields,
        requested: Vec<super::plan::EffectKind>,
    );

    /// The clash config owner, once its commit has settled.
    fn clash_committed(&self, fields: super::plan::ClashEffectFields);

    /// The profiles owner. No effect reads the profiles, so a commit only asks
    /// the tray for a partial refresh.
    fn profiles_committed(&self);

    /// The Runtime, after a mutation's Confirm or Cancel, a lifecycle command
    /// or a recovery: the ports the core is bound to now. `refresh` asks the
    /// tray for a partial refresh even when nothing it reads moved.
    fn runtime_bound(&self, ports: Option<ResolvedPortBindings>, refresh: bool);

    /// Hands every owner its complete desired value and rebuilds the tray.
    /// StartupReconcile sends it once (T10 §1.9), with the ports it bound.
    fn publish_full(&self, ports: Option<ResolvedPortBindings>);
}

#[cfg(test)]
pub(crate) struct NoopCommitNotifications;
#[cfg(test)]
impl CommitNotifications for NoopCommitNotifications {
    fn application_committed(
        &self,
        _: super::plan::ApplicationEffectFields,
        _: Vec<super::plan::EffectKind>,
    ) {
    }

    fn clash_committed(&self, _: super::plan::ClashEffectFields) {}

    fn profiles_committed(&self) {}

    fn runtime_bound(&self, _: Option<ResolvedPortBindings>, _: bool) {}

    fn publish_full(&self, _: Option<ResolvedPortBindings>) {}
}
