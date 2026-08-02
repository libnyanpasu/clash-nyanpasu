mod application;
mod clash_config;
mod core;
mod core_bridge;
mod error;
mod event_sink;
mod ports;
#[cfg(test)]
mod process_core_bridge;
pub mod profiles;
pub mod rebuild;
pub mod runtime;
mod session_state;
mod system_dns;

pub(crate) use self::application::ApplicationClient;
use self::{clash_config::ClashConfigClient, session_state::SessionStateClient};
use crate::{
    core::actor::runtime as core_runtime,
    enhance::{
        EnhanceScriptRunner, FsProfileContentSource, RuntimeBuildInput, RuntimeBuilder,
        runtime_snapshot_data_from_artifact,
    },
    service::profile_file::{ProfileFileService, SelfProxyPortSource},
    state::{
        ConditionalReplaceResult, TypedConfigPatchPlan,
        application::ApplicationSnapshot,
        clash_config::ClashConfigSnapshot,
        mirror::{
            ClashLegacyBridge as ClashLegacyBridgeTrait, PreparedTypedReplace,
            VergeLegacyBridge as VergeLegacyBridgeTrait,
            WindowLegacyBridge as WindowLegacyBridgeTrait,
        },
        profiles::{
            CommitReport, NewProfileRequest, ProfilesError, ReorderOp,
            ports::{ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher},
        },
        session_state::SessionStateSnapshot,
    },
    utils::path::PathResolver,
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch},
    profile::{
        LocalBinding, ProfileDefinition, ProfileId, ProfileMetadata, ProfileMetadataPatch,
        ProfileSource, Profiles, RemoteProfileOptions, RemoteProfileOptionsPatch,
    },
    runtime::executor::ResolvedPortBindings,
    state::{PersistentState, PersistentStatePatch},
};
use std::{path::PathBuf, sync::Arc};
use struct_patch::Patch as _;

#[cfg(test)]
pub(crate) use core::CoreClientArgs;
#[allow(unused_imports)]
pub use core::{CoreClient, CoreOperationGuard};
pub use core_bridge::{CoreLifecycleLease, CoreLifecyclePort};
pub use error::{ClientError, Result};
pub(crate) use error::{CompensationFailure, LegacyVergeDomain, PartialCommit};
#[cfg(test)]
pub use event_sink::NoopUiEventSink;
pub use event_sink::{TauriCoreDegradationSink, TauriUiEventSink, UiEventSink};
pub use ports::SessionPortResolver;
pub use runtime::RuntimePaths;
#[cfg(test)]
pub use system_dns::{MockSystemDnsCache, NoopSystemDnsCache};
pub use system_dns::{OsSystemDnsCache, SystemDnsCache};
#[cfg(test)]
pub use tests::{MockRunningCoreBridge, TestRunningCoreBridge as RunningCoreBridge};
#[cfg(test)]
pub(crate) use tests::{test_binary_resolver, test_degradation_sink, test_service_control};

pub struct ClientSetupArgs {
    pub paths: PathResolver,
    pub runtime_paths: RuntimePaths,
    pub bridges: LegacyBridgeSet,
    pub ui_sink: Arc<dyn UiEventSink>,
    pub core: Option<Arc<dyn CoreLifecyclePort>>,
    pub binary_resolver: Arc<dyn crate::core::actor::request::CoreBinaryResolver>,
    pub degradation: Arc<dyn crate::core::actor::backend::CoreDegradationSink>,
    pub(crate) service_control: Arc<dyn crate::core::actor::backend::ServiceControlOps>,
    pub system_dns: Arc<dyn SystemDnsCache>,
}

#[derive(Clone)]
pub struct LegacyBridgeSet {
    pub verge: Arc<dyn VergeLegacyBridgeTrait>,
    pub window: Arc<dyn WindowLegacyBridgeTrait>,
    pub clash: Arc<dyn ClashLegacyBridgeTrait>,
}

#[derive(Clone)]
pub struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

pub(crate) struct TypedConfigSnapshots {
    pub application: ApplicationSnapshot,
    pub session: SessionStateSnapshot,
    pub clash: ClashConfigSnapshot,
}

enum PreparedConfigDomain {
    Application {
        expected_version: u64,
        forward: PreparedTypedReplace<NyanpasuAppConfig>,
        rollback: Box<PreparedTypedReplace<NyanpasuAppConfig>>,
    },
    Session {
        expected_version: u64,
        forward: PreparedTypedReplace<PersistentState>,
        rollback: PreparedTypedReplace<PersistentState>,
    },
    Clash {
        expected_version: u64,
        forward: PreparedTypedReplace<ClashConfig>,
        rollback: PreparedTypedReplace<ClashConfig>,
    },
}

#[derive(Debug, thiserror::Error)]
enum RuntimeRebuildError {
    #[error(transparent)]
    Build(ClientError),
    #[error(transparent)]
    CheckAndPromote(core_bridge::CheckAndPromoteFailure),
    #[error(transparent)]
    Publish(crate::core::actor::types::CoreActorError),
}

impl From<RuntimeRebuildError> for ClientError {
    fn from(error: RuntimeRebuildError) -> Self {
        Self::Anyhow(anyhow::Error::new(error))
    }
}

enum CommittedConfigDomain {
    Application {
        committed_version: u64,
        rollback: PreparedTypedReplace<NyanpasuAppConfig>,
    },
    Session {
        committed_version: u64,
        rollback: PreparedTypedReplace<PersistentState>,
    },
    Clash {
        committed_version: u64,
        rollback: PreparedTypedReplace<ClashConfig>,
    },
}

async fn new_typed_config_clients(
    paths: PathResolver,
    bridges: LegacyBridgeSet,
) -> anyhow::Result<(ApplicationClient, SessionStateClient, ClashConfigClient)> {
    let application = ApplicationClient::new(
        utf8_path(paths.application_config_path())?,
        bridges.verge.snapshot_legacy()?,
        bridges.verge.clone(),
    )
    .await?;

    let session_state = SessionStateClient::new(
        utf8_path(paths.session_state_path())?,
        bridges.window.snapshot_legacy()?,
        bridges.window.clone(),
    )
    .await?;

    let clash_config = ClashConfigClient::new(
        utf8_path(paths.clash_config_path())?,
        bridges.clash.snapshot_legacy()?,
        bridges.clash.clone(),
    )
    .await?;

    sync_legacy_mirrors(&application, &session_state, &clash_config, &bridges).await?;
    Ok((application, session_state, clash_config))
}

async fn sync_legacy_mirrors(
    application: &ApplicationClient,
    session_state: &SessionStateClient,
    clash_config: &ClashConfigClient,
    bridges: &LegacyBridgeSet,
) -> anyhow::Result<()> {
    let application = application
        .get()
        .await
        .context("failed to read loaded application config")?
        .state;
    bridges
        .verge
        .prepare(&application)
        .context("failed to prepare loaded application config legacy mirror")?
        .apply();

    let session_state = session_state
        .get()
        .await
        .context("failed to read loaded session state")?
        .state;
    bridges
        .window
        .prepare(&session_state)
        .context("failed to prepare loaded session state legacy mirror")?
        .apply();

    let clash_config = clash_config
        .get()
        .await
        .context("failed to read loaded clash config")?
        .state;
    bridges
        .clash
        .prepare(&clash_config)
        .context("failed to prepare loaded clash config legacy mirror")?
        .apply();

    Ok(())
}

/// Fallback name for an imported subscription with no caller-provided name:
/// the url's last non-empty path segment (sans `.yaml`/`.yml`), else the host,
/// else a constant. Kept separate so `import_profile` reads as orchestration.
fn url_derived_name(url: &url::Url) -> String {
    url.path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| {
            segment
                .trim_end_matches(".yaml")
                .trim_end_matches(".yml")
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .or_else(|| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "Remote Profile".into())
}

struct NyanpasuClientInner {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    profiles: profiles::ProfilesClient,
    fs: Arc<dyn ProfileFsPort>,
    ports: Arc<SessionPortResolver>,
    profiles_dir: PathBuf,
    runtime_paths: RuntimePaths,
    ui_sink: Arc<dyn UiEventSink>,
    core_client: CoreClient,
    core: Arc<dyn CoreLifecyclePort>,
    requests: crate::core::actor::request::CoreRequestFactory,
    service_control: Arc<dyn crate::core::actor::backend::ServiceControlOps>,
    degradation: Arc<dyn crate::core::actor::backend::CoreDegradationSink>,
    system_dns: Arc<dyn SystemDnsCache>,
    /// Instance-owned background dirty coordinator (capacity-1 coalesce).
    /// Request/reply regeneration calls typed facade methods directly.
    rebuild: rebuild::RebuildCoordinator,
    runtime_revisions: runtime::RuntimeRevisionAllocator,
}

#[allow(dead_code)]
impl NyanpasuClient {
    pub fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            paths,
            runtime_paths,
            bridges,
            ui_sink,
            core,
            binary_resolver,
            degradation,
            service_control,
            system_dns,
        } = args;
        let client_degradation = degradation.clone();
        let profiles_dir = paths.app_profiles_dir();
        let profiles_path = utf8_path(paths.profiles_path())?;
        let runtime_paths_for_setup = runtime_paths.clone();
        let (
            application,
            session_state,
            clash_config,
            profiles,
            ports,
            fs,
            rebuild,
            core_client,
            core,
            requests,
        ) = tauri::async_runtime::block_on(async move {
            runtime_paths_for_setup
                .cleanup_stale_candidates(std::time::Duration::from_secs(24 * 60 * 60))
                .await
                .context("failed to clean stale runtime candidates")?;
            let (application, session_state, clash_config) =
                new_typed_config_clients(paths.clone(), bridges).await?;
            let app = application.get().await?.state;
            let mode = crate::core::RunType::classify(
                app.enable_service_mode,
                crate::core::service::ipc::get_ipc_state(),
            );
            let requests = crate::core::actor::request::CoreRequestFactory::new(
                &paths,
                runtime_paths_for_setup.clone(),
                binary_resolver,
            )?;
            let core_client = CoreClient::new(core::CoreClientArgs {
                mode,
                requests: requests.clone(),
                degradation,
            })
            .await?;
            let core = match core {
                Some(core) => {
                    #[cfg(test)]
                    {
                        Arc::new(core_bridge::ActorBackedTestCoreLifecyclePort::new(
                            core,
                            core_client.clone(),
                        )) as Arc<dyn CoreLifecyclePort>
                    }
                    #[cfg(not(test))]
                    {
                        core
                    }
                }
                None => Arc::new(core::CoreLifecycleAdapter::new(
                    core_client.clone(),
                    application.clone(),
                    requests.clone(),
                )) as Arc<dyn CoreLifecyclePort>,
            };

            // Eager session port resolution: the core is not running yet,
            // so probing strategies is race-free (design §19.2 caller duty).
            let ports = Arc::new(SessionPortResolver::default());
            let clash_snapshot = clash_config.get().await?.state;
            ports
                .resolve(&clash_snapshot)
                .context("failed to resolve session ports")?;

            let file_service = Arc::new(ProfileFileService::new(
                paths,
                ports.clone() as Arc<dyn SelfProxyPortSource>,
            ));
            let rebuild = rebuild::RebuildCoordinator::new();
            let profiles = profiles::ProfilesClient::new(
                profiles_path,
                file_service.clone() as Arc<dyn ProfileFsPort>,
                file_service.clone() as Arc<dyn SubscriptionFetcher>,
                file_service.clone() as Arc<dyn ProfileMaterializationPort>,
                Arc::new(rebuild.notifier()),
            )
            .await?;
            anyhow::Ok((
                application,
                session_state,
                clash_config,
                profiles,
                ports,
                file_service as Arc<dyn ProfileFsPort>,
                rebuild,
                core_client,
                core,
                requests,
            ))
        })?;
        let client = Self::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            fs,
            ports,
            profiles_dir,
            runtime_paths,
            ui_sink,
            core_client,
            core,
            requests,
            service_control,
            client_degradation,
            system_dns,
            rebuild,
        );
        client.start_rebuild_worker();
        Ok(client)
    }

    #[allow(dead_code, clippy::too_many_arguments)]
    fn with_parts(
        application: ApplicationClient,
        session_state: SessionStateClient,
        clash_config: ClashConfigClient,
        profiles: profiles::ProfilesClient,
        fs: Arc<dyn ProfileFsPort>,
        ports: Arc<SessionPortResolver>,
        profiles_dir: PathBuf,
        runtime_paths: RuntimePaths,
        ui_sink: Arc<dyn UiEventSink>,
        core_client: CoreClient,
        core: Arc<dyn CoreLifecyclePort>,
        requests: crate::core::actor::request::CoreRequestFactory,
        service_control: Arc<dyn crate::core::actor::backend::ServiceControlOps>,
        degradation: Arc<dyn crate::core::actor::backend::CoreDegradationSink>,
        system_dns: Arc<dyn SystemDnsCache>,
        rebuild: rebuild::RebuildCoordinator,
    ) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner {
                application,
                session_state,
                clash_config,
                profiles,
                fs,
                ports,
                profiles_dir,
                runtime_paths,
                ui_sink,
                core_client,
                core,
                requests,
                service_control,
                degradation,
                system_dns,
                rebuild,
                runtime_revisions: runtime::RuntimeRevisionAllocator::new(),
            }),
        }
    }

    /// Start the capacity-1 dirty worker. The worker upgrades a `Weak` client
    /// graph so shutdown/drop cannot form an Arc cycle.
    fn start_rebuild_worker(&self) {
        let weak = Arc::downgrade(&self.inner);
        self.inner.rebuild.start_worker(move || {
            let weak = weak.clone();
            async move {
                let Some(inner) = weak.upgrade() else {
                    return Ok(());
                };
                NyanpasuClient { inner }
                    .rebuild_running_config_in_background()
                    .await
                    .map_err(anyhow::Error::from)
            }
        });
    }

    /// Stop the instance-owned rebuild worker and core actor, awaiting both exits.
    ///
    /// Contract (PR-5a S11):
    /// - Shuts down the capacity-1 dirty rebuild worker before the core actor/backend.
    /// - Does **not** act as a general service locator teardown.
    /// - Does **not** stop desired-state actors, system proxy, or unrelated OS resources.
    /// - Safe to call multiple times; post-shutdown dirty notifications are no-ops.
    /// - An already in-flight rebuild is allowed to finish; coalesce waits abort.
    pub async fn shutdown(&self) {
        self.inner.rebuild.shutdown().await;
        self.inner.core_client.shutdown().await;
    }

    fn core_mode_reconciler(&self) -> crate::core::actor::request::CoreModeReconciler {
        crate::core::actor::request::CoreModeReconciler {
            core: self.inner.core_client.clone(),
            application: self.inner.application.clone(),
            requests: self.inner.requests.clone(),
        }
    }

    pub fn core_status(
        &self,
    ) -> (
        std::borrow::Cow<'static, nyanpasu_ipc::api::status::CoreState>,
        i64,
        crate::core::RunType,
    ) {
        let status = self.inner.core_client.status();
        self.inner.core_client.hint_refresh();
        (
            std::borrow::Cow::Owned(status.state),
            status.state_changed_at,
            status.run_type,
        )
    }

    pub async fn restart_core(&self) -> Result<()> {
        let result: anyhow::Result<()> = async {
            let app = self.inner.application.get().await?.state;
            let operation = self.inner.core_client.begin_operation().await?;
            let request = self.inner.requests.for_product(app.core)?;
            self.inner.core_client.run(&operation, &request).await?;
            Ok(())
        }
        .await;
        result?;
        Ok(())
    }

    pub async fn install_service(&self) -> Result<()> {
        self.inner
            .service_control
            .install(self.core_mode_reconciler())
            .await?;
        Ok(())
    }

    pub async fn start_service(&self) -> Result<()> {
        let control = self
            .inner
            .service_control
            .start(self.core_mode_reconciler())
            .await;
        self.reconcile_service_mode().await;
        control?;
        Ok(())
    }

    pub async fn stop_service(&self) -> Result<()> {
        let control = self.inner.service_control.stop().await;
        self.reconcile_service_mode().await;
        control?;
        Ok(())
    }

    pub async fn restart_service(&self) -> Result<()> {
        let control = self
            .inner
            .service_control
            .restart(self.core_mode_reconciler())
            .await;
        self.reconcile_service_mode().await;
        control?;
        Ok(())
    }

    async fn reconcile_service_mode(&self) {
        if let Err(error) = self
            .core_mode_reconciler()
            .reconcile(crate::core::service::ipc::get_ipc_state())
            .await
        {
            log::error!(target: "app", "{error}");
        }
    }

    pub async fn init_service_health(&self) -> Result<()> {
        let app = self.inner.application.get().await?.state;
        crate::utils::init::init_service(app.enable_service_mode, self.core_mode_reconciler())
            .await?;
        Ok(())
    }

    pub async fn update_core(
        &self,
        core_type: crate::config::nyanpasu::ClashCore,
    ) -> Result<usize> {
        Ok(crate::core::updater::UpdaterManager::global()
            .write()
            .await
            .update_core(&core_type, self.inner.core_client.clone())
            .await?)
    }

    pub(crate) fn runtime_paths(&self) -> &RuntimePaths {
        &self.inner.runtime_paths
    }

    #[cfg(test)]
    pub(crate) fn rebuild_coordinator(&self) -> &rebuild::RebuildCoordinator {
        &self.inner.rebuild
    }

    pub async fn get_app_config(&self) -> Result<NyanpasuAppConfig> {
        let client = self.inner.application.clone();
        Ok(client.get().await?.state)
    }

    pub async fn flush_system_dns_cache(&self) -> Result<()> {
        let system_dns = self.inner.system_dns.clone();
        tokio::task::spawn_blocking(move || system_dns.flush())
            .await
            .context("system DNS cache flush task failed")??;
        Ok(())
    }

    pub async fn patch_app_config(&self, patch: NyanpasuAppConfigPatch) -> Result<()> {
        let client = self.inner.application.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_app_config(&self, state: NyanpasuAppConfig) -> Result<()> {
        let client = self.inner.application.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub async fn get_session_state(&self) -> Result<PersistentState> {
        let client = self.inner.session_state.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_session_state(&self, patch: PersistentStatePatch) -> Result<()> {
        let client = self.inner.session_state.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_session_state(&self, state: PersistentState) -> Result<()> {
        let client = self.inner.session_state.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub async fn get_clash_config(&self) -> Result<ClashConfig> {
        let client = self.inner.clash_config.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_clash_config(&self, patch: ClashConfigPatch) -> Result<()> {
        let client = self.inner.clash_config.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_clash_config(&self, state: ClashConfig) -> Result<()> {
        let client = self.inner.clash_config.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub(crate) async fn typed_config_snapshots(&self) -> Result<TypedConfigSnapshots> {
        Ok(TypedConfigSnapshots {
            application: self.inner.application.get().await?,
            session: self.inner.session_state.get().await?,
            clash: self.inner.clash_config.get().await?,
        })
    }

    pub(crate) async fn apply_legacy_verge_patch_saga<F>(
        &self,
        plan: TypedConfigPatchPlan,
        finalize: F,
    ) -> Result<()>
    where
        F: FnOnce() -> anyhow::Result<()>,
    {
        let snapshots = self.typed_config_snapshots().await?;
        let application = plan.application.map(|patch| {
            let mut state = snapshots.application.state.clone();
            state.apply(patch);
            state
        });
        let session = plan.session_state.map(|patch| {
            let mut state = snapshots.session.state.clone();
            state.apply(patch);
            state
        });
        let clash = plan.clash_config.map(|patch| {
            let mut state = snapshots.clash.state.clone();
            state.apply(patch);
            state
        });
        self.apply_legacy_verge_states_saga(snapshots, application, session, clash, finalize)
            .await
    }

    pub(crate) async fn apply_legacy_verge_replacement_saga<F>(
        &self,
        application: NyanpasuAppConfig,
        session: PersistentState,
        clash: ClashConfig,
        finalize: F,
    ) -> Result<()>
    where
        F: FnOnce() -> anyhow::Result<()>,
    {
        let snapshots = self.typed_config_snapshots().await?;
        self.apply_legacy_verge_states_saga(
            snapshots,
            Some(application),
            Some(session),
            Some(clash),
            finalize,
        )
        .await
    }

    async fn apply_legacy_verge_states_saga<F>(
        &self,
        snapshots: TypedConfigSnapshots,
        application: Option<NyanpasuAppConfig>,
        session: Option<PersistentState>,
        clash: Option<ClashConfig>,
        finalize: F,
    ) -> Result<()>
    where
        F: FnOnce() -> anyhow::Result<()>,
    {
        let mut prepared = Vec::new();
        if let Some(state) = application {
            prepared.push(PreparedConfigDomain::Application {
                expected_version: snapshots.application.version,
                forward: self.inner.application.prepare_replace(state).await?,
                rollback: Box::new(
                    self.inner
                        .application
                        .prepare_replace(snapshots.application.state.clone())
                        .await?,
                ),
            });
        }
        if let Some(state) = session {
            prepared.push(PreparedConfigDomain::Session {
                expected_version: snapshots.session.version,
                forward: self.inner.session_state.prepare_replace(state).await?,
                rollback: self
                    .inner
                    .session_state
                    .prepare_replace(snapshots.session.state.clone())
                    .await?,
            });
        }
        if let Some(state) = clash {
            prepared.push(PreparedConfigDomain::Clash {
                expected_version: snapshots.clash.version,
                forward: self.inner.clash_config.prepare_replace(state).await?,
                rollback: self
                    .inner
                    .clash_config
                    .prepare_replace(snapshots.clash.state.clone())
                    .await?,
            });
        }

        let mut committed = Vec::new();
        for domain in prepared {
            let result = match domain {
                PreparedConfigDomain::Application {
                    expected_version,
                    forward,
                    rollback,
                } => match self
                    .inner
                    .application
                    .replace_prepared_if_version(expected_version, forward)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                        committed.push(CommittedConfigDomain::Application {
                            committed_version: snapshot.version,
                            rollback: *rollback,
                        });
                        continue;
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        ClientError::Custom(format!(
                            "application config version conflict: expected {expected_version}, actual {actual_version}"
                        ))
                    }
                    Err(error) => ClientError::Anyhow(
                        error.context("failed to commit application config in legacy verge saga"),
                    ),
                },
                PreparedConfigDomain::Session {
                    expected_version,
                    forward,
                    rollback,
                } => match self
                    .inner
                    .session_state
                    .replace_prepared_if_version(expected_version, forward)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                        committed.push(CommittedConfigDomain::Session {
                            committed_version: snapshot.version,
                            rollback,
                        });
                        continue;
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        ClientError::Custom(format!(
                            "session config version conflict: expected {expected_version}, actual {actual_version}"
                        ))
                    }
                    Err(error) => ClientError::Anyhow(
                        error.context("failed to commit session state in legacy verge saga"),
                    ),
                },
                PreparedConfigDomain::Clash {
                    expected_version,
                    forward,
                    rollback,
                } => match self
                    .inner
                    .clash_config
                    .replace_prepared_if_version(expected_version, forward)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                        committed.push(CommittedConfigDomain::Clash {
                            committed_version: snapshot.version,
                            rollback,
                        });
                        continue;
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        ClientError::Custom(format!(
                            "clash config version conflict: expected {expected_version}, actual {actual_version}"
                        ))
                    }
                    Err(error) => ClientError::Anyhow(
                        error.context("failed to commit clash config in legacy verge saga"),
                    ),
                },
            };
            return self
                .compensate_legacy_verge_saga(committed, result, Vec::new())
                .await;
        }

        if let Err(error) = finalize() {
            let legacy_uncertainty = CompensationFailure::LegacyStateUncertain {
                message: format!("{error:#}"),
            };
            return self
                .compensate_legacy_verge_saga(
                    committed,
                    ClientError::Anyhow(
                        error.context("failed to finalize legacy verge persistence"),
                    ),
                    vec![legacy_uncertainty],
                )
                .await;
        }

        Ok(())
    }

    async fn compensate_legacy_verge_saga(
        &self,
        mut committed: Vec<CommittedConfigDomain>,
        primary: ClientError,
        mut failed_compensations: Vec<CompensationFailure>,
    ) -> Result<()> {
        let committed_domains = committed
            .iter()
            .map(|domain| match domain {
                CommittedConfigDomain::Application { .. } => LegacyVergeDomain::Application,
                CommittedConfigDomain::Session { .. } => LegacyVergeDomain::Session,
                CommittedConfigDomain::Clash { .. } => LegacyVergeDomain::Clash,
            })
            .collect::<Vec<_>>();
        let mut compensated_domains = Vec::new();

        while let Some(domain) = committed.pop() {
            match domain {
                CommittedConfigDomain::Application {
                    committed_version,
                    rollback,
                } => match self
                    .inner
                    .application
                    .replace_prepared_if_version(committed_version, rollback)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(_)) => {
                        compensated_domains.push(LegacyVergeDomain::Application)
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        failed_compensations.push(CompensationFailure::Conflict {
                            domain: LegacyVergeDomain::Application,
                            expected_version: committed_version,
                            actual_version,
                        });
                    }
                    Err(error) => failed_compensations.push(CompensationFailure::Error {
                        domain: LegacyVergeDomain::Application,
                        message: format!("{error:#}"),
                    }),
                },
                CommittedConfigDomain::Session {
                    committed_version,
                    rollback,
                } => match self
                    .inner
                    .session_state
                    .replace_prepared_if_version(committed_version, rollback)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(_)) => {
                        compensated_domains.push(LegacyVergeDomain::Session)
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        failed_compensations.push(CompensationFailure::Conflict {
                            domain: LegacyVergeDomain::Session,
                            expected_version: committed_version,
                            actual_version,
                        });
                    }
                    Err(error) => failed_compensations.push(CompensationFailure::Error {
                        domain: LegacyVergeDomain::Session,
                        message: format!("{error:#}"),
                    }),
                },
                CommittedConfigDomain::Clash {
                    committed_version,
                    rollback,
                } => match self
                    .inner
                    .clash_config
                    .replace_prepared_if_version(committed_version, rollback)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(_)) => {
                        compensated_domains.push(LegacyVergeDomain::Clash)
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        failed_compensations.push(CompensationFailure::Conflict {
                            domain: LegacyVergeDomain::Clash,
                            expected_version: committed_version,
                            actual_version,
                        });
                    }
                    Err(error) => failed_compensations.push(CompensationFailure::Error {
                        domain: LegacyVergeDomain::Clash,
                        message: format!("{error:#}"),
                    }),
                },
            }
        }

        if failed_compensations.is_empty() {
            return Err(primary);
        }

        let partial = PartialCommit::new(
            &primary,
            committed_domains,
            compensated_domains,
            failed_compensations,
        );
        tracing::error!(partial_commit = ?partial, "legacy verge saga requires reconciliation");
        self.inner.ui_sink.refresh_verge();
        self.inner.ui_sink.refresh_clash();
        Err(partial.into())
    }

    // ---- profiles domain (PR-3 T07) ----

    pub async fn get_profiles(&self) -> Result<Arc<Profiles>> {
        Ok(self.inner.profiles.get().await?)
    }

    /// Map crate-internal profile materialization degradations onto the public
    /// wire. Actor-internal Cleanup/Reconcile phases collapse to
    /// `ProfileMaterialization`; retryability stays code-derived.
    fn map_profile_degradation(
        degradation: &crate::state::profiles::ports::ProfileDegradation,
    ) -> runtime::Degradation {
        use crate::state::profiles::ports::ProfileDegradationCode;

        let code = match degradation.code {
            ProfileDegradationCode::JournalInvalid => "journal_invalid",
            ProfileDegradationCode::MaterializationDeferred => "materialization_deferred",
            ProfileDegradationCode::CleanupDeferred => "cleanup_deferred",
        };
        runtime::Degradation {
            phase: runtime::DegradationPhase::ProfileMaterialization,
            code: code.into(),
            message: degradation.message.clone(),
            retryable: degradation.code.retryable(),
        }
    }

    async fn collect_post_commit_degradations(
        &self,
        report: &CommitReport,
    ) -> Result<Vec<runtime::Degradation>> {
        // Post-commit side-effect failures are degraded results, not transaction
        // failures (T04 contract): state is already persisted, so surface them.
        let mut degradations: Vec<runtime::Degradation> = report
            .degradations
            .iter()
            .map(|degradation| {
                tracing::warn!(
                    phase = ?degradation.phase,
                    code = ?degradation.code,
                    retryable = degradation.code.retryable(),
                    message = %degradation.message,
                    "profile commit completed with a degraded side effect",
                );
                Self::map_profile_degradation(degradation)
            })
            .collect();

        if report.affects_current {
            let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
            let (result, mut runtime_degradations) =
                self.rebuild_pipeline_with_lease(&mut *lease).await;
            result?;
            if runtime_degradations.is_empty() {
                drop(lease);
                self.inner.ui_sink.refresh_clash();
                self.inner.core.on_profile_change().await;
            }
            for degradation in &runtime_degradations {
                tracing::warn!(
                    phase = ?degradation.phase,
                    code = %degradation.code,
                    retryable = degradation.retryable,
                    message = %degradation.message,
                    "post-commit rebuild failed; state stays committed (degraded)",
                );
            }
            degradations.append(&mut runtime_degradations);
        }
        Ok(degradations)
    }

    async fn after_commit(&self, report: &CommitReport) -> Result<runtime::MutationOutcome<()>> {
        Ok(runtime::MutationOutcome::from_parts(
            (),
            self.collect_post_commit_degradations(report).await?,
        ))
    }

    /// Public wire for a post-commit auto-activation hard failure. Create/import
    /// already committed the profile, so this must never become `Err` that erases
    /// the `ProfileId`. VersionConflict is not special-cased as success.
    fn auto_activation_failure_degradation(error: &ProfilesError) -> runtime::Degradation {
        tracing::warn!(
            %error,
            "profile auto-activation failed after commit; retaining committed profile id",
        );
        runtime::Degradation {
            phase: runtime::DegradationPhase::SystemEffect,
            code: "profile_auto_activation_failed".into(),
            message: error.to_string(),
            // Activation can be retried via activate_profile / set_current; even
            // VersionConflict is a transient CAS race, not a permanent rejection.
            retryable: true,
        }
    }

    /// Shared create/import post-commit auto-activation protocol:
    /// - `Ok(Some(report))` → merge report (and rebuild) degradations
    /// - `Ok(None)` → existing current won; no degradation
    /// - `Err(_)` → committed degradation, profile id retained by the caller
    async fn try_auto_activate_if_none(&self, uid: ProfileId) -> Result<Vec<runtime::Degradation>> {
        match self.inner.profiles.set_current_if_none(uid).await {
            Ok(Some(report)) => self.collect_post_commit_degradations(&report).await,
            Ok(None) => Ok(Vec::new()),
            Err(error) => Ok(vec![Self::auto_activation_failure_degradation(&error)]),
        }
    }

    /// Public facade entry for durable profile adds. Rejects remote definitions
    /// here so callers cannot stage an empty remote shell and bypass the
    /// fetch-before-commit import path. `ProfilesClient::add` stays available for
    /// crate-internal actor tests and legacy internals.
    pub async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<runtime::MutationOutcome<ProfileId>> {
        // Create/add do not download: a remote source would be committed
        // unmaterialized (and auto-activation would rebuild against a missing
        // file). Remote subscriptions must use import_profile.
        if matches!(request.definition.source(), Some(source) if source.is_remote()) {
            return Err(ClientError::Custom(
                "remote profiles must be created via import_profile".into(),
            ));
        }
        let report = self.inner.profiles.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("add committed without a created uid".into()))?;
        Ok(runtime::MutationOutcome::from_parts(
            created,
            self.collect_post_commit_degradations(&report).await?,
        ))
    }

    /// Create a profile from a fully-specified request and apply the design §9
    /// auto-activation rule (activate a new Config profile when nothing is
    /// current). Keeps the auto-activation policy in the facade so the command
    /// stays a thin adapter. Remote rejection is owned by [`Self::add_profile`].
    pub async fn create_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<runtime::MutationOutcome<ProfileId>> {
        // Kind is fixed by the request; avoid a post-commit get() that could turn
        // a successful add into a hard error and erase the committed ProfileId.
        let is_config = matches!(request.definition, ProfileDefinition::Config { .. });
        let mut outcome = self.add_profile(request, initial_file).await?;
        // design §9: auto-activate a Config definition (File/Composition) when
        // nothing is currently selected. set_current_if_none keeps the
        // check-and-set atomic so a concurrent selection is not overwritten.
        if is_config {
            let uid = outcome.value().clone();
            outcome = outcome.extend_degradations(self.try_auto_activate_if_none(uid).await?);
        }
        Ok(outcome)
    }

    /// Import a remote subscription via actor-owned fetch-before-commit, then
    /// auto-activate when nothing is current.
    ///
    /// Naming: a non-empty caller-provided `name` (e.g. a deep-link `name=`
    /// parameter) is user intent, so it is pinned (`custom_name = true`) and
    /// never overwritten by later name-sync. Without one, the name is derived
    /// from the url and left unpinned so the first import can adopt the
    /// subscription's `profile-title` / `Content-Disposition` name.
    ///
    /// No durable placeholder/profile document/file is written until fetch and
    /// validation succeed. Caller cancellation before durable commit begins
    /// discards the download; a complete valid profile may remain only if
    /// cancellation races after commit has already started.
    pub async fn import_profile(
        &self,
        url: url::Url,
        name: Option<String>,
        options: Option<RemoteProfileOptionsPatch>,
    ) -> Result<runtime::MutationOutcome<ProfileId>> {
        let update_interval_explicit = options
            .as_ref()
            .and_then(|patch| patch.update_interval_minutes)
            .is_some();
        let (name, custom_name) = match name {
            Some(name) if !name.trim().is_empty() => (name, true),
            _ => (url_derived_name(&url), false),
        };
        let mut option = RemoteProfileOptions::default();
        if let Some(patch) = options {
            option.apply(patch);
        }
        let report = self
            .inner
            .profiles
            .import(
                url,
                ProfileMetadata {
                    name,
                    desc: None,
                    custom_name,
                },
                option,
                update_interval_explicit,
            )
            .await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("import committed without a created uid".into()))?;
        let mut degradations = self.collect_post_commit_degradations(&report).await?;
        // Atomically activate only when nothing was selected during the download
        // window. Failures degrade; they must not erase the committed ProfileId.
        degradations.extend(self.try_auto_activate_if_none(created.clone()).await?);
        Ok(runtime::MutationOutcome::from_parts(created, degradations))
    }

    pub async fn delete_profile(&self, uid: ProfileId) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.delete(uid).await?;
        self.after_commit(&report).await
    }

    pub async fn reorder_profile(
        &self,
        active: ProfileId,
        over: ProfileId,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self
            .inner
            .profiles
            .reorder(ReorderOp::Move { active, over })
            .await?;
        self.after_commit(&report).await
    }

    pub async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileId>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.reorder(ReorderOp::ByList(list)).await?;
        self.after_commit(&report).await
    }

    pub async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.refresh(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.patch_metadata(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.patch_remote_options(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn replace_profile_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self
            .inner
            .profiles
            .replace_definition(uid, definition)
            .await?;
        self.after_commit(&report).await
    }

    pub async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.set_current(uid).await?;
        self.after_commit(&report).await
    }

    pub async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.set_global_transforms(ids).await?;
        self.after_commit(&report).await
    }

    pub async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.set_valid_fields(fields).await?;
        self.after_commit(&report).await
    }

    pub async fn get_profile_materialized_path(&self, uid: ProfileId) -> Result<PathBuf> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or(ProfilesError::ProfileNotFound(uid))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        Ok(self
            .inner
            .profiles_dir
            .join(source.materialized().file.as_path()))
    }

    pub async fn read_profile_file(&self, uid: ProfileId) -> Result<String> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        let raw = self
            .inner
            .fs
            .read(&source.materialized().file)
            .map_err(ClientError::Anyhow)?;
        match &item.definition {
            ProfileDefinition::Config { .. } => {
                crate::service::profile_file::normalize_yaml_document(&raw)
                    .map_err(ClientError::Anyhow)
            }
            ProfileDefinition::Transform { .. } => Ok(raw),
        }
    }

    pub async fn save_profile_file(&self, uid: ProfileId, data: String) -> Result<()> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        match source {
            ProfileSource::Local {
                binding:
                    LocalBinding::Managed {
                        materialized: materialized_file,
                    },
            } => {
                self.inner
                    .fs
                    .write_atomic(&materialized_file.file, &data)
                    .map_err(ClientError::Anyhow)?;
                Ok(())
            }
            ProfileSource::Remote { .. } => Err(ProfilesError::FileNotWritable {
                reason: "remote profiles are updater-owned".into(),
            }
            .into()),
            ProfileSource::Local {
                binding: LocalBinding::External { .. },
            } => Err(ProfilesError::FileNotWritable {
                reason: "external profiles are edited at their source".into(),
            }
            .into()),
        }
    }

    pub fn session_ports(&self) -> Option<ResolvedPortBindings> {
        self.inner.ports.cached_ports()
    }

    pub async fn promoted_runtime(&self) -> Option<Arc<core_runtime::RuntimeSnapshot>> {
        self.inner.core_client.lifecycle().promoted
    }

    pub(crate) fn runtime_product_path(&self) -> &camino::Utf8Path {
        self.inner.runtime_paths.product()
    }

    pub(crate) async fn promote_existing_runtime_product(
        &self,
    ) -> Result<Arc<core_runtime::RuntimeSnapshot>> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        let bytes = tokio::fs::read(self.inner.runtime_paths.product())
            .await
            .map_err(ClientError::Io)?;
        let config: serde_yaml::Mapping =
            serde_yaml::from_slice(&bytes).map_err(ClientError::SerdeYaml)?;
        let app = self.get_app_config().await?;
        let snapshot = Arc::new(core_runtime::RuntimeSnapshot::from_data(
            revision,
            app.core,
            Arc::from(bytes.clone()),
            core_runtime::RuntimeSnapshotData {
                exists_keys: config
                    .keys()
                    .filter_map(serde_yaml::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect(),
                config,
                postprocessing_output: Default::default(),
            },
        ));
        let candidate = self
            .inner
            .runtime_paths
            .create_candidate(&bytes)
            .await
            .map_err(ClientError::Anyhow)?;
        let checked = lease
            .check_and_promote(&candidate, app.core, self.inner.runtime_paths.product())
            .await;
        if let Err(error) = candidate.cleanup().await {
            tracing::warn!(%error, "failed to remove existing-product candidate config");
        }
        checked.map_err(|error| ClientError::Anyhow(error.into()))?;
        lease
            .publish_promoted(snapshot.clone())
            .await
            .map_err(|error| ClientError::Anyhow(error.into()))?;
        Ok(snapshot)
    }

    pub(crate) async fn start_promoted_runtime(&self) -> Result<()> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let promoted = self.promoted_runtime().await.ok_or_else(|| {
            ClientError::Custom("cannot start core without a promoted runtime".into())
        })?;
        lease
            .restart()
            .await
            .map_err(|error| ClientError::Anyhow(error.into()))?;
        lease
            .publish_applied(promoted)
            .await
            .map_err(|error| ClientError::Anyhow(error.into()))
    }

    fn actor_error_is_post_commit_exempt(
        error: &crate::core::actor::types::CoreActorError,
    ) -> bool {
        matches!(
            error,
            crate::core::actor::types::CoreActorError::ShuttingDown
        ) || matches!(
            error,
            crate::core::actor::types::CoreActorError::StaleOperation
                | crate::core::actor::types::CoreActorError::LifecycleInvariant(_)
        )
    }

    fn post_commit_actor_error(error: crate::core::actor::types::CoreActorError) -> ClientError {
        if matches!(
            error,
            crate::core::actor::types::CoreActorError::StaleOperation
                | crate::core::actor::types::CoreActorError::LifecycleInvariant(_)
        ) {
            tracing::error!(%error, "core lifecycle invariant failed after desired-state commit");
        }
        ClientError::Anyhow(anyhow::Error::new(error))
    }

    fn not_applied_report(revision: core_runtime::RuntimeRevision) -> runtime::RuntimeApplyReport {
        runtime::RuntimeApplyReport {
            outcome: runtime::RuntimeApplyOutcome::NotApplied,
            desired_revision: revision.get(),
            applied_revision: None,
        }
    }

    fn runtime_degradation(
        phase: runtime::DegradationPhase,
        code: &'static str,
        message: String,
    ) -> runtime::Degradation {
        runtime::Degradation {
            phase,
            code: code.into(),
            message,
            retryable: true,
        }
    }

    fn rebuild_failure_degradation(
        error: RuntimeRebuildError,
    ) -> std::result::Result<runtime::Degradation, ClientError> {
        match error {
            RuntimeRebuildError::Build(error) => Ok(Self::runtime_degradation(
                runtime::DegradationPhase::RuntimeBuild,
                "runtime_build_failed",
                error.to_string(),
            )),
            RuntimeRebuildError::CheckAndPromote(
                core_bridge::CheckAndPromoteFailure::Operation(error),
            ) => {
                let (phase, code) = match error.phase {
                    core_bridge::CheckAndPromotePhase::Check => (
                        runtime::DegradationPhase::RuntimeCheck,
                        "runtime_check_failed",
                    ),
                    core_bridge::CheckAndPromotePhase::Promote => (
                        runtime::DegradationPhase::RuntimePromote,
                        "runtime_promote_failed",
                    ),
                };
                Ok(Self::runtime_degradation(phase, code, error.to_string()))
            }
            RuntimeRebuildError::CheckAndPromote(core_bridge::CheckAndPromoteFailure::Actor(
                error,
            )) => {
                if Self::actor_error_is_post_commit_exempt(&error) {
                    return Err(Self::post_commit_actor_error(error));
                }
                let (phase, code) = match &error {
                    crate::core::actor::types::CoreActorError::NoBackend { .. } => (
                        runtime::DegradationPhase::RuntimeApply,
                        "core_backend_unavailable",
                    ),
                    _ => (
                        runtime::DegradationPhase::RuntimeCheck,
                        "runtime_check_failed",
                    ),
                };
                Ok(Self::runtime_degradation(phase, code, error.to_string()))
            }
            RuntimeRebuildError::Publish(error) => {
                if Self::actor_error_is_post_commit_exempt(&error) {
                    return Err(Self::post_commit_actor_error(error));
                }
                let (phase, code) = match &error {
                    crate::core::actor::types::CoreActorError::NoBackend { .. } => (
                        runtime::DegradationPhase::RuntimeApply,
                        "core_backend_unavailable",
                    ),
                    _ => (
                        runtime::DegradationPhase::RuntimePromote,
                        "runtime_promote_failed",
                    ),
                };
                Ok(Self::runtime_degradation(phase, code, error.to_string()))
            }
        }
    }

    fn apply_failure_degradation(
        error: crate::core::actor::types::CoreActorError,
    ) -> std::result::Result<runtime::Degradation, ClientError> {
        if Self::actor_error_is_post_commit_exempt(&error) {
            return Err(Self::post_commit_actor_error(error));
        }
        let (code, message) = match error {
            crate::core::actor::types::CoreActorError::NoBackend { last_error } => {
                ("core_backend_unavailable", last_error.to_string())
            }
            crate::core::actor::types::CoreActorError::Backend(error) => {
                use crate::core::actor::backend::BackendFailureClass;
                let code = match crate::core::actor::backend::classify_apply_backend_failure(
                    error.as_ref(),
                ) {
                    BackendFailureClass::RevisionConflict => "revision_conflict",
                    BackendFailureClass::NotRunning => "core_not_running",
                    BackendFailureClass::TransportLost => "core_transport_lost",
                    BackendFailureClass::Other => "runtime_apply_failed",
                };
                (code, error.to_string())
            }
            error => ("runtime_apply_failed", error.to_string()),
        };
        Ok(Self::runtime_degradation(
            runtime::DegradationPhase::RuntimeApply,
            code,
            message,
        ))
    }

    async fn rebuild_pipeline_with_lease(
        &self,
        lease: &mut dyn CoreLifecycleLease,
    ) -> (
        Result<runtime::RuntimeApplyReport>,
        Vec<runtime::Degradation>,
    ) {
        let revision = match self.inner.runtime_revisions.allocate() {
            Ok(revision) => revision,
            Err(error) => return (Err(ClientError::Anyhow(error)), Vec::new()),
        };
        let promoted = match self.regenerate_runtime_at_revision(lease, revision).await {
            Ok(promoted) => promoted,
            Err(error) => {
                return match Self::rebuild_failure_degradation(error) {
                    Ok(degradation) => (Ok(Self::not_applied_report(revision)), vec![degradation]),
                    Err(error) => (Err(error), Vec::new()),
                };
            }
        };
        match lease.apply_promoted(promoted).await {
            Ok(data) => {
                let (report, degradations) =
                    runtime::runtime_outcome_from_apply_data(&data, revision.get());
                (Ok(report), degradations)
            }
            Err(error) => match Self::apply_failure_degradation(error) {
                Ok(degradation) => (Ok(Self::not_applied_report(revision)), vec![degradation]),
                Err(error) => (Err(error), Vec::new()),
            },
        }
    }

    fn require_clean_pipeline(
        result: Result<runtime::RuntimeApplyReport>,
        degradations: Vec<runtime::Degradation>,
    ) -> Result<runtime::RuntimeApplyReport> {
        let report = result?;
        if degradations.is_empty() {
            return Ok(report);
        }
        Err(ClientError::Custom(
            degradations
                .iter()
                .map(|degradation| format!("{}: {}", degradation.code, degradation.message))
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }

    pub async fn patch_running_config(
        &self,
        patch: serde_yaml::Mapping,
    ) -> Result<runtime::MutationOutcome<runtime::RuntimeApplyReport>> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let overrides_patch: nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch =
            serde_yaml::from_value(serde_yaml::Value::Mapping(patch.clone()))
                .map_err(ClientError::SerdeYaml)?;
        let mut overrides = self.inner.clash_config.get().await?.state.overrides.clone();
        overrides.apply(overrides_patch);
        let mut desired = ClashConfig::new_empty_patch();
        desired.overrides = Some(overrides);
        let client = self.clone();
        let outcome = crate::feat::patch_clash_with_rebuild(
            self.clone(),
            patch,
            move |_restart| async move {
                client.inner.clash_config.patch(desired).await?;
                let (report, degradations) = client.rebuild_pipeline_with_lease(&mut *lease).await;
                Ok(runtime::MutationOutcome::from_parts(
                    report.map_err(anyhow::Error::from)?,
                    degradations,
                ))
            },
        )
        .await
        .map_err(ClientError::Anyhow)?;
        crate::feat::update_proxies_buff(None);
        Ok(outcome)
    }

    pub async fn rebuild_running_config(&self) -> Result<()> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let (report, degradations) = self.rebuild_pipeline_with_lease(&mut *lease).await;
        Self::require_clean_pipeline(report, degradations)?;
        drop(lease);
        self.inner.ui_sink.refresh_clash();
        // 用户决策 2026-07-06:所有 rebuild 统一触发(选项默认 false 门控)。
        self.inner.core.on_profile_change().await;
        Ok(())
    }

    async fn rebuild_running_config_in_background(&self) -> Result<()> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let (report, degradations) = self.rebuild_pipeline_with_lease(&mut *lease).await;
        report?;
        if degradations.is_empty() {
            drop(lease);
            self.inner.ui_sink.refresh_clash();
            self.inner.core.on_profile_change().await;
            return Ok(());
        }
        for degradation in degradations {
            tracing::warn!(
                phase = ?degradation.phase,
                code = %degradation.code,
                retryable = degradation.retryable,
                message = %degradation.message,
                "background-driven rebuild failed (degraded)",
            );
            self.inner.degradation.publish(degradation);
        }
        Ok(())
    }

    pub(crate) async fn regenerate_runtime(&self) -> Result<()> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        self.regenerate_runtime_inner(&mut *lease).await.map(|_| ())
    }

    /// Must only run while holding the core operation guard: revision allocation
    /// happens before desired snapshots are read, and failed attempts never reuse it.
    async fn regenerate_runtime_inner(
        &self,
        lease: &mut dyn CoreLifecycleLease,
    ) -> Result<Arc<core_runtime::RuntimeSnapshot>> {
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        self.regenerate_runtime_at_revision(lease, revision)
            .await
            .map_err(Into::into)
    }

    async fn regenerate_runtime_at_revision(
        &self,
        lease: &mut dyn CoreLifecycleLease,
        revision: core_runtime::RuntimeRevision,
    ) -> std::result::Result<Arc<core_runtime::RuntimeSnapshot>, RuntimeRebuildError> {
        let profiles = self
            .inner
            .profiles
            .get()
            .await
            .map_err(ClientError::from)
            .map_err(RuntimeRebuildError::Build)?;
        let clash = self
            .get_clash_config()
            .await
            .map_err(RuntimeRebuildError::Build)?;
        let app = self
            .get_app_config()
            .await
            .map_err(RuntimeRebuildError::Build)?;
        self.regenerate_runtime_with(lease, revision, profiles, clash, app)
            .await
    }

    async fn regenerate_runtime_with(
        &self,
        lease: &mut dyn CoreLifecycleLease,
        revision: core_runtime::RuntimeRevision,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: NyanpasuAppConfig,
    ) -> std::result::Result<Arc<core_runtime::RuntimeSnapshot>, RuntimeRebuildError> {
        let resolved_ports = self
            .inner
            .ports
            .resolve(&clash)
            .map_err(ClientError::Anyhow)
            .map_err(RuntimeRebuildError::Build)?;
        let profiles_dir = self.inner.profiles_dir.clone();
        let core = app.core;
        let builtin_enabled = app.enable_builtin_enhanced;
        let (data, yaml) = tokio::task::spawn_blocking(
            move || -> anyhow::Result<(core_runtime::RuntimeSnapshotData, String)> {
                let content = FsProfileContentSource::new(profiles_dir);
                let scripts = EnhanceScriptRunner::new()?;
                let input = RuntimeBuildInput {
                    profiles: profiles.clone(),
                    clash,
                    app,
                    resolved_ports,
                };
                let artifact = RuntimeBuilder::build(&input, &content, &scripts)?;
                let data = runtime_snapshot_data_from_artifact(
                    &artifact,
                    &profiles,
                    core,
                    builtin_enabled,
                )?;
                let yaml = format!(
                    "# Generated by Clash Nyanpasu\n\n{}",
                    serde_yaml::to_string(&data.config)?
                );
                Ok((data, yaml))
            },
        )
        .await
        .map_err(|error| ClientError::Custom(format!("runtime build task failed: {error}")))
        .map_err(RuntimeRebuildError::Build)?
        .map_err(ClientError::Anyhow)
        .map_err(RuntimeRebuildError::Build)?;
        let product_bytes: Arc<[u8]> = Arc::from(yaml.into_bytes());
        let snapshot = Arc::new(core_runtime::RuntimeSnapshot::from_data(
            revision,
            core,
            product_bytes.clone(),
            data,
        ));
        // Candidate -> check -> promote -> PUBLISH (spec §5.2, P0-1): readers
        // only ever see checked-and-promoted configs; a rejected candidate
        // leaves both the product and the manager untouched. target core =
        // the same input snapshot the builder used (P0-3).
        let candidate = self
            .inner
            .runtime_paths
            .create_candidate(&product_bytes)
            .await
            .map_err(|source| {
                RuntimeRebuildError::CheckAndPromote(
                    core_bridge::CheckAndPromoteFailure::Operation(
                        core_bridge::CheckAndPromoteError {
                            phase: core_bridge::CheckAndPromotePhase::Promote,
                            source,
                        },
                    ),
                )
            })?;
        if candidate.bytes_sha256() != snapshot.product_sha256 {
            return Err(RuntimeRebuildError::CheckAndPromote(
                core_bridge::CheckAndPromoteFailure::Operation(core_bridge::CheckAndPromoteError {
                    phase: core_bridge::CheckAndPromotePhase::Promote,
                    source: anyhow::anyhow!("runtime snapshot hash does not match candidate bytes"),
                }),
            ));
        }
        let checked = lease
            .check_and_promote(&candidate, core, self.inner.runtime_paths.product())
            .await;
        if let Err(error) = candidate.cleanup().await {
            tracing::warn!(%error, "failed to remove candidate config");
        }
        checked.map_err(RuntimeRebuildError::CheckAndPromote)?;
        lease
            .publish_promoted(snapshot.clone())
            .await
            .map_err(RuntimeRebuildError::Publish)?;
        Ok(snapshot)
    }
}

fn utf8_path(path: PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        mirror::{
            ClashLegacyBridge, NoopPreparedLegacyMirror, PreparedLegacyMirror, VergeLegacyBridge,
            WindowLegacyBridge,
        },
        profiles::ports::{
            CleanupOutcome, MaterializationReconcileReport, MockProfileFsPort,
            MockProfileMaterializationPort, MockRebuildNotifier, MockSubscriptionFetcher,
            PreparedCleanup, PreparedMaterialization, ProfileMaterializationPort, RebuildNotifier,
        },
    };
    use async_trait::async_trait;
    use camino::Utf8PathBuf;
    use nyanpasu_config::{
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileMetadata, ProfileSource, SubscriptionInfo,
        },
        state::window::{WindowLabel, WindowState},
    };
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap,
        sync::{
            Condvar, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    mockall::mock! {
        pub RunningCoreOps {}

        #[async_trait]
        impl TestRunningCoreBridge for RunningCoreOps {
            async fn check_and_promote(
                &self,
                candidate: &runtime::CandidateFile,
                target_core: nyanpasu_config::application::ClashCore,
            ) -> anyhow::Result<()>;
            async fn apply_config(&self) -> anyhow::Result<()>;
            async fn restart_core(&self) -> anyhow::Result<()>;
            async fn on_profile_change(&self);
        }
    }

    #[async_trait]
    pub trait TestRunningCoreBridge: Send + Sync + 'static {
        async fn check_and_promote(
            &self,
            candidate: &runtime::CandidateFile,
            target_core: nyanpasu_config::application::ClashCore,
        ) -> anyhow::Result<()>;
        async fn apply_config(&self) -> anyhow::Result<()>;
        async fn restart_core(&self) -> anyhow::Result<()>;
        async fn on_profile_change(&self);
    }

    pub struct MockRunningCoreBridge(Arc<MockRunningCoreOps>);

    impl MockRunningCoreBridge {
        pub fn new() -> Self {
            Self(Arc::new(MockRunningCoreOps::new()))
        }
    }

    impl std::ops::Deref for MockRunningCoreBridge {
        type Target = MockRunningCoreOps;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl std::ops::DerefMut for MockRunningCoreBridge {
        fn deref_mut(&mut self) -> &mut Self::Target {
            Arc::get_mut(&mut self.0).expect("mock expectations must be configured before sharing")
        }
    }

    #[async_trait]
    impl TestRunningCoreBridge for MockRunningCoreBridge {
        async fn check_and_promote(
            &self,
            candidate: &runtime::CandidateFile,
            target_core: nyanpasu_config::application::ClashCore,
        ) -> anyhow::Result<()> {
            self.0.check_and_promote(candidate, target_core).await
        }

        async fn apply_config(&self) -> anyhow::Result<()> {
            self.0.apply_config().await
        }

        async fn restart_core(&self) -> anyhow::Result<()> {
            self.0.restart_core().await
        }

        async fn on_profile_change(&self) {
            self.0.on_profile_change().await;
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for MockRunningCoreBridge {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            Ok(Box::new(MockCoreLease {
                inner: self.0.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("mock core status is not configured")
        }

        async fn on_profile_change(&self) {
            self.0.on_profile_change().await;
        }
    }

    struct MockCoreLease {
        inner: Arc<MockRunningCoreOps>,
    }

    #[async_trait]
    impl CoreLifecycleLease for MockCoreLease {
        async fn check_and_promote(
            &mut self,
            candidate: &runtime::CandidateFile,
            target_core: nyanpasu_config::application::ClashCore,
            _product: &camino::Utf8Path,
        ) -> std::result::Result<[u8; 32], core_bridge::CheckAndPromoteFailure> {
            self.inner.check_and_promote(candidate, target_core).await?;
            Ok(candidate.bytes_sha256())
        }

        async fn apply_promoted(
            &mut self,
            snapshot: Arc<core_runtime::RuntimeSnapshot>,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::apply::CoreApplyData,
            crate::core::actor::types::CoreActorError,
        > {
            self.inner.apply_config().await.map_err(|error| {
                crate::core::actor::types::CoreActorError::Backend(Arc::new(
                    crate::core::actor::backend::CoreBackendError::Construct(error),
                ))
            })?;
            Ok(core_bridge::test_apply_data(&snapshot))
        }

        async fn running_identity(
            &mut self,
        ) -> std::result::Result<
            (
                Option<crate::core::actor::types::CoreRequest>,
                crate::core::actor::types::FaithfulLifecycle,
            ),
            crate::core::actor::types::CoreActorError,
        > {
            Ok((None, crate::core::actor::types::FaithfulLifecycle::Running))
        }

        async fn restart(&mut self) -> std::result::Result<(), core_bridge::RestartFailure> {
            self.inner
                .restart_core()
                .await
                .map_err(core_bridge::RestartFailure::Operation)
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct TestCorePort {
        inner: Arc<dyn TestRunningCoreBridge>,
    }

    struct TestCoreLease {
        inner: Arc<dyn TestRunningCoreBridge>,
    }

    struct OperationProbeCore {
        gate: Arc<tokio::sync::Mutex<()>>,
        state: Arc<OperationProbeState>,
    }

    struct OperationProbeState {
        begin_calls: AtomicUsize,
        check_calls: AtomicUsize,
        active_checks: AtomicUsize,
        max_active_checks: AtomicUsize,
        candidates: StdMutex<Vec<Vec<u8>>>,
        begin_entered_tx: StdMutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release_begin_rx: StdMutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        first_check_entered_tx: StdMutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release_first_check_rx: StdMutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        second_begin_attempted_tx: StdMutex<Option<tokio::sync::oneshot::Sender<()>>>,
        applied_revisions: StdMutex<Vec<u64>>,
    }

    struct OperationProbeLease {
        state: Arc<OperationProbeState>,
        _guard: tokio::sync::OwnedMutexGuard<()>,
    }

    #[derive(Clone, Copy)]
    enum PromoteFailurePoint {
        BeforeWrite,
        AfterWrite,
    }

    struct PromoteFailureCore {
        failure: PromoteFailurePoint,
        check_calls: Arc<AtomicUsize>,
    }

    struct PromoteFailureLease {
        failure: PromoteFailurePoint,
        check_calls: Arc<AtomicUsize>,
    }

    struct ExemptRestartCore;

    struct ExemptRestartLease;

    #[async_trait]
    impl CoreLifecyclePort for ExemptRestartCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            Ok(Box::new(ExemptRestartLease))
        }

        async fn status(&self) -> anyhow::Result<core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("status is not used by the exempt restart test")
        }

        async fn on_profile_change(&self) {}
    }

    #[async_trait]
    impl CoreLifecycleLease for ExemptRestartLease {
        async fn check_and_promote(
            &mut self,
            candidate: &runtime::CandidateFile,
            _target_core: nyanpasu_config::application::ClashCore,
            _product: &camino::Utf8Path,
        ) -> std::result::Result<[u8; 32], core_bridge::CheckAndPromoteFailure> {
            Ok(candidate.bytes_sha256())
        }

        async fn running_identity(
            &mut self,
        ) -> std::result::Result<
            (
                Option<crate::core::actor::types::CoreRequest>,
                crate::core::actor::types::FaithfulLifecycle,
            ),
            crate::core::actor::types::CoreActorError,
        > {
            Ok((
                None,
                crate::core::actor::types::FaithfulLifecycle::Stopped { reason: None },
            ))
        }

        async fn apply_promoted(
            &mut self,
            _snapshot: Arc<core_runtime::RuntimeSnapshot>,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::apply::CoreApplyData,
            crate::core::actor::types::CoreActorError,
        > {
            Err(crate::core::actor::types::CoreActorError::StaleOperation)
        }

        async fn restart(&mut self) -> std::result::Result<(), core_bridge::RestartFailure> {
            Err(core_bridge::RestartFailure::Actor(
                crate::core::actor::types::CoreActorError::StaleOperation,
            ))
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for PromoteFailureCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            Ok(Box::new(PromoteFailureLease {
                failure: self.failure,
                check_calls: self.check_calls.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("status is not used by promote failure tests")
        }

        async fn on_profile_change(&self) {}
    }

    #[async_trait]
    impl CoreLifecycleLease for PromoteFailureLease {
        async fn check_and_promote(
            &mut self,
            candidate: &runtime::CandidateFile,
            _target_core: nyanpasu_config::application::ClashCore,
            product: &camino::Utf8Path,
        ) -> std::result::Result<[u8; 32], core_bridge::CheckAndPromoteFailure> {
            let call = self.check_calls.fetch_add(1, Ordering::SeqCst);
            if call == 1 && matches!(self.failure, PromoteFailurePoint::BeforeWrite) {
                return Err(core_bridge::CheckAndPromoteFailure::Operation(
                    core_bridge::CheckAndPromoteError {
                        phase: core_bridge::CheckAndPromotePhase::Promote,
                        source: anyhow::anyhow!("candidate hash mismatch before product write"),
                    },
                ));
            }

            let bytes = tokio::fs::read(candidate.path()).await.map_err(|source| {
                core_bridge::CheckAndPromoteFailure::Operation(core_bridge::CheckAndPromoteError {
                    phase: core_bridge::CheckAndPromotePhase::Promote,
                    source: source.into(),
                })
            })?;
            if let Some(parent) = product.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|source| {
                    core_bridge::CheckAndPromoteFailure::Operation(
                        core_bridge::CheckAndPromoteError {
                            phase: core_bridge::CheckAndPromotePhase::Promote,
                            source: source.into(),
                        },
                    )
                })?;
            }
            tokio::fs::write(product, bytes).await.map_err(|source| {
                core_bridge::CheckAndPromoteFailure::Operation(core_bridge::CheckAndPromoteError {
                    phase: core_bridge::CheckAndPromotePhase::Promote,
                    source: source.into(),
                })
            })?;
            if call == 1 && matches!(self.failure, PromoteFailurePoint::AfterWrite) {
                return Err(core_bridge::CheckAndPromoteFailure::Operation(
                    core_bridge::CheckAndPromoteError {
                        phase: core_bridge::CheckAndPromotePhase::Promote,
                        source: anyhow::anyhow!("product hash mismatch after product write"),
                    },
                ));
            }
            Ok(candidate.bytes_sha256())
        }

        async fn apply_promoted(
            &mut self,
            snapshot: Arc<core_runtime::RuntimeSnapshot>,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::apply::CoreApplyData,
            crate::core::actor::types::CoreActorError,
        > {
            Ok(core_bridge::test_apply_data(&snapshot))
        }

        async fn running_identity(
            &mut self,
        ) -> std::result::Result<
            (
                Option<crate::core::actor::types::CoreRequest>,
                crate::core::actor::types::FaithfulLifecycle,
            ),
            crate::core::actor::types::CoreActorError,
        > {
            Ok((None, crate::core::actor::types::FaithfulLifecycle::Running))
        }

        async fn restart(&mut self) -> std::result::Result<(), core_bridge::RestartFailure> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn promote_failure_core(failure: PromoteFailurePoint) -> Arc<dyn CoreLifecyclePort> {
        Arc::new(PromoteFailureCore {
            failure,
            check_calls: Arc::new(AtomicUsize::new(0)),
        })
    }

    impl OperationProbeState {
        fn new() -> Self {
            Self {
                begin_calls: AtomicUsize::new(0),
                check_calls: AtomicUsize::new(0),
                active_checks: AtomicUsize::new(0),
                max_active_checks: AtomicUsize::new(0),
                candidates: StdMutex::new(Vec::new()),
                begin_entered_tx: StdMutex::new(None),
                release_begin_rx: StdMutex::new(None),
                first_check_entered_tx: StdMutex::new(None),
                release_first_check_rx: StdMutex::new(None),
                second_begin_attempted_tx: StdMutex::new(None),
                applied_revisions: StdMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for OperationProbeCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            let call = self.state.begin_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                if let Some(tx) = self.state.begin_entered_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                let release = self.state.release_begin_rx.lock().unwrap().take();
                if let Some(release) = release {
                    let _ = release.await;
                }
            } else if call == 1
                && let Some(tx) = self.state.second_begin_attempted_tx.lock().unwrap().take()
            {
                let _ = tx.send(());
            }
            let guard = self.gate.clone().lock_owned().await;
            Ok(Box::new(OperationProbeLease {
                state: self.state.clone(),
                _guard: guard,
            }))
        }

        async fn status(&self) -> anyhow::Result<core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("status is not used by operation ordering tests")
        }

        async fn on_profile_change(&self) {}
    }

    #[async_trait]
    impl CoreLifecycleLease for OperationProbeLease {
        async fn check_and_promote(
            &mut self,
            candidate: &runtime::CandidateFile,
            _target_core: nyanpasu_config::application::ClashCore,
            _product: &camino::Utf8Path,
        ) -> std::result::Result<[u8; 32], core_bridge::CheckAndPromoteFailure> {
            let call = self.state.check_calls.fetch_add(1, Ordering::SeqCst);
            let active = self.state.active_checks.fetch_add(1, Ordering::SeqCst) + 1;
            self.state
                .max_active_checks
                .fetch_max(active, Ordering::SeqCst);
            let bytes = tokio::fs::read(candidate.path())
                .await
                .map_err(anyhow::Error::from)?;
            self.state.candidates.lock().unwrap().push(bytes);
            if call == 0 {
                if let Some(tx) = self.state.first_check_entered_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                let release = self.state.release_first_check_rx.lock().unwrap().take();
                if let Some(release) = release {
                    let _ = release.await;
                }
            }
            self.state.active_checks.fetch_sub(1, Ordering::SeqCst);
            Ok(candidate.bytes_sha256())
        }

        async fn apply_promoted(
            &mut self,
            snapshot: Arc<core_runtime::RuntimeSnapshot>,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::apply::CoreApplyData,
            crate::core::actor::types::CoreActorError,
        > {
            Ok(core_bridge::test_apply_data(&snapshot))
        }

        async fn running_identity(
            &mut self,
        ) -> std::result::Result<
            (
                Option<crate::core::actor::types::CoreRequest>,
                crate::core::actor::types::FaithfulLifecycle,
            ),
            crate::core::actor::types::CoreActorError,
        > {
            Ok((None, crate::core::actor::types::FaithfulLifecycle::Running))
        }

        async fn restart(&mut self) -> std::result::Result<(), core_bridge::RestartFailure> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn publish_applied(
            &mut self,
            snapshot: Arc<core_runtime::RuntimeSnapshot>,
        ) -> std::result::Result<(), crate::core::actor::types::CoreActorError> {
            self.state
                .applied_revisions
                .lock()
                .unwrap()
                .push(snapshot.revision.get());
            Ok(())
        }
    }

    fn operation_probe_core(state: Arc<OperationProbeState>) -> Arc<dyn CoreLifecyclePort> {
        Arc::new(OperationProbeCore {
            gate: Arc::new(tokio::sync::Mutex::new(())),
            state,
        })
    }

    #[async_trait]
    impl CoreLifecyclePort for TestCorePort {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            Ok(Box::new(TestCoreLease {
                inner: self.inner.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("test core status is not configured")
        }

        async fn on_profile_change(&self) {
            self.inner.on_profile_change().await;
        }
    }

    #[async_trait]
    impl CoreLifecycleLease for TestCoreLease {
        async fn check_and_promote(
            &mut self,
            candidate: &runtime::CandidateFile,
            target_core: nyanpasu_config::application::ClashCore,
            _product: &camino::Utf8Path,
        ) -> std::result::Result<[u8; 32], core_bridge::CheckAndPromoteFailure> {
            self.inner.check_and_promote(candidate, target_core).await?;
            Ok(candidate.bytes_sha256())
        }

        async fn apply_promoted(
            &mut self,
            snapshot: Arc<core_runtime::RuntimeSnapshot>,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::apply::CoreApplyData,
            crate::core::actor::types::CoreActorError,
        > {
            self.inner.apply_config().await.map_err(|error| {
                crate::core::actor::types::CoreActorError::Backend(Arc::new(
                    crate::core::actor::backend::CoreBackendError::Construct(error),
                ))
            })?;
            Ok(core_bridge::test_apply_data(&snapshot))
        }

        async fn running_identity(
            &mut self,
        ) -> std::result::Result<
            (
                Option<crate::core::actor::types::CoreRequest>,
                crate::core::actor::types::FaithfulLifecycle,
            ),
            crate::core::actor::types::CoreActorError,
        > {
            Ok((None, crate::core::actor::types::FaithfulLifecycle::Running))
        }

        async fn restart(&mut self) -> std::result::Result<(), core_bridge::RestartFailure> {
            self.inner
                .restart_core()
                .await
                .map_err(core_bridge::RestartFailure::Operation)
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    pub(crate) fn test_core_port(
        inner: Arc<dyn TestRunningCoreBridge>,
    ) -> Arc<dyn CoreLifecyclePort> {
        Arc::new(TestCorePort { inner })
    }

    struct NoopVergeBridge;

    impl VergeLegacyBridge for NoopVergeBridge {
        fn prepare(
            &self,
            _snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    pub(crate) async fn test_application_client(
        dir: &TempDir,
        core: nyanpasu_config::application::ClashCore,
    ) -> ApplicationClient {
        let seed = NyanpasuAppConfig {
            core,
            ..NyanpasuAppConfig::default()
        };
        ApplicationClient::new(
            temp_config_path(dir, "application.yaml"),
            seed,
            Arc::new(NoopVergeBridge),
        )
        .await
        .unwrap()
    }

    struct RecordingVergeBridge {
        mirrored_theme_color: Arc<StdMutex<Option<String>>>,
    }

    struct RecordingPreparedVergeMirror {
        mirrored_theme_color: Arc<StdMutex<Option<String>>>,
        theme_color: String,
    }

    impl PreparedLegacyMirror for RecordingPreparedVergeMirror {
        fn apply(self: Box<Self>) {
            *self
                .mirrored_theme_color
                .lock()
                .expect("mirror capture should not poison") = Some(self.theme_color);
        }
    }

    impl VergeLegacyBridge for RecordingVergeBridge {
        fn prepare(
            &self,
            snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(RecordingPreparedVergeMirror {
                mirrored_theme_color: Arc::clone(&self.mirrored_theme_color),
                theme_color: snap.theme_color.to_string(),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    struct NoopWindowBridge;

    impl WindowLegacyBridge for NoopWindowBridge {
        fn prepare(
            &self,
            _snap: &PersistentState,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    struct FixedCoreBinaryResolver(Utf8PathBuf);

    impl crate::core::actor::request::CoreBinaryResolver for FixedCoreBinaryResolver {
        fn resolve(&self, _kind: &nyanpasu_utils::core::CoreType) -> anyhow::Result<Utf8PathBuf> {
            Ok(self.0.clone())
        }
    }

    #[derive(Default)]
    struct NoopCoreDegradationSink;

    impl crate::core::actor::backend::CoreDegradationSink for NoopCoreDegradationSink {
        fn publish(&self, _degradation: runtime::Degradation) {}
    }

    #[derive(Default)]
    struct RecordingCoreDegradationSink(StdMutex<Vec<runtime::Degradation>>);

    impl crate::core::actor::backend::CoreDegradationSink for RecordingCoreDegradationSink {
        fn publish(&self, degradation: runtime::Degradation) {
            self.0.lock().unwrap().push(degradation);
        }
    }

    struct NotifyingCoreDegradationSink(tokio::sync::mpsc::UnboundedSender<runtime::Degradation>);

    impl crate::core::actor::backend::CoreDegradationSink for NotifyingCoreDegradationSink {
        fn publish(&self, degradation: runtime::Degradation) {
            let _ = self.0.send(degradation);
        }
    }

    #[derive(Default)]
    struct NoopServiceControlOps;

    #[async_trait]
    impl crate::core::actor::backend::ServiceControlOps for NoopServiceControlOps {
        async fn install(
            &self,
            _reconciler: crate::core::actor::request::CoreModeReconciler,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn start(
            &self,
            _reconciler: crate::core::actor::request::CoreModeReconciler,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn restart(
            &self,
            _reconciler: crate::core::actor::request::CoreModeReconciler,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct ScriptedServiceControlOps {
        error: Option<&'static str>,
        calls: AtomicUsize,
    }

    struct BlockingVergeBridge {
        entered: mpsc::Sender<()>,
        release: Arc<(StdMutex<bool>, Condvar)>,
    }

    impl VergeLegacyBridgeTrait for BlockingVergeBridge {
        fn prepare(
            &self,
            _snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            self.entered.send(()).unwrap();
            let (lock, wake) = &*self.release;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = wake.wait(released).unwrap();
            }
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    impl ScriptedServiceControlOps {
        fn success() -> Self {
            Self {
                error: None,
                calls: AtomicUsize::new(0),
            }
        }

        fn failing(error: &'static str) -> Self {
            Self {
                error: Some(error),
                calls: AtomicUsize::new(0),
            }
        }

        fn result(&self) -> anyhow::Result<()> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            match self.error {
                Some(error) => anyhow::bail!(error),
                None => Ok(()),
            }
        }
    }

    #[async_trait]
    impl crate::core::actor::backend::ServiceControlOps for ScriptedServiceControlOps {
        async fn install(
            &self,
            _reconciler: crate::core::actor::request::CoreModeReconciler,
        ) -> anyhow::Result<()> {
            self.result()
        }

        async fn start(
            &self,
            _reconciler: crate::core::actor::request::CoreModeReconciler,
        ) -> anyhow::Result<()> {
            self.result()
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.result()
        }

        async fn restart(
            &self,
            _reconciler: crate::core::actor::request::CoreModeReconciler,
        ) -> anyhow::Result<()> {
            self.result()
        }
    }

    struct FailingCoreBinaryResolver;

    impl crate::core::actor::request::CoreBinaryResolver for FailingCoreBinaryResolver {
        fn resolve(&self, _kind: &nyanpasu_utils::core::CoreType) -> anyhow::Result<Utf8PathBuf> {
            anyhow::bail!("scripted binary resolution failure")
        }
    }

    pub(crate) fn test_binary_resolver(
        dir: &TempDir,
    ) -> Arc<dyn crate::core::actor::request::CoreBinaryResolver> {
        Arc::new(FixedCoreBinaryResolver(
            Utf8PathBuf::from_path_buf(dir.path().join("fake-core")).unwrap(),
        ))
    }

    pub(crate) fn test_degradation_sink()
    -> Arc<dyn crate::core::actor::backend::CoreDegradationSink> {
        Arc::new(NoopCoreDegradationSink)
    }

    pub(crate) fn test_service_control() -> Arc<dyn crate::core::actor::backend::ServiceControlOps>
    {
        Arc::new(NoopServiceControlOps)
    }

    async fn test_actor_parts(
        paths: &PathResolver,
        runtime_paths: RuntimePaths,
    ) -> (CoreClient, crate::core::actor::request::CoreRequestFactory) {
        let requests = crate::core::actor::request::CoreRequestFactory::new(
            paths,
            runtime_paths,
            Arc::new(FixedCoreBinaryResolver(
                Utf8PathBuf::from_path_buf(paths.app_data_dir().join("fake-core")).unwrap(),
            )),
        )
        .unwrap();
        let core = CoreClient::new(core::CoreClientArgs {
            mode: crate::core::RunType::Normal,
            requests: requests.clone(),
            degradation: Arc::new(NoopCoreDegradationSink),
        })
        .await
        .unwrap();
        (core, requests)
    }

    pub(crate) async fn actor_backed_test_client(
        dir: &TempDir,
        backend: crate::core::actor::backend::TestBackend,
        degradation: Arc<dyn crate::core::actor::backend::CoreDegradationSink>,
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let ports = Arc::new(SessionPortResolver::default());
        ports.resolve(&ClashConfig::default()).unwrap();
        let file_service = Arc::new(ProfileFileService::new(
            paths.clone(),
            ports.clone() as Arc<dyn SelfProxyPortSource>,
        ));
        let rebuild = rebuild::RebuildCoordinator::new();
        let profiles = profiles::ProfilesClient::new(
            temp_config_path(dir, "profiles.yaml"),
            file_service.clone() as Arc<dyn ProfileFsPort>,
            file_service.clone() as Arc<dyn SubscriptionFetcher>,
            file_service.clone() as Arc<dyn ProfileMaterializationPort>,
            Arc::new(rebuild.notifier()),
        )
        .await
        .unwrap();
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let requests = crate::core::actor::request::CoreRequestFactory::new(
            &paths,
            runtime_paths.clone(),
            test_binary_resolver(dir),
        )
        .unwrap();
        let core_client = CoreClient::new_with_reconciled_backend(
            core::CoreClientArgs {
                mode: crate::core::RunType::Normal,
                requests: requests.clone(),
                degradation: degradation.clone(),
            },
            backend,
        )
        .await
        .unwrap();
        let core = Arc::new(core::CoreLifecycleAdapter::new(
            core_client.clone(),
            application.clone(),
            requests.clone(),
        ));
        NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            file_service.clone() as Arc<dyn ProfileFsPort>,
            ports,
            paths.app_profiles_dir(),
            runtime_paths,
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            core_client,
            core,
            requests,
            test_service_control(),
            degradation,
            Arc::new(NoopSystemDnsCache),
            rebuild,
        )
    }

    async fn service_facade(
        dir: &TempDir,
        enable_service: bool,
        backend: crate::core::actor::backend::TestBackend,
        binary: Arc<dyn crate::core::actor::request::CoreBinaryResolver>,
        service_control: Arc<dyn crate::core::actor::backend::ServiceControlOps>,
        application: Option<ApplicationClient>,
    ) -> NyanpasuClient {
        let (default_application, session_state, clash_config) =
            test_typed_config_clients(dir).await;
        let application = match application {
            Some(application) => application,
            None => {
                let mut patch = NyanpasuAppConfig::new_empty_patch();
                patch.enable_service_mode = Some(enable_service);
                default_application.patch(patch).await.unwrap();
                default_application
            }
        };
        let profiles = profiles::ProfilesClient::new(
            temp_config_path(dir, "profiles.yaml"),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .unwrap();
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let requests = crate::core::actor::request::CoreRequestFactory::new(
            &paths,
            runtime_paths.clone(),
            binary,
        )
        .unwrap();
        let degradation = test_degradation_sink();
        let core_client = CoreClient::new_with_reconciled_backend(
            core::CoreClientArgs {
                mode: crate::core::RunType::Normal,
                requests: requests.clone(),
                degradation: degradation.clone(),
            },
            backend,
        )
        .await
        .unwrap();
        let ports = Arc::new(SessionPortResolver::default());
        ports.resolve(&ClashConfig::default()).unwrap();
        NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            Arc::new(MockProfileFsPort::new()),
            ports,
            paths.app_profiles_dir(),
            runtime_paths,
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            core_client,
            test_core_port(Arc::new(MockRunningCoreBridge::new())),
            requests,
            service_control,
            degradation,
            Arc::new(NoopSystemDnsCache),
            rebuild::RebuildCoordinator::new(),
        )
    }

    fn facade_observation(
        lifecycle: crate::core::actor::types::FaithfulLifecycle,
    ) -> crate::core::actor::types::BackendObservation {
        let state = match lifecycle {
            crate::core::actor::types::FaithfulLifecycle::Stopped { ref reason } => {
                nyanpasu_ipc::api::status::CoreState::Stopped(reason.clone())
            }
            crate::core::actor::types::FaithfulLifecycle::Starting
            | crate::core::actor::types::FaithfulLifecycle::Restarting => {
                nyanpasu_ipc::api::status::CoreState::Stopped(None)
            }
            crate::core::actor::types::FaithfulLifecycle::Running
            | crate::core::actor::types::FaithfulLifecycle::Switching
            | crate::core::actor::types::FaithfulLifecycle::Stopping => {
                nyanpasu_ipc::api::status::CoreState::Running
            }
        };
        crate::core::actor::types::BackendObservation {
            view: crate::core::actor::types::CoreStatusView {
                state,
                state_changed_at: 1,
                run_type: crate::core::RunType::Normal,
                revision: None,
                recovery_exhausted: false,
            },
            lifecycle,
        }
    }

    fn facade_backend() -> crate::core::actor::backend::TestBackend {
        crate::core::actor::backend::TestBackend::new(facade_observation(
            crate::core::actor::types::FaithfulLifecycle::Running,
        ))
    }

    #[tokio::test]
    async fn core_status_facade_refreshes_stale_state_and_publishes_degradation_once() {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        let degradation = Arc::new(RecordingCoreDegradationSink::default());
        let client = actor_backed_test_client(&dir, backend.clone(), degradation.clone()).await;
        let mut status = client.inner.core_client.subscribe_status();
        status.borrow_and_update();
        let reason = "core kept crashing; restart budget exhausted\nfacade".to_owned();
        backend.set_observation(crate::core::actor::types::BackendObservation {
            view: crate::core::actor::types::CoreStatusView {
                state: nyanpasu_ipc::api::status::CoreState::Stopped(Some(reason.clone())),
                state_changed_at: 2,
                run_type: crate::core::RunType::Normal,
                revision: None,
                recovery_exhausted: true,
            },
            lifecycle: crate::core::actor::types::FaithfulLifecycle::Stopped {
                reason: Some(reason),
            },
        });

        assert!(matches!(
            client.core_status().0,
            std::borrow::Cow::Owned(nyanpasu_ipc::api::status::CoreState::Running)
        ));
        status.changed().await.unwrap();
        assert!(matches!(
            client.core_status().0,
            std::borrow::Cow::Owned(nyanpasu_ipc::api::status::CoreState::Stopped(Some(_)))
        ));
        status.changed().await.unwrap();
        assert_eq!(degradation.0.lock().unwrap().len(), 1);
        client.shutdown().await;
    }

    struct NoopClashBridge;

    impl ClashLegacyBridge for NoopClashBridge {
        fn prepare(&self, _snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    fn temp_config_path(dir: &TempDir, file_name: &str) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join(file_name)).expect("temp path should be UTF-8")
    }

    /// Restores a directory's unix mode on drop so tempdir cleanup stays reliable
    /// after permission-poison tests.
    #[cfg(unix)]
    struct RestoreDirMode {
        path: PathBuf,
        mode: u32,
    }

    #[cfg(unix)]
    impl Drop for RestoreDirMode {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.mode));
        }
    }

    async fn test_typed_config_clients(
        dir: &TempDir,
    ) -> (ApplicationClient, SessionStateClient, ClashConfigClient) {
        let application = ApplicationClient::new(
            temp_config_path(dir, "application.yaml"),
            NyanpasuAppConfig::default(),
            Arc::new(NoopVergeBridge),
        )
        .await
        .expect("application client should be created");
        let session_state = SessionStateClient::new(
            temp_config_path(dir, "session-state.yaml"),
            PersistentState::default(),
            Arc::new(NoopWindowBridge),
        )
        .await
        .expect("session state client should be created");
        let clash_config = ClashConfigClient::new(
            temp_config_path(dir, "clash-config.yaml"),
            ClashConfig::default(),
            Arc::new(NoopClashBridge),
        )
        .await
        .expect("clash config client should be created");

        (application, session_state, clash_config)
    }

    fn test_materialization_port() -> Arc<dyn ProfileMaterializationPort> {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_state_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("state".into())));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("file".into())));
        materialization.expect_promote().returning(|_| Ok(()));
        materialization.expect_complete().returning(|_| Ok(()));
        materialization.expect_compensate().returning(|_| Ok(()));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Ok(PreparedCleanup::new("cleanup".into())));
        materialization
            .expect_activate_cleanup()
            .returning(|_| Ok(()));
        materialization
            .expect_cancel_cleanup()
            .returning(|_| Ok(()));
        materialization
            .expect_retry_cleanup()
            .returning(|_, _| Ok(CleanupOutcome::Removed));
        Arc::new(materialization)
    }

    async fn test_client(dir: &TempDir) -> NyanpasuClient {
        test_client_with_system_dns(dir, Arc::new(NoopSystemDnsCache)).await
    }

    async fn test_client_with_system_dns(
        dir: &TempDir,
        system_dns: Arc<dyn SystemDnsCache>,
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        let profiles = profiles::ProfilesClient::new(
            temp_config_path(dir, "profiles.yaml"),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .expect("profiles client should be created");
        let ports = Arc::new(SessionPortResolver::default());
        ports
            .resolve(&ClashConfig::default())
            .expect("default ports should resolve");
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let (core_client, requests) = test_actor_parts(&paths, runtime_paths.clone()).await;
        NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            Arc::new(MockProfileFsPort::new()),
            ports,
            dir.path().join("profiles"),
            runtime_paths,
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            core_client,
            test_core_port(Arc::new(MockRunningCoreBridge::new())),
            requests,
            test_service_control(),
            test_degradation_sink(),
            system_dns,
            rebuild::RebuildCoordinator::new(),
        )
    }

    #[tokio::test]
    async fn flush_system_dns_cache_forwards_to_injected_adapter() {
        let dir = tempdir().expect("tempdir should be created");
        let mut system_dns = MockSystemDnsCache::new();
        system_dns.expect_flush().times(1).returning(|| Ok(()));
        let client = test_client_with_system_dns(&dir, Arc::new(system_dns)).await;

        client
            .flush_system_dns_cache()
            .await
            .expect("DNS cache flush should succeed");
    }

    #[tokio::test]
    async fn flush_system_dns_cache_propagates_adapter_failure() {
        let dir = tempdir().expect("tempdir should be created");
        let mut system_dns = MockSystemDnsCache::new();
        system_dns
            .expect_flush()
            .times(1)
            .returning(|| anyhow::bail!("dns flush exploded"));
        let client = test_client_with_system_dns(&dir, Arc::new(system_dns)).await;

        let error = client.flush_system_dns_cache().await.unwrap_err();
        assert!(error.to_string().contains("dns flush exploded"));
    }

    #[tokio::test]
    async fn service_control_failure_still_reconciles_and_remains_the_result() {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        let control = Arc::new(ScriptedServiceControlOps::failing("control-a"));
        let client = service_facade(
            &dir,
            true,
            backend.clone(),
            test_binary_resolver(&dir),
            control.clone(),
            None,
        )
        .await;

        let error = client.start_service().await.unwrap_err();
        assert!(error.to_string().contains("control-a"));
        assert_eq!(control.calls.load(Ordering::Acquire), 1);
        assert_eq!(backend.run_calls(), 1);
    }

    #[tokio::test]
    async fn disabled_service_mode_does_not_touch_the_core_actor() {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        let client = service_facade(
            &dir,
            false,
            backend.clone(),
            test_binary_resolver(&dir),
            Arc::new(ScriptedServiceControlOps::success()),
            None,
        )
        .await;

        client.start_service().await.unwrap();
        assert_eq!(backend.run_calls(), 0);
        assert_eq!(backend.shutdown_calls(), 0);
    }

    #[tokio::test]
    async fn backend_replacement_failure_does_not_replace_control_success() {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        backend.fail_next_replace();
        let client = service_facade(
            &dir,
            true,
            backend.clone(),
            test_binary_resolver(&dir),
            Arc::new(ScriptedServiceControlOps::success()),
            None,
        )
        .await;

        client.start_service().await.unwrap();
        assert_eq!(backend.run_calls(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn snapshot_read_failure_does_not_replace_control_success() {
        let dir = tempdir().unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new((StdMutex::new(false), Condvar::new()));
        let seed = NyanpasuAppConfig {
            enable_service_mode: true,
            ..NyanpasuAppConfig::default()
        };
        let application = ApplicationClient::new(
            temp_config_path(&dir, "blocked-application.yaml"),
            seed,
            Arc::new(BlockingVergeBridge {
                entered: entered_tx,
                release: release.clone(),
            }),
        )
        .await
        .unwrap();
        let control = Arc::new(ScriptedServiceControlOps::success());
        let client = service_facade(
            &dir,
            true,
            facade_backend(),
            test_binary_resolver(&dir),
            control.clone(),
            Some(application.clone()),
        )
        .await;
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_silent_start = Some(true);
        let patch_task = tokio::spawn(async move { application.patch(patch).await });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let result = client.start_service().await;
        let (lock, wake) = &*release;
        *lock.lock().unwrap() = true;
        wake.notify_all();
        patch_task.await.unwrap().unwrap();

        result.unwrap();
        assert_eq!(control.calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn request_and_run_failures_do_not_replace_control_success() {
        let request_dir = tempdir().unwrap();
        let request_backend = facade_backend();
        let request_client = service_facade(
            &request_dir,
            true,
            request_backend.clone(),
            Arc::new(FailingCoreBinaryResolver),
            Arc::new(ScriptedServiceControlOps::success()),
            None,
        )
        .await;
        request_client.start_service().await.unwrap();
        assert_eq!(request_backend.run_calls(), 0);

        let run_dir = tempdir().unwrap();
        let run_backend = facade_backend();
        run_backend.fail_next_run();
        let run_client = service_facade(
            &run_dir,
            true,
            run_backend.clone(),
            test_binary_resolver(&run_dir),
            Arc::new(ScriptedServiceControlOps::success()),
            None,
        )
        .await;
        run_client.start_service().await.unwrap();
        assert_eq!(run_backend.run_calls(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn operation_timeout_does_not_replace_control_success() {
        let dir = tempdir().unwrap();
        let client = service_facade(
            &dir,
            true,
            facade_backend(),
            test_binary_resolver(&dir),
            Arc::new(ScriptedServiceControlOps::success()),
            None,
        )
        .await;
        let held = client.inner.core_client.begin_operation().await.unwrap();
        let call = tokio::spawn({
            let client = client.clone();
            async move { client.start_service().await }
        });
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(121)).await;

        call.await.unwrap().unwrap();
        drop(held);
    }

    #[tokio::test]
    async fn control_error_is_not_replaced_by_a_reconcile_error() {
        let dir = tempdir().unwrap();
        let client = service_facade(
            &dir,
            true,
            facade_backend(),
            Arc::new(FailingCoreBinaryResolver),
            Arc::new(ScriptedServiceControlOps::failing("control-a")),
            None,
        )
        .await;

        let error = client.start_service().await.unwrap_err();
        assert!(error.to_string().contains("control-a"));
        assert!(!error.to_string().contains("binary resolution"));
    }

    /// Like [`test_profiles_client_args`], but accepts an already-typed
    /// [`CoreLifecyclePort`] (e.g. process-backed S09 adapter) without the
    /// mockall `TestRunningCoreBridge` wrapper.
    pub(crate) fn test_client_args_with_lifecycle(
        dir: &TempDir,
        core: Arc<dyn CoreLifecyclePort>,
    ) -> ClientSetupArgs {
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        ClientSetupArgs {
            paths,
            runtime_paths,
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core: Some(core),
            binary_resolver: test_binary_resolver(dir),
            degradation: test_degradation_sink(),
            service_control: test_service_control(),
            system_dns: Arc::new(NoopSystemDnsCache),
        }
    }

    pub(crate) fn test_profiles_client_args(
        dir: &TempDir,
        core: Arc<dyn TestRunningCoreBridge>,
    ) -> ClientSetupArgs {
        test_client_args_with_lifecycle(dir, test_core_port(core))
    }

    pub(crate) fn minimal_file_profile_request() -> NewProfileRequest {
        NewProfileRequest {
            metadata: ProfileMetadata {
                name: "t".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("t.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    /// Build a facade whose profiles domain uses a real [`ProfileFileService`]
    /// for the filesystem port and an injected fake fetcher. The refresh
    /// transaction must materialize `{uid}.yaml` on disk so the
    /// activate-triggered rebuild can read it back; only the network fetch is
    /// faked.
    async fn test_client_with_fetcher(
        dir: &TempDir,
        fetcher: Arc<dyn SubscriptionFetcher>,
        core: Arc<dyn TestRunningCoreBridge>,
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let ports = Arc::new(SessionPortResolver::default());
        ports
            .resolve(&ClashConfig::default())
            .expect("default ports should resolve");
        let file_service = Arc::new(ProfileFileService::new(
            paths.clone(),
            ports.clone() as Arc<dyn SelfProxyPortSource>,
        ));
        let rebuild = rebuild::RebuildCoordinator::new();
        let profiles = profiles::ProfilesClient::new(
            temp_config_path(dir, "profiles.yaml"),
            file_service.clone() as Arc<dyn ProfileFsPort>,
            fetcher,
            file_service.clone() as Arc<dyn ProfileMaterializationPort>,
            Arc::new(rebuild.notifier()),
        )
        .await
        .expect("profiles client should be created");
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let (core_client, requests) = test_actor_parts(&paths, runtime_paths.clone()).await;
        let client = NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            file_service.clone() as Arc<dyn ProfileFsPort>,
            ports,
            paths.app_profiles_dir(),
            runtime_paths,
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            core_client,
            test_core_port(core),
            requests,
            test_service_control(),
            test_degradation_sink(),
            Arc::new(NoopSystemDnsCache),
            rebuild,
        );
        client.start_rebuild_worker();
        client
    }

    #[test]
    fn client_constructs_with_mandatory_typed_config_clients() {
        let dir = tempdir().expect("tempdir should be created");

        tauri::async_runtime::block_on(async {
            let client = test_client(&dir).await;
            let _ = client.clone();
        });
    }

    #[test]
    fn typed_config_facade_delegates_to_typed_clients() {
        let dir = tempdir().expect("tempdir should be created");

        tauri::async_runtime::block_on(async {
            let client = test_client(&dir).await;

            let mut app_patch = NyanpasuAppConfig::new_empty_patch();
            app_patch.enable_system_proxy = Some(true);
            client
                .patch_app_config(app_patch)
                .await
                .expect("app patch should succeed");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);

            let mut app_replacement = NyanpasuAppConfig::default();
            app_replacement.enable_silent_start = true;
            client
                .replace_app_config(app_replacement)
                .await
                .expect("app replace should succeed");
            assert!(client.get_app_config().await.unwrap().enable_silent_start);

            let window_label = WindowLabel("main".into());
            let window_state = WindowState {
                width: 1024,
                height: 768,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            };
            let mut session_patch = PersistentState::new_empty_patch();
            session_patch.window_state = Some(BTreeMap::from([(
                window_label.clone(),
                window_state.clone(),
            )]));
            client
                .patch_session_state(session_patch)
                .await
                .expect("session patch should succeed");
            assert_eq!(
                client
                    .get_session_state()
                    .await
                    .unwrap()
                    .window_state
                    .get(&window_label),
                Some(&window_state)
            );

            client
                .replace_session_state(PersistentState::default())
                .await
                .expect("session replace should succeed");
            assert!(
                client
                    .get_session_state()
                    .await
                    .unwrap()
                    .window_state
                    .is_empty()
            );

            let mut clash_patch = ClashConfig::new_empty_patch();
            clash_patch.enable_tun_mode = Some(true);
            client
                .patch_clash_config(clash_patch)
                .await
                .expect("clash patch should succeed");
            assert!(client.get_clash_config().await.unwrap().enable_tun_mode);

            client
                .replace_clash_config(ClashConfig::default())
                .await
                .expect("clash replace should succeed");
            assert!(!client.get_clash_config().await.unwrap().enable_tun_mode);
        });
    }

    #[test]
    fn typed_setup_mirrors_loaded_state_to_legacy_bridges() {
        let dir = tempdir().expect("tempdir should be created");

        tauri::async_runtime::block_on(async {
            let (application, session_state, clash_config) = test_typed_config_clients(&dir).await;
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.theme_color = Some(serde_yaml::from_str("\"#123456\"").unwrap());
            application
                .patch(patch)
                .await
                .expect("typed application patch should persist");
            drop(application);
            drop(session_state);
            drop(clash_config);

            let mirrored_theme_color = Arc::new(StdMutex::new(None));
            let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
            let bridges = LegacyBridgeSet {
                verge: Arc::new(RecordingVergeBridge {
                    mirrored_theme_color: mirrored_theme_color.clone(),
                }),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            };

            let _loaded = new_typed_config_clients(paths, bridges)
                .await
                .expect("typed clients should load and mirror persisted state");

            assert_eq!(
                mirrored_theme_color
                    .lock()
                    .expect("mirror capture should not poison")
                    .as_deref(),
                Some("#123456")
            );
        });
    }

    #[test]
    fn try_new_with_args_constructs_typed_config_facade() {
        let dir = tempdir().expect("tempdir should be created");
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
            paths,
            runtime_paths,
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core: Some(test_core_port(Arc::new(MockRunningCoreBridge::new()))),
            binary_resolver: test_binary_resolver(&dir),
            degradation: test_degradation_sink(),
            service_control: test_service_control(),
            system_dns: Arc::new(NoopSystemDnsCache),
        })
        .expect("client should construct with typed config actors");

        tauri::async_runtime::block_on(async {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.enable_system_proxy = Some(true);
            client
                .patch_app_config(patch)
                .await
                .expect("typed app patch should succeed");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);
        });
    }

    #[test]
    fn runtime_lifecycle_is_empty_before_first_rebuild() {
        let dir = tempdir().unwrap();
        let client = tauri::async_runtime::block_on(test_client(&dir));
        let promoted = tauri::async_runtime::block_on(client.promoted_runtime());
        let lifecycle = client.inner.core_client.lifecycle();

        assert!(promoted.is_none());
        assert!(lifecycle.promoted.is_none());
        assert!(lifecycle.applied.is_none());
    }

    #[test]
    fn facade_add_activate_rebuilds_via_core_bridge() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().times(1).returning(|| Ok(()));
        core.expect_on_profile_change().times(1).returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();

        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            client
                .activate_profile(Some(uid.clone()))
                .await
                .expect("activate");
            let promoted = client
                .promoted_runtime()
                .await
                .expect("promoted runtime stored after rebuild");
            assert!(promoted.config.get("mixed-port").is_some());
            assert!(
                !promoted.exists_keys.is_empty(),
                "guard overrides must register applied fields"
            );
            let _ = promoted.postprocessing_output.clone();

            let lifecycle = client.inner.core_client.lifecycle();
            let applied = lifecycle
                .applied
                .as_ref()
                .expect("successful apply must publish Applied");
            assert!(applied.identity_eq(promoted.as_ref()));
            let path = client
                .get_profile_materialized_path(uid.clone())
                .await
                .unwrap();
            let expected_file = format!("{}.yaml", uid.0);
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(expected_file.as_str())
            );
            let content = client.read_profile_file(uid.clone()).await.unwrap();
            assert!(content.contains("proxies"));
            client
                .save_profile_file(uid.clone(), "proxies: []\nmode: direct\n".into())
                .await
                .unwrap();
        });
    }

    #[test]
    fn activate_returns_degraded_and_keeps_commit_when_rebuild_fails() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            let outcome = client
                .activate_profile(Some(uid.clone()))
                .await
                .expect("activate must commit");
            assert!(
                matches!(
                    outcome,
                    crate::client::runtime::MutationOutcome::CommittedDegraded { .. }
                ),
                "post-commit rebuild failure must be committed_degraded"
            );
            let degradations = outcome.degradations();
            assert_eq!(degradations.len(), 1);
            assert_eq!(
                degradations[0].phase,
                crate::client::runtime::DegradationPhase::RuntimeCheck
            );
            assert_eq!(degradations[0].code, "runtime_check_failed");
            assert!(degradations[0].retryable);
            assert!(degradations[0].message.contains("check boom"));
            let profiles = client.get_profiles().await.unwrap();
            assert_eq!(
                profiles.current.as_ref(),
                Some(&uid),
                "state stays committed"
            );
        });
    }

    #[test]
    fn background_rebuild_degradation_reaches_sink_and_sync_caller_keeps_it_in_outcome() {
        let dir = tempdir().unwrap();
        let (degradation_tx, mut degradation_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("background check failure")));
        let mut args = test_profiles_client_args(&dir, Arc::new(core));
        args.degradation = Arc::new(NotifyingCoreDegradationSink(degradation_tx));
        let client = NyanpasuClient::try_new_with_args(args).unwrap();

        tauri::async_runtime::block_on(async {
            client.rebuild_coordinator().notifier().request_rebuild();
            let degradation = tokio::time::timeout(Duration::from_secs(2), degradation_rx.recv())
                .await
                .expect("background rebuild must publish without polling sleeps")
                .expect("degradation sink must stay connected");
            assert_eq!(degradation.phase, runtime::DegradationPhase::RuntimeCheck);
            assert_eq!(degradation.code, "runtime_check_failed");
            assert!(degradation.message.contains("background check failure"));

            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .unwrap()
                .into_value();
            let outcome = client.activate_profile(Some(uid)).await.unwrap();
            assert!(matches!(
                outcome,
                runtime::MutationOutcome::CommittedDegraded { .. }
            ));
            assert_eq!(outcome.degradations().len(), 1);
            assert_eq!(outcome.degradations()[0].code, "runtime_check_failed");
            assert!(degradation_rx.try_recv().is_err());
            client.shutdown().await;
        });
    }

    #[test]
    fn legacy_regeneration_path_still_errors_on_rebuild_failure() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        let result = tauri::async_runtime::block_on(client.regenerate_runtime_for_legacy());
        assert!(
            result.is_err(),
            "legacy callers rely on Err to discard their drafts"
        );
    }

    #[test]
    fn rebuild_checks_and_promotes_before_core_apply() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_apply_config()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            client.activate_profile(Some(uid)).await.expect("activate");
        });
    }

    /// D5+P0-1 invariant: a failed check must leave the manager unpublished
    /// (product left untouched is proven by LegacyCoreBridge ordering + the
    /// promote atomicity unit test).
    #[test]
    fn failed_check_keeps_runtime_lifecycle_unpublished() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            // T8: a failed rebuild degrades (commit stays) instead of erroring;
            // the rejected candidate must still never reach readers.
            let _ = client.activate_profile(Some(uid)).await;
            let lifecycle = client.inner.core_client.lifecycle();
            assert!(
                client.promoted_runtime().await.is_none(),
                "a rejected candidate must never be published to readers"
            );
            assert!(lifecycle.promoted.is_none());
            assert!(lifecycle.applied.is_none());
        });
    }

    async fn seed_active_runtime(client: &NyanpasuClient) -> Arc<core_runtime::RuntimeSnapshot> {
        let uid = client
            .add_profile(
                minimal_file_profile_request(),
                Some("proxies: []\nmode: rule\n".into()),
            )
            .await
            .expect("add profile")
            .into_value();
        client
            .activate_profile(Some(uid))
            .await
            .expect("activate profile");
        client
            .inner
            .core_client
            .lifecycle()
            .applied
            .expect("initial rebuild must publish Applied")
    }

    fn allow_lan_patch() -> serde_yaml::Mapping {
        serde_yaml::from_str("allow-lan: true\n").unwrap()
    }

    fn assert_not_applied_degradation(
        outcome: &runtime::MutationOutcome<runtime::RuntimeApplyReport>,
        old_revision: u64,
        phase: runtime::DegradationPhase,
        code: &str,
    ) {
        assert!(matches!(
            outcome,
            runtime::MutationOutcome::CommittedDegraded { .. }
        ));
        assert_eq!(
            outcome.value().outcome,
            runtime::RuntimeApplyOutcome::NotApplied
        );
        assert!(outcome.value().desired_revision > old_revision);
        assert_eq!(outcome.value().applied_revision, None);
        assert_eq!(outcome.degradations().len(), 1);
        assert_eq!(outcome.degradations()[0].phase, phase);
        assert_eq!(outcome.degradations()[0].code, code);
    }

    fn scripted_apply(
        outcome: nyanpasu_ipc::api::core::apply::ApplyOutcomeKind,
        generation: u64,
    ) -> nyanpasu_ipc::api::core::apply::CoreApplyData {
        nyanpasu_ipc::api::core::apply::CoreApplyData {
            outcome,
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            },
            warning: None,
            failed_apply: None,
        }
    }

    #[test]
    fn change_core_acquire_failure_does_not_commit_desired_core() {
        struct BeginFailureCore;

        #[async_trait]
        impl CoreLifecyclePort for BeginFailureCore {
            async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
                anyhow::bail!("scripted acquire timeout")
            }

            async fn status(&self) -> anyhow::Result<core_bridge::CoreStatusSnapshot> {
                anyhow::bail!("status is not used")
            }

            async fn on_profile_change(&self) {}
        }

        let dir = tempdir().unwrap();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            Arc::new(BeginFailureCore),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            let before = client.get_app_config().await.unwrap().core;
            let error = client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .unwrap_err();

            assert!(error.to_string().contains("acquire timeout"));
            assert_eq!(client.get_app_config().await.unwrap().core, before);
        });
    }

    #[tokio::test]
    async fn change_core_build_failure_commits_desired_and_reports_not_applied() {
        let dir = tempdir().unwrap();
        let client =
            actor_backed_test_client(&dir, facade_backend(), Arc::new(NoopCoreDegradationSink))
                .await;
        let uid = client
            .add_profile(minimal_file_profile_request(), Some("proxies: [".into()))
            .await
            .unwrap()
            .into_value();
        let activation = client.activate_profile(Some(uid)).await.unwrap();
        assert!(matches!(
            activation,
            runtime::MutationOutcome::CommittedDegraded { .. }
        ));

        let outcome = client
            .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
            .await
            .unwrap();

        assert_eq!(
            client.get_app_config().await.unwrap().core,
            nyanpasu_config::application::ClashCore::ClashRs
        );
        assert_not_applied_degradation(
            &outcome,
            0,
            runtime::DegradationPhase::RuntimeBuild,
            "runtime_build_failed",
        );
    }

    #[test]
    fn change_core_check_failure_commits_desired_without_promoting() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .once()
            .returning(|_, _| Err(anyhow::anyhow!("scripted change-core check failure")));
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let outcome = client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .unwrap();

            assert_eq!(
                client.get_app_config().await.unwrap().core,
                nyanpasu_config::application::ClashCore::ClashRs
            );
            assert_not_applied_degradation(
                &outcome,
                0,
                runtime::DegradationPhase::RuntimeCheck,
                "runtime_check_failed",
            );
            assert!(client.inner.core_client.lifecycle().promoted.is_none());
        });
    }

    #[tokio::test]
    async fn change_core_transport_loss_is_committed_degraded_and_preserves_applied() {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        let client =
            actor_backed_test_client(&dir, backend.clone(), Arc::new(NoopCoreDegradationSink))
                .await;
        let old_applied = seed_active_runtime(&client).await;
        let ipc = nyanpasu_ipc::client::Client::new("change-core-transport-test").unwrap();
        for _ in 0..5 {
            let source = ipc.http_client().get("::::").build().unwrap_err();
            backend.push_apply_result(Err(crate::core::actor::backend::CoreBackendError::Service(
                nyanpasu_ipc::client::ClientError::Request {
                    operation: "apply",
                    source,
                },
            )));
        }

        let outcome = client
            .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
            .await
            .unwrap();

        assert_not_applied_degradation(
            &outcome,
            old_applied.revision.get(),
            runtime::DegradationPhase::RuntimeApply,
            "core_transport_lost",
        );
        assert_eq!(
            client.get_app_config().await.unwrap().core,
            nyanpasu_config::application::ClashCore::ClashRs
        );
        let lifecycle = client.inner.core_client.lifecycle();
        assert!(lifecycle.applied.unwrap().identity_eq(&old_applied));
        assert!(lifecycle.promoted.unwrap().revision > old_applied.revision);
    }

    #[tokio::test]
    async fn change_core_rolled_back_keeps_old_applied_without_application_rollback() {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        let client =
            actor_backed_test_client(&dir, backend.clone(), Arc::new(NoopCoreDegradationSink))
                .await;
        let old_applied = seed_active_runtime(&client).await;
        backend.push_apply_result(Ok(scripted_apply(
            nyanpasu_ipc::api::core::apply::ApplyOutcomeKind::RolledBack,
            old_applied.revision.get(),
        )));

        let outcome = client
            .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            runtime::MutationOutcome::CommittedDegraded { .. }
        ));
        assert_eq!(
            outcome.value().outcome,
            runtime::RuntimeApplyOutcome::RolledBack
        );
        assert_eq!(
            outcome.value().applied_revision,
            Some(old_applied.revision.get())
        );
        assert_eq!(outcome.degradations()[0].code, "core_rollback");
        assert_eq!(
            client.get_app_config().await.unwrap().core,
            nyanpasu_config::application::ClashCore::ClashRs
        );
        let lifecycle = client.inner.core_client.lifecycle();
        let promoted = lifecycle.promoted.unwrap();
        let applied = lifecycle.applied.unwrap();
        assert_eq!(
            promoted.target_core,
            nyanpasu_config::application::ClashCore::ClashRs
        );
        assert!(promoted.revision > old_applied.revision);
        assert!(applied.identity_eq(&old_applied));
    }

    #[tokio::test]
    async fn change_core_running_switches_via_apply_without_restart() {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        let client =
            actor_backed_test_client(&dir, backend.clone(), Arc::new(NoopCoreDegradationSink))
                .await;
        seed_active_runtime(&client).await;
        client.restart_core().await.unwrap();
        let prior_runs = backend.run_calls();
        let prior_applies = backend.apply_calls();
        backend.push_apply_result(Ok(scripted_apply(
            nyanpasu_ipc::api::core::apply::ApplyOutcomeKind::Switched,
            2,
        )));

        let outcome = client
            .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
            .await
            .unwrap();

        assert!(matches!(outcome, runtime::MutationOutcome::Applied { .. }));
        assert_eq!(
            outcome.value().outcome,
            runtime::RuntimeApplyOutcome::Switched
        );
        assert_eq!(backend.run_calls(), prior_runs);
        assert_eq!(backend.apply_calls(), prior_applies + 1);
        let lifecycle = client.inner.core_client.lifecycle();
        let promoted = lifecycle.promoted.unwrap();
        let applied = lifecycle.applied.unwrap();
        assert!(applied.identity_eq(&promoted));
        assert_eq!(
            promoted.target_core,
            nyanpasu_config::application::ClashCore::ClashRs
        );
        assert_eq!(
            outcome.value().applied_revision,
            Some(promoted.revision.get())
        );
    }

    #[tokio::test]
    async fn change_core_stopped_starts_promoted_core_once() {
        let dir = tempdir().unwrap();
        let backend = crate::core::actor::backend::TestBackend::new(facade_observation(
            crate::core::actor::types::FaithfulLifecycle::Stopped { reason: None },
        ));
        let client =
            actor_backed_test_client(&dir, backend.clone(), Arc::new(NoopCoreDegradationSink))
                .await;

        let outcome = client
            .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
            .await
            .unwrap();

        assert!(matches!(outcome, runtime::MutationOutcome::Applied { .. }));
        assert_eq!(
            outcome.value().outcome,
            runtime::RuntimeApplyOutcome::Started
        );
        assert_eq!(backend.apply_calls(), 0);
        assert_eq!(backend.run_calls(), 1);
        assert_eq!(
            backend.run_requests()[0].core_type,
            nyanpasu_utils::core::CoreType::Clash(nyanpasu_utils::core::ClashCoreType::ClashRust)
        );
        let lifecycle = client.inner.core_client.lifecycle();
        let promoted = lifecycle.promoted.unwrap();
        let applied = lifecycle.applied.unwrap();
        assert!(applied.identity_eq(&promoted));
        assert_eq!(
            outcome.value().applied_revision,
            Some(promoted.revision.get())
        );
    }

    #[tokio::test]
    async fn change_core_stopped_start_failure_is_committed_degraded() {
        let dir = tempdir().unwrap();
        let backend = crate::core::actor::backend::TestBackend::new(facade_observation(
            crate::core::actor::types::FaithfulLifecycle::Stopped { reason: None },
        ));
        backend.fail_next_run();
        let client =
            actor_backed_test_client(&dir, backend.clone(), Arc::new(NoopCoreDegradationSink))
                .await;

        let outcome = client
            .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            runtime::MutationOutcome::CommittedDegraded { .. }
        ));
        assert_eq!(
            outcome.value().outcome,
            runtime::RuntimeApplyOutcome::NotApplied
        );
        assert_eq!(outcome.value().applied_revision, None);
        assert!(outcome.value().desired_revision > 0);
        assert_eq!(outcome.degradations().len(), 1);
        assert_eq!(
            outcome.degradations()[0].phase,
            runtime::DegradationPhase::CoreLifecycle
        );
        assert_eq!(outcome.degradations()[0].code, "core_start_failed");
        assert_eq!(
            client.get_app_config().await.unwrap().core,
            nyanpasu_config::application::ClashCore::ClashRs
        );
        let lifecycle = client.inner.core_client.lifecycle();
        assert_eq!(
            lifecycle.promoted.unwrap().target_core,
            nyanpasu_config::application::ClashCore::ClashRs
        );
        assert!(lifecycle.applied.is_none());
    }

    #[test]
    fn change_core_stopped_exempt_restart_failure_returns_plain_error() {
        let dir = tempdir().unwrap();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            Arc::new(ExemptRestartCore),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            let error = client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .expect_err("stale operation must stay a plain error");

            let ClientError::Anyhow(error) = error else {
                panic!("stale operation must stay an actor-backed error");
            };
            assert!(matches!(
                error.downcast_ref(),
                Some(crate::core::actor::types::CoreActorError::StaleOperation)
            ));
        });
    }

    #[tokio::test]
    async fn change_core_uses_faithful_lifecycle_for_attach_and_transition_states() {
        use crate::core::actor::types::FaithfulLifecycle;

        let cases = [
            (FaithfulLifecycle::Running, true),
            (FaithfulLifecycle::Starting, true),
            (FaithfulLifecycle::Restarting, true),
            (FaithfulLifecycle::Stopping, false),
        ];
        for (lifecycle, should_apply) in cases {
            let dir = tempdir().unwrap();
            let backend =
                crate::core::actor::backend::TestBackend::new(facade_observation(lifecycle));
            if should_apply {
                backend.push_apply_result(Ok(scripted_apply(
                    nyanpasu_ipc::api::core::apply::ApplyOutcomeKind::Switched,
                    1,
                )));
            }
            let client =
                actor_backed_test_client(&dir, backend.clone(), Arc::new(NoopCoreDegradationSink))
                    .await;

            let outcome = client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .unwrap();

            if should_apply {
                assert_eq!(backend.apply_calls(), 1);
                assert_eq!(backend.run_calls(), 0);
                assert_eq!(
                    outcome.value().outcome,
                    runtime::RuntimeApplyOutcome::Switched
                );
            } else {
                assert_eq!(backend.apply_calls(), 0);
                assert_eq!(backend.run_calls(), 1);
                assert_eq!(
                    outcome.value().outcome,
                    runtime::RuntimeApplyOutcome::Started
                );
            }
        }
    }

    #[test]
    fn patch_promote_prewrite_failure_keeps_product_and_reports_not_applied() {
        let dir = tempdir().unwrap();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            promote_failure_core(PromoteFailurePoint::BeforeWrite),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            let old_applied = seed_active_runtime(&client).await;
            let old_product = tokio::fs::read(client.runtime_product_path())
                .await
                .unwrap();

            let outcome = client
                .patch_running_config(allow_lan_patch())
                .await
                .unwrap();

            assert_not_applied_degradation(
                &outcome,
                old_applied.revision.get(),
                runtime::DegradationPhase::RuntimePromote,
                "runtime_promote_failed",
            );
            assert_eq!(
                tokio::fs::read(client.runtime_product_path())
                    .await
                    .unwrap(),
                old_product
            );
            let lifecycle = client.inner.core_client.lifecycle();
            assert!(
                lifecycle
                    .promoted
                    .as_ref()
                    .is_some_and(|promoted| promoted.identity_eq(&old_applied))
            );
            assert!(
                lifecycle
                    .applied
                    .as_ref()
                    .is_some_and(|applied| applied.identity_eq(&old_applied))
            );
            assert!(
                outcome.degradations()[0]
                    .message
                    .contains("before product write")
            );
        });
    }

    #[test]
    fn patch_promote_postwrite_failure_self_heals_on_next_rebuild() {
        let dir = tempdir().unwrap();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            promote_failure_core(PromoteFailurePoint::AfterWrite),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            let old_applied = seed_active_runtime(&client).await;
            let old_product = tokio::fs::read(client.runtime_product_path())
                .await
                .unwrap();

            let outcome = client
                .patch_running_config(allow_lan_patch())
                .await
                .unwrap();

            assert_not_applied_degradation(
                &outcome,
                old_applied.revision.get(),
                runtime::DegradationPhase::RuntimePromote,
                "runtime_promote_failed",
            );
            let written_product = tokio::fs::read(client.runtime_product_path())
                .await
                .unwrap();
            assert_ne!(written_product, old_product);
            let split_lifecycle = client.inner.core_client.lifecycle();
            assert!(
                split_lifecycle
                    .promoted
                    .as_ref()
                    .is_some_and(|promoted| promoted.identity_eq(&old_applied))
            );
            assert!(
                outcome.degradations()[0]
                    .message
                    .contains("after product write")
            );

            client.rebuild_running_config().await.unwrap();

            let healed = client.inner.core_client.lifecycle();
            let promoted = healed.promoted.expect("self-heal must publish Promoted");
            let applied = healed.applied.expect("self-heal must publish Applied");
            assert!(applied.identity_eq(&promoted));
            assert_eq!(promoted.product_bytes(), written_product);
            assert_eq!(
                tokio::fs::read(client.runtime_product_path())
                    .await
                    .unwrap(),
                promoted.product_bytes()
            );
        });
    }

    async fn assert_patch_apply_backend_failure(
        errors: Vec<crate::core::actor::backend::CoreBackendError>,
        expected_code: &str,
    ) {
        let dir = tempdir().unwrap();
        let backend = facade_backend();
        let client =
            actor_backed_test_client(&dir, backend.clone(), Arc::new(NoopCoreDegradationSink))
                .await;
        let old_applied = seed_active_runtime(&client).await;
        for error in errors {
            backend.push_apply_result(Err(error));
        }

        let outcome = client
            .patch_running_config(allow_lan_patch())
            .await
            .unwrap();

        assert_not_applied_degradation(
            &outcome,
            old_applied.revision.get(),
            runtime::DegradationPhase::RuntimeApply,
            expected_code,
        );
        let lifecycle = client.inner.core_client.lifecycle();
        let promoted = lifecycle.promoted.expect("candidate must stay Promoted");
        let applied = lifecycle.applied.expect("old Applied must be retained");
        assert_eq!(promoted.revision.get(), outcome.value().desired_revision);
        assert!(promoted.revision > old_applied.revision);
        assert!(applied.identity_eq(&old_applied));
    }

    #[test]
    fn patch_revision_conflict_is_committed_degraded_and_preserves_applied() {
        tauri::async_runtime::block_on(async {
            let expected = nyanpasu_core_manager::RevisionId {
                epoch: 1,
                generation: 2,
                effective_hash: "expected".into(),
            };
            assert_patch_apply_backend_failure(
                vec![crate::core::actor::backend::CoreBackendError::Local(
                    nyanpasu_core_manager::Error::RevisionConflict {
                        expected,
                        actual: None,
                    },
                )],
                "revision_conflict",
            )
            .await;
        });
    }

    #[test]
    fn patch_transport_loss_is_committed_degraded_and_preserves_applied() {
        tauri::async_runtime::block_on(async {
            let ipc = nyanpasu_ipc::client::Client::new("post-commit-transport-test").unwrap();
            let transport_failures = (0..5)
                .map(|_| {
                    let source = ipc.http_client().get("::::").build().unwrap_err();
                    crate::core::actor::backend::CoreBackendError::Service(
                        nyanpasu_ipc::client::ClientError::Request {
                            operation: "apply",
                            source,
                        },
                    )
                })
                .collect();
            assert_patch_apply_backend_failure(transport_failures, "core_transport_lost").await;
        });
    }

    #[test]
    fn patch_backend_apply_error_is_committed_degraded_and_preserves_applied() {
        tauri::async_runtime::block_on(async {
            assert_patch_apply_backend_failure(
                vec![crate::core::actor::backend::CoreBackendError::Construct(
                    anyhow::anyhow!("scripted apply failure"),
                )],
                "runtime_apply_failed",
            )
            .await;
        });
    }

    #[test]
    fn post_commit_exemption_boundary_keeps_backend_failures_degraded() {
        struct ErrorEventCounter(Arc<AtomicUsize>);

        impl<S> tracing_subscriber::Layer<S> for ErrorEventCounter
        where
            S: tracing::Subscriber,
        {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _context: tracing_subscriber::layer::Context<'_, S>,
            ) {
                if *event.metadata().level() == tracing::Level::ERROR {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        let shutting_down = NyanpasuClient::apply_failure_degradation(
            crate::core::actor::types::CoreActorError::ShuttingDown,
        );
        assert!(shutting_down.is_err());

        let errors = Arc::new(AtomicUsize::new(0));
        let subscriber = {
            use tracing_subscriber::prelude::*;
            tracing_subscriber::registry().with(ErrorEventCounter(errors.clone()))
        };
        let invariant = tracing::subscriber::with_default(subscriber, || {
            NyanpasuClient::apply_failure_degradation(
                crate::core::actor::types::CoreActorError::LifecycleInvariant(
                    crate::core::actor::types::LifecycleInvariantKind::PromotedRegression,
                ),
            )
        });
        assert!(invariant.is_err());
        assert_eq!(errors.load(Ordering::SeqCst), 1);

        let backend = NyanpasuClient::apply_failure_degradation(
            crate::core::actor::types::CoreActorError::Backend(Arc::new(
                crate::core::actor::backend::CoreBackendError::Construct(anyhow::anyhow!(
                    "backend apply failure"
                )),
            )),
        )
        .unwrap();
        let backend_outcome = runtime::MutationOutcome::from_parts(
            NyanpasuClient::not_applied_report(core_runtime::RuntimeRevision(41)),
            vec![backend],
        );
        assert!(matches!(
            backend_outcome,
            runtime::MutationOutcome::CommittedDegraded { .. }
        ));
        assert_eq!(
            backend_outcome.value().outcome,
            runtime::RuntimeApplyOutcome::NotApplied
        );

        let no_backend = NyanpasuClient::apply_failure_degradation(
            crate::core::actor::types::CoreActorError::NoBackend {
                last_error: Arc::new(crate::core::actor::backend::CoreBackendError::Construct(
                    anyhow::anyhow!("backend unavailable"),
                )),
            },
        )
        .unwrap();
        let no_backend_outcome = runtime::MutationOutcome::from_parts(
            NyanpasuClient::not_applied_report(core_runtime::RuntimeRevision(42)),
            vec![no_backend],
        );
        assert!(matches!(
            no_backend_outcome,
            runtime::MutationOutcome::CommittedDegraded { .. }
        ));
        assert_eq!(
            no_backend_outcome.degradations()[0].code,
            "core_backend_unavailable"
        );
        assert_eq!(
            no_backend_outcome.value().outcome,
            runtime::RuntimeApplyOutcome::NotApplied
        );
    }

    #[test]
    fn enhance_profiles_failures_are_plain_errors_and_do_not_publish_degradations() {
        let build_dir = tempdir().unwrap();
        let build_sink = Arc::new(RecordingCoreDegradationSink::default());
        let build_client = tauri::async_runtime::block_on(actor_backed_test_client(
            &build_dir,
            facade_backend(),
            build_sink.clone(),
        ));
        tauri::async_runtime::block_on(async {
            let uid = build_client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .unwrap()
                .into_value();
            build_client
                .activate_profile(Some(uid.clone()))
                .await
                .unwrap();
            let materialized = build_client
                .get_profile_materialized_path(uid)
                .await
                .unwrap();
            tokio::fs::remove_file(materialized).await.unwrap();
            assert!(build_client.rebuild_running_config().await.is_err());
        });
        assert!(build_sink.0.lock().unwrap().is_empty());

        let check_dir = tempdir().unwrap();
        let check_sink = Arc::new(RecordingCoreDegradationSink::default());
        let mut check_core = MockRunningCoreBridge::new();
        check_core
            .expect_check_and_promote()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("scripted check failure")));
        let mut check_args = test_profiles_client_args(&check_dir, Arc::new(check_core));
        check_args.degradation = check_sink.clone();
        let check_client = NyanpasuClient::try_new_with_args(check_args).unwrap();
        assert!(tauri::async_runtime::block_on(check_client.rebuild_running_config()).is_err());
        assert!(check_sink.0.lock().unwrap().is_empty());

        let apply_dir = tempdir().unwrap();
        let apply_sink = Arc::new(RecordingCoreDegradationSink::default());
        let mut apply_core = MockRunningCoreBridge::new();
        apply_core
            .expect_check_and_promote()
            .times(1)
            .returning(|_, _| Ok(()));
        apply_core
            .expect_apply_config()
            .times(1)
            .returning(|| Err(anyhow::anyhow!("scripted apply failure")));
        let mut apply_args = test_profiles_client_args(&apply_dir, Arc::new(apply_core));
        apply_args.degradation = apply_sink.clone();
        let apply_client = NyanpasuClient::try_new_with_args(apply_args).unwrap();
        assert!(tauri::async_runtime::block_on(apply_client.rebuild_running_config()).is_err());
        assert!(apply_sink.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn legacy_apply_rolled_back_is_a_plain_error_without_sink_degradation() {
        let dir = tempdir().unwrap();
        let sink = Arc::new(RecordingCoreDegradationSink::default());
        let backend = facade_backend();
        backend.push_apply_result(Ok(nyanpasu_ipc::api::core::apply::CoreApplyData {
            outcome: nyanpasu_ipc::api::core::apply::ApplyOutcomeKind::RolledBack,
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 17,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            },
            warning: None,
            failed_apply: None,
        }));
        let client = actor_backed_test_client(&dir, backend, sink.clone()).await;

        let error = client
            .regenerate_and_apply_for_legacy()
            .await
            .expect_err("RolledBack must make legacy draft callers discard");

        assert!(error.to_string().contains("rolled back"));
        assert!(sink.0.lock().unwrap().is_empty());
        let lifecycle = client.inner.core_client.lifecycle();
        assert!(lifecycle.promoted.is_some());
        assert!(lifecycle.applied.is_none());
    }

    fn test_runtime_snapshot(
        revision: u64,
        bytes: &'static [u8],
    ) -> Arc<core_runtime::RuntimeSnapshot> {
        let config = serde_yaml::from_slice(bytes).expect("test runtime must be valid YAML");
        Arc::new(core_runtime::RuntimeSnapshot::from_data(
            core_runtime::RuntimeRevision(revision),
            nyanpasu_config::application::ClashCore::Mihomo,
            Arc::from(bytes),
            core_runtime::RuntimeSnapshotData {
                config,
                exists_keys: Vec::new(),
                postprocessing_output: Default::default(),
            },
        ))
    }

    fn candidate_tun_enabled(bytes: &[u8]) -> Option<bool> {
        let config: serde_yaml::Mapping =
            serde_yaml::from_slice(bytes).expect("candidate must be valid YAML");
        let tun = config
            .get(serde_yaml::Value::String("tun".into()))
            .and_then(serde_yaml::Value::as_mapping)?;
        tun.get(serde_yaml::Value::String("enable".into()))
            .and_then(serde_yaml::Value::as_bool)
    }

    #[test]
    fn concurrent_rebuilds_serialize_and_second_reads_latest_snapshot() {
        let dir = tempdir().unwrap();
        let state = Arc::new(OperationProbeState::new());
        let (first_check_entered_tx, first_check_entered_rx) = tokio::sync::oneshot::channel();
        let (release_first_check_tx, release_first_check_rx) = tokio::sync::oneshot::channel();
        let (second_begin_attempted_tx, second_begin_attempted_rx) =
            tokio::sync::oneshot::channel();
        *state.first_check_entered_tx.lock().unwrap() = Some(first_check_entered_tx);
        *state.release_first_check_rx.lock().unwrap() = Some(release_first_check_rx);
        *state.second_begin_attempted_tx.lock().unwrap() = Some(second_begin_attempted_tx);
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            operation_probe_core(state.clone()),
        ))
        .unwrap();

        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            let first = tauri::async_runtime::spawn({
                let client = client.clone();
                async move { client.activate_profile(Some(uid)).await }
            });
            first_check_entered_rx
                .await
                .expect("first rebuild must reach its check while holding the operation guard");

            let mut latest = client.get_clash_config().await.unwrap();
            assert!(!latest.enable_tun_mode);
            latest.enable_tun_mode = true;
            client
                .replace_clash_config(latest)
                .await
                .expect("new desired snapshot must commit while the first rebuild is blocked");

            let second = tauri::async_runtime::spawn({
                let client = client.clone();
                async move { client.rebuild_running_config().await }
            });
            second_begin_attempted_rx
                .await
                .expect("second rebuild must queue on the operation guard");
            assert_eq!(state.check_calls.load(Ordering::SeqCst), 1);
            assert_eq!(state.candidates.lock().unwrap().len(), 1);

            let _ = release_first_check_tx.send(());
            first
                .await
                .expect("first rebuild task must join")
                .expect("first rebuild must succeed");
            second
                .await
                .expect("second rebuild task must join")
                .expect("second rebuild must succeed");

            assert_eq!(state.max_active_checks.load(Ordering::SeqCst), 1);
            let candidates = state.candidates.lock().unwrap();
            assert_eq!(candidates.len(), 2);
            assert_ne!(candidate_tun_enabled(&candidates[0]), Some(true));
            assert_eq!(
                candidate_tun_enabled(&candidates[1]),
                Some(true),
                "the queued rebuild must read the post-queue desired snapshot"
            );
        });
    }

    #[test]
    fn promote_default_builds_candidate_after_operation_guard() {
        let dir = tempdir().unwrap();
        let state = Arc::new(OperationProbeState::new());
        let (begin_entered_tx, begin_entered_rx) = tokio::sync::oneshot::channel();
        let (release_begin_tx, release_begin_rx) = tokio::sync::oneshot::channel();
        *state.begin_entered_tx.lock().unwrap() = Some(begin_entered_tx);
        *state.release_begin_rx.lock().unwrap() = Some(release_begin_rx);
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            operation_probe_core(state.clone()),
        ))
        .unwrap();
        let candidate_dir = client.inner.runtime_paths.candidate_dir().to_owned();

        tauri::async_runtime::block_on(async {
            let promote = tauri::async_runtime::spawn({
                let client = client.clone();
                async move { client.promote_default_runtime_config().await }
            });
            begin_entered_rx
                .await
                .expect("default promotion must begin the operation first");
            let candidates_before_release = std::fs::read_dir(&candidate_dir)
                .map(|entries| {
                    entries
                        .filter_map(std::result::Result::ok)
                        .filter(|entry| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .starts_with("candidate-")
                        })
                        .count()
                })
                .unwrap_or(0);
            assert_eq!(candidates_before_release, 0);
            assert_eq!(state.check_calls.load(Ordering::SeqCst), 0);
            let _ = release_begin_tx.send(());
            promote
                .await
                .expect("default promotion task must join")
                .expect("default promotion must succeed");
            assert_eq!(state.check_calls.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn promote_existing_reads_product_after_operation_guard() {
        let dir = tempdir().unwrap();
        let state = Arc::new(OperationProbeState::new());
        let (begin_entered_tx, begin_entered_rx) = tokio::sync::oneshot::channel();
        let (release_begin_tx, release_begin_rx) = tokio::sync::oneshot::channel();
        *state.begin_entered_tx.lock().unwrap() = Some(begin_entered_tx);
        *state.release_begin_rx.lock().unwrap() = Some(release_begin_rx);
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            operation_probe_core(state.clone()),
        ))
        .unwrap();
        let product = client.runtime_product_path().to_owned();

        tauri::async_runtime::block_on(async {
            let promote = tauri::async_runtime::spawn({
                let client = client.clone();
                async move { client.promote_existing_runtime_product().await }
            });
            begin_entered_rx
                .await
                .expect("existing-product promotion must begin the operation first");
            assert!(!product.exists());
            tokio::fs::create_dir_all(product.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(&product, b"mode: rule\n").await.unwrap();
            let _ = release_begin_tx.send(());
            let promoted = promote
                .await
                .expect("existing-product promotion task must join")
                .expect("product created after begin must be observed");
            assert_eq!(promoted.product_bytes(), b"mode: rule\n");
            assert_eq!(state.check_calls.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn start_promoted_reads_latest_snapshot_after_operation_guard() {
        let dir = tempdir().unwrap();
        let state = Arc::new(OperationProbeState::new());
        let (begin_entered_tx, begin_entered_rx) = tokio::sync::oneshot::channel();
        let (release_begin_tx, release_begin_rx) = tokio::sync::oneshot::channel();
        *state.begin_entered_tx.lock().unwrap() = Some(begin_entered_tx);
        *state.release_begin_rx.lock().unwrap() = Some(release_begin_rx);
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_lifecycle(
            &dir,
            operation_probe_core(state.clone()),
        ))
        .unwrap();

        tauri::async_runtime::block_on(async {
            let old = test_runtime_snapshot(1, b"mode: rule\n");
            let operation = client.inner.core_client.begin_operation().await.unwrap();
            client
                .inner
                .core_client
                .publish_promoted(&operation, old)
                .await
                .unwrap();
            operation.release().await;

            let start = tauri::async_runtime::spawn({
                let client = client.clone();
                async move { client.start_promoted_runtime().await }
            });
            begin_entered_rx
                .await
                .expect("start must begin the operation before reading Promoted");

            let newest = test_runtime_snapshot(2, b"mode: global\n");
            let operation = client.inner.core_client.begin_operation().await.unwrap();
            client
                .inner
                .core_client
                .publish_promoted(&operation, newest)
                .await
                .unwrap();
            operation.release().await;
            let _ = release_begin_tx.send(());
            start
                .await
                .expect("start task must join")
                .expect("start must succeed");

            assert_eq!(
                state.applied_revisions.lock().unwrap().as_slice(),
                &[2],
                "start must associate Applied with the Promoted snapshot read after begin"
            );
        });
    }

    #[test]
    fn apply_failure_advances_promoted_but_preserves_applied() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_apply_config()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_apply_config()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("apply boom")));
        core.expect_on_profile_change().times(1).returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();

        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            client
                .activate_profile(Some(uid))
                .await
                .expect("initial apply");

            let before = client.inner.core_client.lifecycle();
            let old_applied = before.applied.expect("initial Applied");
            assert!(old_applied.identity_eq(before.promoted.as_deref().expect("initial Promoted")));

            let error = client
                .rebuild_running_config()
                .await
                .expect_err("second apply must fail");
            assert!(error.to_string().contains("apply boom"));

            let after = client.inner.core_client.lifecycle();
            let promoted = after.promoted.expect("second Promoted");
            let applied = after.applied.expect("previous Applied retained");
            assert!(promoted.revision > old_applied.revision);
            assert!(applied.identity_eq(old_applied.as_ref()));
            assert!(!applied.identity_eq(promoted.as_ref()));
        });
    }

    #[test]
    fn boot_repromotes_existing_product_then_publishes_applied() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        let product = client.runtime_product_path().to_owned();
        let bytes = b"# previous session runtime\nmode: rule\n";
        std::fs::create_dir_all(product.parent().unwrap()).unwrap();
        std::fs::write(&product, bytes).unwrap();

        tauri::async_runtime::block_on(async {
            let promoted = client.promote_existing_runtime_product().await.unwrap();
            assert_eq!(
                promoted.product_sha256,
                <[u8; 32]>::from(Sha256::digest(bytes))
            );
            assert_eq!(promoted.config.get("mode"), Some(&"rule".into()));

            client.start_promoted_runtime().await.unwrap();

            let lifecycle = client.inner.core_client.lifecycle();
            assert!(
                lifecycle
                    .applied
                    .as_deref()
                    .is_some_and(|applied| applied.identity_eq(promoted.as_ref()))
            );
        });
    }

    #[test]
    fn facade_import_downloads_and_conditionally_activates() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                // No server name: exercises the url last-segment fallback below.
                filename: None,
                suggested_update_interval_minutes: Some(360),
            })
        });
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let url = url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap();
            let mut patch = RemoteProfileOptions::new_empty_patch();
            patch.with_proxy = Some(false);
            let uid = client
                .import_profile(url, None, Some(patch))
                .await
                .expect("import")
                .into_value();
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(
                snapshot.current.as_ref(),
                Some(&uid),
                "empty current must auto-activate"
            );
            let item = &snapshot.items[&uid];
            assert_eq!(item.metadata.name, "my-sub"); // url last-segment fallback naming
            let source = item.definition.source().unwrap();
            assert!(source.is_remote());
            let ProfileSource::Remote { option, .. } = source else {
                unreachable!()
            };
            assert_eq!(option.update_interval_minutes, 360);
            assert!(!option.with_proxy);
        });
    }

    #[test]
    fn facade_import_keeps_explicit_interval_over_server_suggestion() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                filename: None,
                suggested_update_interval_minutes: Some(360),
            })
        });
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let mut patch = RemoteProfileOptions::new_empty_patch();
            patch.update_interval_minutes = Some(45);
            let url = url::Url::parse("https://example.com/subs/explicit.yaml").unwrap();
            let uid = client
                .import_profile(url, None, Some(patch))
                .await
                .expect("import")
                .into_value();
            let snapshot = client.get_profiles().await.unwrap();
            let ProfileSource::Remote { option, .. } =
                snapshot.items[&uid].definition.source().unwrap()
            else {
                unreachable!()
            };
            assert_eq!(option.update_interval_minutes, 45);
        });
    }

    #[test]
    fn facade_import_rejects_explicit_zero_interval_before_fetch() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(0);
        let core = MockRunningCoreBridge::new();

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let mut patch = RemoteProfileOptions::new_empty_patch();
            patch.update_interval_minutes = Some(0);
            let url = url::Url::parse("https://example.com/subs/invalid.yaml").unwrap();
            assert!(client.import_profile(url, None, Some(patch)).await.is_err());
            assert!(client.get_profiles().await.unwrap().items.is_empty());
        });
    }

    fn local_config_request(name: &str) -> NewProfileRequest {
        NewProfileRequest {
            metadata: ProfileMetadata {
                name: name.into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("pending.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    fn remote_config_request() -> NewProfileRequest {
        NewProfileRequest {
            metadata: ProfileMetadata {
                name: "remote".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Remote {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("pending.yaml").unwrap(),
                            updated_at: None,
                        },
                        url: url::Url::parse("https://example.com/sub").unwrap(),
                        option: RemoteProfileOptions::default(),
                        subscription: SubscriptionInfo::default(),
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    #[test]
    fn facade_import_failure_commits_nothing() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher
            .expect_fetch()
            .returning(|_, _| anyhow::bail!("dns exploded"));
        // A failed import never reaches core apply, so the bridge expects nothing.
        let core = MockRunningCoreBridge::new();

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let url = url::Url::parse("https://example.com/subs/x.yaml").unwrap();
            let result = client.import_profile(url, None, None).await;
            assert!(
                result.is_err(),
                "import must fail when the first download fails"
            );
            let snapshot = client.get_profiles().await.unwrap();
            assert!(
                snapshot.items.is_empty(),
                "fetch-before-commit must leave zero durable items on download failure"
            );
        });
    }

    #[test]
    fn facade_add_profile_rejects_remote_before_persist() {
        let dir = tempdir().unwrap();
        // No fetcher/core activity: the remote shell must be rejected at the
        // public facade boundary before ProfilesClient::add is reached.
        let client = tauri::async_runtime::block_on(test_client(&dir));

        tauri::async_runtime::block_on(async {
            let rejected = client.add_profile(remote_config_request(), None).await;
            match rejected {
                Err(ClientError::Custom(message)) => {
                    assert!(
                        message.contains("import_profile"),
                        "stable rejection must direct callers to import_profile: {message}"
                    );
                }
                other => panic!("expected Custom(import_profile) rejection, got {other:?}"),
            }
            let snapshot = client.get_profiles().await.unwrap();
            assert!(
                snapshot.items.is_empty(),
                "direct remote add must leave zero durable items"
            );
            assert!(snapshot.current.is_none());
        });
    }

    #[test]
    fn facade_create_auto_activates_config_and_rejects_remote() {
        let dir = tempdir().unwrap();
        let fetcher = MockSubscriptionFetcher::new();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;

            // create_profile shares the public add_profile remote guard.
            let rejected = client.create_profile(remote_config_request(), None).await;
            assert!(
                matches!(rejected, Err(ClientError::Custom(message)) if message.contains("import_profile")),
                "create must reject remote sources via the add_profile guard"
            );
            assert!(
                client.get_profiles().await.unwrap().items.is_empty(),
                "rejected remote create must not persist"
            );

            // A local Config with no current selection auto-activates (design §9).
            let uid = client
                .create_profile(local_config_request("local"), Some("proxies: []\n".into()))
                .await
                .expect("create local config")
                .into_value();
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(
                snapshot.current.as_ref(),
                Some(&uid),
                "an empty current must auto-activate the new Config profile"
            );
        });
    }

    /// H2 E2E (Unix): Add commits, then post-commit `set_current_if_none` state
    /// persistence fails via the production materialization `complete` seam
    /// permission-poisoning the profiles parent. create_profile must return
    /// `Ok(CommittedDegraded)` with the real ProfileId and keep current empty.
    #[cfg(unix)]
    #[test]
    fn facade_create_auto_activation_persist_failure_is_committed_degraded() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let parent = dir.path().to_path_buf();
        let restore = RestoreDirMode {
            path: parent.clone(),
            mode: 0o755,
        };

        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_state_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("state".into())));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("file".into())));
        materialization.expect_promote().returning(|_| Ok(()));
        let parent_for_complete = parent.clone();
        materialization.expect_complete().returning(move |_| {
            // After durable Add commit, block the subsequent profiles.yaml rewrite
            // that set_current_if_none needs for auto-activation.
            std::fs::set_permissions(&parent_for_complete, std::fs::Permissions::from_mode(0o555))
                .expect("poison profiles parent after Add complete");
            Ok(())
        });
        materialization.expect_compensate().returning(|_| Ok(()));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Ok(PreparedCleanup::new("cleanup".into())));
        materialization
            .expect_activate_cleanup()
            .returning(|_| Ok(()));
        materialization
            .expect_cancel_cleanup()
            .returning(|_| Ok(()));
        materialization
            .expect_retry_cleanup()
            .returning(|_, _| Ok(CleanupOutcome::Removed));

        tauri::async_runtime::block_on(async {
            let (application, session_state, clash_config) = test_typed_config_clients(&dir).await;
            let profiles = profiles::ProfilesClient::new(
                temp_config_path(&dir, "profiles.yaml"),
                Arc::new(MockProfileFsPort::new()),
                Arc::new(MockSubscriptionFetcher::new()),
                Arc::new(materialization),
                Arc::new(MockRebuildNotifier::new()),
            )
            .await
            .expect("profiles client");
            let ports = Arc::new(SessionPortResolver::default());
            ports
                .resolve(&ClashConfig::default())
                .expect("default ports");
            let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
            let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
            let (core_client, requests) = test_actor_parts(&paths, runtime_paths.clone()).await;
            let client = NyanpasuClient::with_parts(
                application,
                session_state,
                clash_config,
                profiles,
                Arc::new(MockProfileFsPort::new()),
                ports,
                dir.path().join("profiles"),
                runtime_paths,
                Arc::new(crate::client::event_sink::NoopUiEventSink),
                core_client,
                test_core_port(Arc::new(MockRunningCoreBridge::new())),
                requests,
                test_service_control(),
                test_degradation_sink(),
                Arc::new(NoopSystemDnsCache),
                rebuild::RebuildCoordinator::new(),
            );

            let outcome = client
                .create_profile(local_config_request("local"), Some("proxies: []\n".into()))
                .await
                .expect("create must keep the committed ProfileId as Ok");
            // Restore before further assertions that may touch the temp dir.
            drop(restore);

            assert!(
                matches!(
                    outcome,
                    crate::client::runtime::MutationOutcome::CommittedDegraded { .. }
                ),
                "auto-activation hard failure after commit must be CommittedDegraded"
            );
            let uid = outcome.value().clone();
            let codes: Vec<_> = outcome
                .degradations()
                .iter()
                .map(|item| item.code.as_str())
                .collect();
            assert!(
                codes.contains(&"profile_auto_activation_failed"),
                "expected profile_auto_activation_failed, got {codes:?}"
            );
            assert!(
                outcome.degradations().iter().any(|item| {
                    item.phase == crate::client::runtime::DegradationPhase::SystemEffect
                        && item.retryable
                }),
                "H2 degradation must be retryable SystemEffect"
            );

            let snapshot = client.get_profiles().await.unwrap();
            assert!(
                snapshot.items.contains_key(&uid),
                "committed item must remain after auto-activation failure"
            );
            assert!(
                snapshot.current.is_none(),
                "failed set_current_if_none must leave current unset"
            );
        });
    }

    #[test]
    fn facade_import_does_not_steal_existing_current() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                filename: None,
                suggested_update_interval_minutes: None,
            })
        });
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;

            // Establish a current selection via a local Config.
            let local_uid = client
                .create_profile(local_config_request("local"), Some("proxies: []\n".into()))
                .await
                .expect("create local config")
                .into_value();
            assert_eq!(
                client.get_profiles().await.unwrap().current.as_ref(),
                Some(&local_uid)
            );

            // Import a remote subscription; current is already set, so import
            // must NOT overwrite the selection made before it.
            // Ok(None) from set_current_if_none remains non-degraded applied.
            let url = url::Url::parse("https://example.com/subs/x.yaml").unwrap();
            let outcome = client
                .import_profile(url, None, None)
                .await
                .expect("import");
            assert!(
                matches!(
                    outcome,
                    crate::client::runtime::MutationOutcome::Applied { .. }
                ),
                "skipped auto-activation (existing current) must stay applied"
            );
            let imported = outcome.into_value();
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(
                snapshot.current.as_ref(),
                Some(&local_uid),
                "import must not overwrite an existing current selection"
            );
            assert!(snapshot.items.contains_key(&imported));
            let ProfileSource::Remote { option, .. } =
                snapshot.items[&imported].definition.source().unwrap()
            else {
                unreachable!()
            };
            assert_eq!(option.update_interval_minutes, 120);
        });
    }

    #[test]
    fn facade_create_skips_activation_as_applied_when_current_exists() {
        let dir = tempdir().unwrap();
        let fetcher = MockSubscriptionFetcher::new();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let first = client
                .create_profile(local_config_request("first"), Some("proxies: []\n".into()))
                .await
                .expect("first create")
                .into_value();
            let second = client
                .create_profile(local_config_request("second"), Some("proxies: []\n".into()))
                .await
                .expect("second create");
            assert!(
                matches!(
                    second,
                    crate::client::runtime::MutationOutcome::Applied { .. }
                ),
                "Ok(None) auto-activation must not invent degradations"
            );
            let second_uid = second.into_value();
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(snapshot.current.as_ref(), Some(&first));
            assert!(snapshot.items.contains_key(&second_uid));
        });
    }

    /// create/import share try_auto_activate_if_none: an activation hard error
    /// becomes committed_degraded and must retain the already-committed ProfileId.
    /// VersionConflict is not special-cased as success.
    #[test]
    fn create_import_auto_activation_failure_retains_profile_id_as_committed_degraded() {
        let uid = ProfileId("committed-uid".into());
        for error in [
            ProfilesError::Persist("disk full".into()),
            ProfilesError::VersionConflict {
                expected: 1,
                actual: 2,
            },
            ProfilesError::Rpc("actor stopped".into()),
        ] {
            let degradation = NyanpasuClient::auto_activation_failure_degradation(&error);
            assert_eq!(degradation.code, "profile_auto_activation_failed");
            assert_eq!(
                degradation.phase,
                crate::client::runtime::DegradationPhase::SystemEffect
            );
            assert!(degradation.retryable);
            assert!(!degradation.message.is_empty());

            // Protocol both create and import use after a successful durable commit.
            let prior = vec![crate::client::runtime::Degradation {
                phase: crate::client::runtime::DegradationPhase::ProfileMaterialization,
                code: "cleanup_deferred".into(),
                message: "materialization cleanup deferred".into(),
                retryable: true,
            }];
            let outcome = crate::client::runtime::MutationOutcome::from_parts(uid.clone(), prior)
                .extend_degradations(vec![degradation]);
            assert!(
                matches!(
                    outcome,
                    crate::client::runtime::MutationOutcome::CommittedDegraded { .. }
                ),
                "activation hard error after commit must be CommittedDegraded"
            );
            assert_eq!(outcome.value(), &uid);
            let codes: Vec<_> = outcome
                .degradations()
                .iter()
                .map(|item| item.code.as_str())
                .collect();
            assert_eq!(
                codes,
                ["cleanup_deferred", "profile_auto_activation_failed"],
                "prior commit degradations must merge with activation failure"
            );
        }
    }

    fn ok_fetch_without_name() -> MockSubscriptionFetcher {
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                filename: None,
                suggested_update_interval_minutes: None,
            })
        });
        fetcher
    }

    #[test]
    fn facade_import_without_name_derives_url_name_and_leaves_it_unpinned() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client =
                test_client_with_fetcher(&dir, Arc::new(ok_fetch_without_name()), Arc::new(core))
                    .await;
            let url = url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap();
            let uid = client
                .import_profile(url, None, None)
                .await
                .expect("import")
                .into_value();
            let item = client.get_profiles().await.unwrap().items[&uid].clone();
            assert_eq!(item.metadata.name, "my-sub");
            assert!(
                !item.metadata.custom_name,
                "no caller name -> unpinned so refresh name-sync can adopt a server name"
            );
        });
    }

    #[test]
    fn facade_import_with_name_uses_it_and_pins_custom_name() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client =
                test_client_with_fetcher(&dir, Arc::new(ok_fetch_without_name()), Arc::new(core))
                    .await;
            let url = url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap();
            let uid = client
                .import_profile(url, Some("My VPN".into()), None)
                .await
                .expect("import")
                .into_value();
            let item = client.get_profiles().await.unwrap().items[&uid].clone();
            assert_eq!(item.metadata.name, "My VPN");
            assert!(
                item.metadata.custom_name,
                "a caller-provided name is user intent and must be pinned"
            );
        });
    }
}
