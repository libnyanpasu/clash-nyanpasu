//! The implementation of [`ApplicationEffectsPort`]: it fans one plan out to
//! the owner of each effect, handing presentation items to the
//! [`PresentationEffectsPort`] at their place in the plan. Inside the client,
//! [`CoreLogCaptureEffects`] adds the Core log owners the client spawns itself.
//!
//! The fan-out is by capability, not by lookup — there is no `get::<T>()` here,
//! so the facade keeps one dependency and this stays a dispatcher rather than a
//! service locator.

use std::sync::Arc;

use nyanpasu_observers::logs::CoreLogsClient;
use nyanpasu_runtime_values::configuration::{EffectFailureCode, EffectKind};

use super::{
    plan::{
        ApplicationEffect, ApplicationEffectPlan, LoggerDesired, ProxyGuardDesired,
        SystemProxyDesired,
    },
    ports::{ApplicationEffectsPort, LogRotation, LoggerRefresher, PresentationEffectsPort},
    status,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::{clash::ws::StreamsClient, client::system_proxy::SystemProxyClient};

/// Applies shared effects and delegates presentation items in the supplied plan order.
pub struct ApplicationEffectExecutor {
    system_proxy: SystemProxyClient,
    logger: Arc<dyn LoggerRefresher>,
    presentation: Option<Arc<dyn PresentationEffectsPort>>,
}

impl ApplicationEffectExecutor {
    /// Wires the shared owners and an optional presentation executor.
    pub fn new(
        system_proxy: SystemProxyClient,
        logger: Arc<dyn LoggerRefresher>,
        presentation: Option<Arc<dyn PresentationEffectsPort>>,
    ) -> Self {
        Self {
            system_proxy,
            logger,
            presentation,
        }
    }

    fn apply_logger(&self, revision: EffectRevision, desired: &LoggerDesired) -> EffectStatus {
        let refreshed = self.logger.refresh(
            Some(desired.level.clone()),
            Some(LogRotation {
                max_files: desired.max_files,
                max_file_size: desired.max_file_size,
            }),
        );
        match refreshed {
            Ok(()) => status::healthy(EffectKind::Logger, revision),
            // Retryable: the configuration is committed, so the next reconcile
            // hands the same desired value to the same adapter again.
            Err(error) => status::degraded(
                EffectKind::Logger,
                revision,
                error.code(),
                status::failure_text(&error),
                true,
            ),
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
        let (proxy, guard, auto_launch) = system_proxy_desires(&plan);
        // Filled on the first effect the system proxy actor owns. One round
        // trip, not three: all three belong to the same actor, and the guard's
        // timer depends on the proxy value applied beside it.
        let mut system_proxy_statuses: Option<Vec<EffectStatus>> = None;

        // Strictly in plan order, which is `EffectKind` order: the locale has
        // to be set before the tray menu is rebuilt from it, and the tray reads
        // the settled value of everything before it.
        let mut statuses = Vec::with_capacity(plan.effects().len());
        for effect in plan.effects() {
            let status = match effect {
                ApplicationEffect::Locale(_)
                | ApplicationEffect::Hotkeys(_)
                | ApplicationEffect::Widget(_)
                | ApplicationEffect::WidgetLogo(_)
                | ApplicationEffect::Tray(_, _) => {
                    if let Some(presentation) = &self.presentation {
                        statuses.extend(
                            presentation
                                .apply(
                                    revision,
                                    ApplicationEffectPlan::from_effects(vec![effect.clone()]),
                                )
                                .await,
                        );
                        continue;
                    }
                    EffectStatus {
                        kind: effect.kind(),
                        desired_revision: revision,
                        applied_revision: EffectRevision::default(),
                        health: EffectHealth::Unsupported {
                            code: EffectFailureCode::PresentationUnsupported,
                        },
                    }
                }
                ApplicationEffect::Logger(desired) => self.apply_logger(revision, desired),
                ApplicationEffect::CoreLogLevel(_)
                | ApplicationEffect::CoreLogStorage(_)
                | ApplicationEffect::TransparentProxy(_) => {
                    unreachable!("CoreRuntimeEffects applies core runtime effects")
                }
                ApplicationEffect::AutoLaunch(_)
                | ApplicationEffect::SystemProxy(_)
                | ApplicationEffect::ProxyGuard(_) => {
                    let owned = match system_proxy_statuses {
                        Some(ref owned) => owned,
                        None => system_proxy_statuses.insert(
                            self.system_proxy
                                .reconcile(revision, proxy.clone(), guard, auto_launch)
                                .await,
                        ),
                    };
                    let kind = effect.kind();
                    owned
                        .iter()
                        .find(|status| status.kind == kind)
                        .cloned()
                        .unwrap_or_else(|| {
                            status::degraded(
                                kind,
                                revision,
                                EffectFailureCode::EffectOwnerSilent,
                                format!("the owner of {kind:?} reported no status"),
                                true,
                            )
                        })
                }
            };
            statuses.push(status);
        }
        statuses
    }
}

/// Applies the Core log capture level and storage settings and hands every
/// other effect to the executor. The client spawns the Core log owners after
/// the composition root has built the executor, so the client wraps the
/// executor instead of joining it.
pub(crate) struct CoreRuntimeEffects {
    executor: Arc<dyn ApplicationEffectsPort>,
    streams: StreamsClient,
    storage: CoreLogsClient,
    core: crate::actor_v2::CoreClient,
}

impl CoreRuntimeEffects {
    pub(crate) fn new(
        executor: Arc<dyn ApplicationEffectsPort>,
        streams: StreamsClient,
        storage: CoreLogsClient,
        core: crate::actor_v2::CoreClient,
    ) -> Self {
        Self {
            executor,
            streams,
            storage,
            core,
        }
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for CoreRuntimeEffects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let mut level = None;
        let mut storage = None;
        let mut transparent = None;
        let mut others = Vec::new();
        for effect in plan.effects() {
            match effect {
                ApplicationEffect::CoreLogLevel(desired) => level = Some(*desired),
                ApplicationEffect::CoreLogStorage(desired) => storage = Some(*desired),
                ApplicationEffect::TransparentProxy(desired) => transparent = Some(desired),
                effect => others.push(effect.clone()),
            }
        }
        let mut statuses = if others.is_empty() {
            Vec::new()
        } else {
            self.executor
                .apply(revision, ApplicationEffectPlan::from_effects(others))
                .await
        };
        if let Some(level) = level {
            statuses.push(match self.streams.set_log_level(level).await {
                Ok(()) => status::healthy(EffectKind::CoreLogLevel, revision),
                Err(error) => status::degraded(
                    EffectKind::CoreLogLevel,
                    revision,
                    EffectFailureCode::EffectOwnerSilent,
                    format!("{error:#}"),
                    false,
                ),
            });
        }
        if let Some(settings) = storage {
            statuses.push(match self.storage.configure(settings).await {
                Ok(()) => status::healthy(EffectKind::CoreLogStorage, revision),
                // Not retryable: a failure stops storage until it is cleared or
                // a new Core session starts, and the store already holds the
                // committed settings those resets run under.
                Err(error) => status::degraded(
                    EffectKind::CoreLogStorage,
                    revision,
                    EffectFailureCode::CoreLogStorageFailed,
                    error.to_string(),
                    false,
                ),
            });
        }
        if let Some(desired) = transparent {
            statuses.push(
                match super::super::transparent_proxy::reconcile(&self.core, desired).await {
                    Ok(()) => status::healthy(EffectKind::TransparentProxy, revision),
                    Err(error) => status::degraded(
                        EffectKind::TransparentProxy,
                        revision,
                        EffectFailureCode::TransparentProxyFailed,
                        format!("{error:#}"),
                        true,
                    ),
                },
            );
        }
        statuses
    }
}

/// The three effects the system proxy actor owns, as one request.
fn system_proxy_desires(
    plan: &ApplicationEffectPlan,
) -> (
    Option<SystemProxyDesired>,
    Option<ProxyGuardDesired>,
    Option<bool>,
) {
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
    (proxy, guard, auto_launch)
}
