//! Dispatches each effect in plan order to its explicit owner.

use std::sync::Arc;

use super::{
    plan::{ApplicationEffect, ApplicationEffectPlan, LoggerDesired},
    ports::{ApplicationEffectsPort, PresentationEffectsPort, PresentationRequest},
};
use crate::{
    clash::ws::StreamsClient,
    effects::{
        EffectKind,
        status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus, failure_text},
    },
    logs::{
        CoreLogsClient,
        logging::{LogRotation, LoggerRefresher},
    },
    system_proxy::{ProxyGuardDesired, SystemProxyClient, SystemProxyDesired},
};

pub struct ApplicationEffectExecutor {
    system_proxy: SystemProxyClient,
    logger: Arc<dyn LoggerRefresher>,
    presentation: Option<Arc<dyn PresentationEffectsPort>>,
}

impl ApplicationEffectExecutor {
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
            Ok(()) => healthy(EffectKind::Logger, revision),
            // Retryable: the configuration is committed, so the next reconcile
            // hands the same desired value to the same adapter again.
            Err(error) => degraded(
                EffectKind::Logger,
                revision,
                EffectFailureCode::LoggerRefreshFailed,
                failure_text(&error),
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
                ApplicationEffect::Logger(desired) => self.apply_logger(revision, desired),
                ApplicationEffect::CoreLogLevel(_) | ApplicationEffect::CoreLogStorage(_) => {
                    unreachable!("CoreLogCaptureEffects applies the Core log effects")
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
                            degraded(
                                kind,
                                revision,
                                EffectFailureCode::EffectOwnerSilent,
                                format!("the owner of {kind:?} reported no status"),
                                true,
                            )
                        })
                }
                ApplicationEffect::Locale(_)
                | ApplicationEffect::Hotkeys(_)
                | ApplicationEffect::Widget(_)
                | ApplicationEffect::Tray(..) => {
                    let kind = effect.kind();
                    match &self.presentation {
                        Some(presentation) => {
                            let request = match effect {
                                ApplicationEffect::Locale(language) => {
                                    PresentationRequest::Locale(*language)
                                }
                                ApplicationEffect::Hotkeys(desired) => {
                                    PresentationRequest::Hotkeys(desired.clone())
                                }
                                ApplicationEffect::Widget(config) => {
                                    PresentationRequest::Widget(*config)
                                }
                                ApplicationEffect::Tray(refresh, application, clash) => {
                                    PresentationRequest::Tray {
                                        refresh: *refresh,
                                        application: application.clone(),
                                        clash: clash.clone(),
                                    }
                                }
                                _ => unreachable!("matched presentation effects"),
                            };
                            presentation.apply(revision, request).await
                        }
                        None => EffectStatus {
                            kind,
                            desired_revision: revision,
                            applied_revision: EffectRevision::default(),
                            health: EffectHealth::Unsupported {
                                code: EffectFailureCode::PresentationUnsupported,
                            },
                        },
                    }
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
pub struct CoreLogCaptureEffects {
    executor: Arc<dyn ApplicationEffectsPort>,
    streams: StreamsClient,
    storage: CoreLogsClient,
}

impl CoreLogCaptureEffects {
    pub fn new(
        executor: Arc<dyn ApplicationEffectsPort>,
        streams: StreamsClient,
        storage: CoreLogsClient,
    ) -> Self {
        Self {
            executor,
            streams,
            storage,
        }
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for CoreLogCaptureEffects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let mut level = None;
        let mut storage = None;
        let mut others = Vec::new();
        for effect in plan.effects() {
            match effect {
                ApplicationEffect::CoreLogLevel(desired) => level = Some(*desired),
                ApplicationEffect::CoreLogStorage(desired) => storage = Some(*desired),
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
                Ok(()) => healthy(EffectKind::CoreLogLevel, revision),
                Err(error) => degraded(
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
                Ok(()) => healthy(EffectKind::CoreLogStorage, revision),
                // Not retryable: a failure stops storage until it is cleared or
                // a new Core session starts, and the store already holds the
                // committed settings those resets run under.
                Err(error) => degraded(
                    EffectKind::CoreLogStorage,
                    revision,
                    EffectFailureCode::CoreLogStorageFailed,
                    error.to_string(),
                    false,
                ),
            });
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

fn healthy(kind: EffectKind, revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: revision,
        health: EffectHealth::Healthy,
    }
}

fn degraded(
    kind: EffectKind,
    revision: EffectRevision,
    code: EffectFailureCode,
    message: String,
    retryable: bool,
) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: EffectRevision::default(),
        health: EffectHealth::Degraded {
            code,
            message,
            retryable,
        },
    }
}
