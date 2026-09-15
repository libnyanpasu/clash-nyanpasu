//! The single implementation of [`ApplicationEffectsPort`]: it fans one plan
//! out to the owner of each effect.
//!
//! The fan-out is by capability, not by lookup — there is no `get::<T>()` here,
//! so the facade keeps one dependency and this stays a dispatcher rather than a
//! service locator.

use super::{
    plan::{ApplicationEffect, ApplicationEffectPlan, EffectKind},
    ports::ApplicationEffectsPort,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::system_proxy::SystemProxyClient;

pub struct ApplicationEffectExecutor {
    system_proxy: SystemProxyClient,
}

impl ApplicationEffectExecutor {
    pub fn new(system_proxy: SystemProxyClient) -> Self {
        Self { system_proxy }
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for ApplicationEffectExecutor {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let mut proxy = None;
        let mut guard = None;
        let mut auto_launch = None;
        for effect in plan.effects() {
            match effect {
                ApplicationEffect::SystemProxy(desired) => proxy = Some(desired.clone()),
                ApplicationEffect::ProxyGuard(desired) => guard = Some(*desired),
                ApplicationEffect::AutoLaunch(enabled) => auto_launch = Some(*enabled),
                _ => {}
            }
        }

        // One round trip, not three: all three belong to the same actor, and
        // the guard's timer depends on the proxy value applied beside it.
        let owned = self
            .system_proxy
            .reconcile(revision, proxy, guard, auto_launch)
            .await;

        plan.effects()
            .iter()
            .map(|effect| {
                let kind = effect.kind();
                owned
                    .iter()
                    .find(|status| status.kind == kind)
                    .cloned()
                    // Locale, Logger, Hotkeys, Widget and Tray are still
                    // applied by `feat::patch_verge`; reporting them healthy
                    // keeps this identical to the no-op port until the hotkey
                    // and UI-effect tasks take them over.
                    .unwrap_or_else(|| unowned(kind, revision))
            })
            .collect()
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        vec![self.system_proxy.restore().await]
    }
}

fn unowned(kind: EffectKind, revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: revision,
        health: EffectHealth::Healthy,
    }
}
