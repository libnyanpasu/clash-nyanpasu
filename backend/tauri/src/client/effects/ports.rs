//! The single dispatch seam between the facade and the owners of each effect.
//!
//! One trait, not one per effect: the facade holds a single dependency and the
//! fan-out to actor clients and adapters stays inside the executor. There is no
//! lookup API here, so this is not a service locator.

use std::time::Duration;

#[cfg(test)]
use super::status::EffectHealth;
use super::{
    plan::ApplicationEffectPlan,
    status::{EffectRevision, EffectStatus},
};
use crate::client::StepOutcome;

/// What the shutdown's independent cleanups confirmed, one outcome per owner
/// (T10 §5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectsShutdown {
    pub system_proxy: StepOutcome,
    pub hotkeys: StepOutcome,
    pub widget: StepOutcome,
}

impl EffectsShutdown {
    pub fn children(&self) -> Vec<(&'static str, StepOutcome)> {
        vec![
            ("SystemProxy", self.system_proxy.clone()),
            ("Hotkeys", self.hotkeys.clone()),
            ("Widget", self.widget.clone()),
        ]
    }
}

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

    /// Tells every owner the shutdown began, without waiting for anything
    /// (T10 §5.5 step 2): work already running abandons what it waits on.
    fn begin_shutdown(&self);

    /// Shutdown path: restores the system state the app found, drops its OS
    /// registrations and stops the widget. The three are started together and
    /// each is bounded on its own within `budget`, so one that hangs keeps
    /// neither of the others from finishing. Never fails.
    async fn shutdown(&self, budget: Duration) -> EffectsShutdown;
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

    fn begin_shutdown(&self) {}

    async fn shutdown(&self, _: Duration) -> EffectsShutdown {
        let nothing = || StepOutcome::skipped("no effect owners");
        EffectsShutdown {
            system_proxy: nothing(),
            hotkeys: nothing(),
            widget: nothing(),
        }
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
        assert!(
            NoopApplicationEffects
                .shutdown(Duration::ZERO)
                .await
                .children()
                .iter()
                .all(|(_, outcome)| matches!(outcome, StepOutcome::Skipped { .. }))
        );
    }
}

/// Post-commit notification only: never a source-transaction vote.
pub(crate) trait CommitNotifications: Send + Sync + 'static {
    fn committed(
        &self,
        inputs: super::plan::ApplicationEffectInputs,
        refresh: bool,
        requested: Vec<super::plan::EffectKind>,
    );

    /// Hands every owner its complete desired value and rebuilds the tray.
    /// StartupReconcile sends it once (T10 §1.9).
    fn publish_full(&self, inputs: super::plan::ApplicationEffectInputs);
}

#[cfg(test)]
pub(crate) struct NoopCommitNotifications;
#[cfg(test)]
impl CommitNotifications for NoopCommitNotifications {
    fn committed(
        &self,
        _: super::plan::ApplicationEffectInputs,
        _: bool,
        _: Vec<super::plan::EffectKind>,
    ) {
    }

    fn publish_full(&self, _: super::plan::ApplicationEffectInputs) {}
}
