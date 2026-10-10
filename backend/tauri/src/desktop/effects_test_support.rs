//! Private copies for host tests of consumers of the core effects contract.
use nyanpasu_config::runtime::executor::ResolvedPortBindings;
use nyanpasu_core::effects::{
    EffectKind,
    plan::{ApplicationEffectFields, ApplicationEffectPlan, ClashEffectFields},
    ports::{ApplicationEffectsPort, CommitNotifications},
    status::{EffectHealth, EffectRevision, EffectStatus},
};

mockall::mock! {
    pub(super) ApplicationEffectsPort {}
    #[async_trait::async_trait]
    impl ApplicationEffectsPort for ApplicationEffectsPort {
        async fn apply(&self, revision: EffectRevision, plan: ApplicationEffectPlan) -> Vec<EffectStatus>;
    }
}

#[derive(Debug, Default)]
pub(super) struct NoopApplicationEffects;

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

pub(crate) struct NoopCommitNotifications;
impl CommitNotifications for NoopCommitNotifications {
    fn application_committed(&self, _: ApplicationEffectFields, _: Vec<EffectKind>) {}
    fn clash_committed(&self, _: ClashEffectFields) {}
    fn profiles_committed(&self) {}
    fn runtime_bound(&self, _: Option<ResolvedPortBindings>, _: bool) {}
    fn publish_full(&self, _: Option<ResolvedPortBindings>) {}
}
