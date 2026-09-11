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
use crate::client::{
    hotkey::{HotkeyClient, ports::HotkeyBindings},
    system_proxy::SystemProxyClient,
};

pub struct ApplicationEffectExecutor {
    system_proxy: SystemProxyClient,
    hotkeys: HotkeyClient,
}

impl ApplicationEffectExecutor {
    pub fn new(system_proxy: SystemProxyClient, hotkeys: HotkeyClient) -> Self {
        Self {
            system_proxy,
            hotkeys,
        }
    }

    /// The facade rejects an unparsable list before it is committed, so getting
    /// one here means it arrived from somewhere else — a migration, or a file
    /// edited by hand. The config stays as written and the effect degrades.
    async fn apply_hotkeys(&self, revision: EffectRevision, raw: &[String]) -> EffectStatus {
        match HotkeyBindings::parse(raw) {
            Ok(desired) => self.hotkeys.reconcile(revision, desired).await,
            Err(error) => EffectStatus {
                kind: EffectKind::Hotkeys,
                desired_revision: revision,
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Degraded {
                    code: "hotkey_invalid_bindings",
                    message: error.to_string(),
                    retryable: false,
                },
            },
        }
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
        let mut hotkeys = None;
        for effect in plan.effects() {
            match effect {
                ApplicationEffect::SystemProxy(desired) => proxy = Some(desired.clone()),
                ApplicationEffect::ProxyGuard(desired) => guard = Some(*desired),
                ApplicationEffect::AutoLaunch(enabled) => auto_launch = Some(*enabled),
                ApplicationEffect::Hotkeys(desired) => hotkeys = Some(desired.clone()),
                _ => {}
            }
        }

        // One round trip, not three: all three belong to the same actor, and
        // the guard's timer depends on the proxy value applied beside it.
        let mut owned = self
            .system_proxy
            .reconcile(revision, proxy, guard, auto_launch)
            .await;
        if let Some(desired) = hotkeys {
            owned.push(self.apply_hotkeys(revision, &desired).await);
        }

        plan.effects()
            .iter()
            .map(|effect| {
                let kind = effect.kind();
                owned
                    .iter()
                    .find(|status| status.kind == kind)
                    .cloned()
                    // Locale, Logger, Widget and Tray are still applied by
                    // `feat::patch_verge`; reporting them healthy keeps this
                    // identical to the no-op port until the UI-effect task
                    // takes them over.
                    .unwrap_or_else(|| unowned(kind, revision))
            })
            .collect()
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        vec![
            self.system_proxy.restore().await,
            self.hotkeys.unregister_all().await,
        ]
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
