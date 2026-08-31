mod application;
mod clash_config;
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

use self::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    session_state::SessionStateClient,
};
use crate::{
    core::actor_v2::{
        CoreClient as CoreClientV2, CoreStatusProjection, HandoffReport, ShutdownReport,
        endpoint::ExecutionHost,
        facade::{CoreFacade, ReconcileReport, RecoverReport, StopReport},
        service_actor::{ServiceClient, ServiceHostStatus, ServicePhase},
    },
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

pub use core_bridge::{CoreLifecycleLease, CoreLifecyclePort, LegacyCoreBridge};
pub use error::{ClientError, Result};
pub(crate) use error::{CompensationFailure, LegacyVergeDomain, PartialCommit};
#[cfg(test)]
pub use event_sink::NoopUiEventSink;
pub use event_sink::{TauriUiEventSink, UiEventSink};
pub use ports::SessionPortResolver;
pub use runtime::RuntimePaths;
#[cfg(test)]
pub use system_dns::{MockSystemDnsCache, NoopSystemDnsCache};
pub use system_dns::{OsSystemDnsCache, SystemDnsCache};
#[cfg(test)]
pub use tests::{MockRunningCoreBridge, TestRunningCoreBridge as RunningCoreBridge};

pub struct ClientSetupArgs {
    pub paths: PathResolver,
    pub runtime_paths: RuntimePaths,
    pub bridges: LegacyBridgeSet,
    pub ui_sink: Arc<dyn UiEventSink>,
    pub core: Arc<dyn CoreLifecyclePort>,
    pub core_v2: CoreClientV2,
    pub service: ServiceClient,
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

fn client_core_error(error: ClientError) -> nyanpasu_core_manager::CoreError {
    nyanpasu_core_manager::CoreError::new(
        nyanpasu_core_manager::CoreErrorKind::Internal,
        error.to_string(),
        false,
    )
}

fn client_error_from_core(error: nyanpasu_core_manager::CoreError) -> ClientError {
    ClientError::Anyhow(anyhow::anyhow!(error))
}

#[cfg(not(test))]
fn runtime_core_spec(
    core: &nyanpasu_config::application::ClashCore,
) -> anyhow::Result<nyanpasu_core_manager::CoreSpec> {
    crate::core::actor_v2::local_host::core_spec(core)
}

#[cfg(test)]
fn runtime_core_spec(
    core: &nyanpasu_config::application::ClashCore,
) -> anyhow::Result<nyanpasu_core_manager::CoreSpec> {
    use nyanpasu_core_manager::CoreKind;
    let kind = match core {
        nyanpasu_config::application::ClashCore::ClashPremium => CoreKind::ClashPremium,
        nyanpasu_config::application::ClashCore::ClashRs
        | nyanpasu_config::application::ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        nyanpasu_config::application::ClashCore::Mihomo
        | nyanpasu_config::application::ClashCore::MihomoAlpha => CoreKind::Mihomo,
        nyanpasu_config::application::ClashCore::Meow => CoreKind::Meow,
    };
    Ok(nyanpasu_core_manager::CoreSpec {
        kind,
        binary_path: camino::Utf8PathBuf::from("fake-core"),
        version: None,
        features: Vec::new(),
    })
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
    core: Arc<dyn CoreLifecyclePort>,
    core_v2: CoreFacade,
    /// Serializes host handoff with service uninstall inside this app process.
    host_transition: tokio::sync::Mutex<()>,
    system_dns: Arc<dyn SystemDnsCache>,
    /// Serializes runtime regeneration (snapshot -> build -> runtime draft ->
    /// core apply). The profiles actor only orders commits; without this gate
    /// a slow rebuild started for an older commit can finish after a newer
    /// one and overwrite the runtime with a stale snapshot.
    rebuild_gate: tokio::sync::Mutex<()>,
    /// Instance-owned background dirty coordinator (capacity-1 coalesce).
    /// Request/reply regeneration calls typed facade methods directly.
    rebuild: rebuild::RebuildCoordinator,
    runtime_revisions: runtime::RuntimeRevisionAllocator,
    runtime: runtime::RuntimeLifecycleStore,
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
            core_v2,
            service,
            system_dns,
        } = args;
        let profiles_dir = paths.app_profiles_dir();
        let profiles_path = utf8_path(paths.profiles_path())?;
        let runtime_paths_for_setup = runtime_paths.clone();
        let (application, session_state, clash_config, profiles, runtime_store, ports, fs, rebuild) =
            tauri::async_runtime::block_on(async move {
                runtime_paths_for_setup
                    .cleanup_stale_candidates(std::time::Duration::from_secs(24 * 60 * 60))
                    .await
                    .context("failed to clean stale runtime candidates")?;
                let (application, session_state, clash_config) =
                    new_typed_config_clients(paths.clone(), bridges).await?;

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
                let runtime_store = runtime::new_runtime_lifecycle_store().await?;
                anyhow::Ok((
                    application,
                    session_state,
                    clash_config,
                    profiles,
                    runtime_store,
                    ports,
                    file_service as Arc<dyn ProfileFsPort>,
                    rebuild,
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
            core,
            core_v2,
            service,
            system_dns,
            runtime_store,
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
        core: Arc<dyn CoreLifecyclePort>,
        core_v2: CoreClientV2,
        service: ServiceClient,
        system_dns: Arc<dyn SystemDnsCache>,
        runtime: runtime::RuntimeLifecycleStore,
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
                core,
                core_v2: CoreFacade::new(core_v2, service),
                host_transition: tokio::sync::Mutex::new(()),
                system_dns,
                rebuild_gate: tokio::sync::Mutex::new(()),
                rebuild,
                runtime_revisions: runtime::RuntimeRevisionAllocator::new(),
                runtime,
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
                    .rebuild_running_config()
                    .await
                    .map_err(anyhow::Error::from)
            }
        });
    }

    /// Stop the instance-owned rebuild worker and await its exit.
    ///
    /// Contract (PR-4S S09):
    /// - Shuts down only the capacity-1 dirty rebuild worker owned by this client graph.
    /// - Does **not** act as a general service locator teardown.
    /// Core shutdown is owned by [`Self::shutdown_core`] and is invoked once by
    /// the application exit path after this worker has quiesced.
    /// - Safe to call multiple times; post-shutdown dirty notifications are no-ops.
    /// - An already in-flight rebuild is allowed to finish; coalesce waits abort.
    pub async fn shutdown(&self) {
        self.inner.rebuild.shutdown().await;
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

    pub async fn reconcile_core(
        &self,
    ) -> std::result::Result<ReconcileReport, nyanpasu_core_manager::CoreError> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let snapshot = self
            .regenerate_runtime_v2_inner()
            .await
            .map_err(client_core_error)?;
        let core_spec = runtime_core_spec(&snapshot.target_core).map_err(|error| {
            nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::BinaryNotFound,
                error.to_string(),
                false,
            )
        })?;
        self.inner
            .core_v2
            .reconcile(snapshot.target_core, &snapshot.config, core_spec)
            .await
    }

    pub async fn update_core(
        &self,
        core: nyanpasu_config::application::ClashCore,
    ) -> std::result::Result<ReconcileReport, nyanpasu_core_manager::CoreError> {
        let mut app = self.get_app_config().await.map_err(client_core_error)?;
        app.core = core;
        self.replace_app_config(app)
            .await
            .map_err(client_core_error)?;
        self.reconcile_core().await
    }

    pub async fn stop_core(
        &self,
    ) -> std::result::Result<StopReport, nyanpasu_core_manager::CoreError> {
        self.inner.core_v2.stop().await
    }

    pub async fn recover_core(
        &self,
    ) -> std::result::Result<RecoverReport, nyanpasu_core_manager::CoreError> {
        self.inner.core_v2.recover().await
    }

    pub async fn change_execution_host(
        &self,
        host: ExecutionHost,
    ) -> std::result::Result<HandoffReport, nyanpasu_core_manager::CoreError> {
        let _transition = self.inner.host_transition.lock().await;
        self.inner.core_v2.change_execution_host(host).await
    }

    /// A completed handoff adopts the target with the runtime *stopped* -- see
    /// [`HandoffReport::Completed`], which leaves the reconcile to the facade.
    /// Skipping it reports a successful host switch and leaves the user with no
    /// core running on the host they just switched to.
    async fn reconcile_after_handoff(
        &self,
        report: HandoffReport,
    ) -> std::result::Result<(), nyanpasu_core_manager::CoreError> {
        match report {
            // Nothing moved, so nothing was stopped.
            HandoffReport::NoChange => Ok(()),
            HandoffReport::Completed { .. } => self.reconcile_core().await.map(|_| ()),
        }
    }

    pub async fn set_execution_host(
        &self,
        service_mode: bool,
    ) -> Result<runtime::MutationOutcome<()>> {
        let _transition = self.inner.host_transition.lock().await;
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_service_mode = Some(service_mode);
        self.patch_app_config(patch).await?;

        let effect = if service_mode {
            match self
                .inner
                .core_v2
                .change_execution_host(ExecutionHost::Service)
                .await
            {
                Ok(report) => self.reconcile_after_handoff(report).await,
                Err(error) => Err(error),
            }
        } else {
            match self
                .inner
                .core_v2
                .change_execution_host(ExecutionHost::Local)
                .await
            {
                Ok(report) => match self.reconcile_after_handoff(report).await {
                    // Only once the core is back up on Local is the daemon
                    // safe to stop; stopping it first would leave the user
                    // with nothing running.
                    Ok(()) => {
                        let phase = self.service_status().phase;
                        if matches!(
                            phase,
                            ServicePhase::NotInstalled | ServicePhase::DaemonStopped
                        ) {
                            Ok(())
                        } else {
                            self.inner.core_v2.stop_service().await
                        }
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };

        let degradations = effect.err().map_or_else(Vec::new, |error| {
            vec![runtime::Degradation {
                phase: runtime::DegradationPhase::SystemEffect,
                code: "service_host_transition_failed".into(),
                message: error.message,
                retryable: error.retryable,
            }]
        });
        Ok(runtime::MutationOutcome::from_parts((), degradations))
    }

    pub fn core_status(&self) -> CoreStatusProjection {
        self.inner.core_v2.core_status()
    }

    pub fn subscribe_core_events(&self) -> tokio::sync::broadcast::Receiver<CoreStatusProjection> {
        self.inner.core_v2.subscribe_core_events()
    }

    pub fn service_status(&self) -> ServiceHostStatus {
        self.inner.core_v2.service_status()
    }

    pub fn subscribe_service_events(&self) -> tokio::sync::watch::Receiver<ServiceHostStatus> {
        self.inner.core_v2.subscribe_service_events()
    }

    pub async fn probe_service(
        &self,
    ) -> std::result::Result<ServiceHostStatus, nyanpasu_core_manager::CoreError> {
        self.inner.core_v2.probe_service().await
    }

    /// Boot-time execution-host restore.
    ///
    /// The core actor always spawns on the Local endpoint, so a session that
    /// persisted service mode has to hand the runtime over before the first
    /// reconcile decides where the core comes up. Without this the app boots
    /// every service-mode user onto a local child process.
    ///
    /// A failing handoff leaves the app on Local rather than failing the boot:
    /// that is what the legacy `RunType` classification did when the daemon
    /// was not reachable, and the caller logs the degradation. No reconcile
    /// happens here -- the boot sequence runs one immediately afterwards, and
    /// a second would start the core twice.
    pub async fn restore_execution_host(&self) -> Result<()> {
        if !self.get_app_config().await?.enable_service_mode {
            return Ok(());
        }
        // Adopt the daemon only when it is already up and compatible; never
        // converge it. The legacy `RunType` classification picked Service only
        // while the IPC was connected, so booting never installed or started
        // the daemon -- and never raised a UAC prompt -- on the user's behalf.
        if !matches!(
            self.service_status().phase,
            crate::core::actor_v2::service_actor::ServicePhase::Ready
        ) {
            return Ok(());
        }
        let _transition = self.inner.host_transition.lock().await;
        self.inner
            .core_v2
            .change_execution_host(ExecutionHost::Service)
            .await
            .map_err(client_error_from_core)?;
        Ok(())
    }

    pub async fn install_service(
        &self,
    ) -> std::result::Result<(), nyanpasu_core_manager::CoreError> {
        self.inner.core_v2.install_service().await
    }

    pub async fn start_service(&self) -> std::result::Result<(), nyanpasu_core_manager::CoreError> {
        self.inner.core_v2.start_service().await
    }

    pub async fn stop_service(&self) -> std::result::Result<(), nyanpasu_core_manager::CoreError> {
        self.inner.core_v2.stop_service().await
    }

    pub async fn restart_service(
        &self,
    ) -> std::result::Result<(), nyanpasu_core_manager::CoreError> {
        self.inner.core_v2.stop_service().await?;
        self.inner.core_v2.start_service().await
    }

    pub async fn shutdown_core(&self) -> ShutdownReport {
        self.inner.core_v2.shutdown().await
    }

    pub async fn uninstall_service(
        &self,
    ) -> std::result::Result<(), nyanpasu_core_manager::CoreError> {
        let _transition = self.inner.host_transition.lock().await;
        if self.core_status().host == ExecutionHost::Service {
            return Err(nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::OperationConflict,
                "handoff to the local host before uninstalling the service",
                false,
            ));
        }
        self.inner.core_v2.uninstall_service().await
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

    /// Post-commit rebuild has a single opaque `Result` today; do not invent
    /// RuntimeCheck/Promote/Apply precision the error surface cannot support.
    fn map_runtime_rebuild_degradation(error: &ClientError) -> runtime::Degradation {
        runtime::Degradation {
            phase: runtime::DegradationPhase::RuntimeBuild,
            code: "runtime_rebuild_failed".into(),
            message: error.to_string(),
            retryable: true,
        }
    }

    async fn collect_post_commit_degradations(
        &self,
        report: &CommitReport,
    ) -> Vec<runtime::Degradation> {
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

        if report.affects_current
            && let Err(error) = self.rebuild_running_config().await
        {
            tracing::warn!(%error, "post-commit rebuild failed; state stays committed (degraded)");
            degradations.push(Self::map_runtime_rebuild_degradation(&error));
        }
        degradations
    }

    async fn after_commit(&self, report: &CommitReport) -> runtime::MutationOutcome<()> {
        runtime::MutationOutcome::from_parts(
            (),
            self.collect_post_commit_degradations(report).await,
        )
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
    async fn try_auto_activate_if_none(&self, uid: ProfileId) -> Vec<runtime::Degradation> {
        match self.inner.profiles.set_current_if_none(uid).await {
            Ok(Some(report)) => self.collect_post_commit_degradations(&report).await,
            Ok(None) => Vec::new(),
            Err(error) => vec![Self::auto_activation_failure_degradation(&error)],
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
            self.collect_post_commit_degradations(&report).await,
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
            outcome = outcome.extend_degradations(self.try_auto_activate_if_none(uid).await);
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
        let mut degradations = self.collect_post_commit_degradations(&report).await;
        // Atomically activate only when nothing was selected during the download
        // window. Failures degrade; they must not erase the committed ProfileId.
        degradations.extend(self.try_auto_activate_if_none(created.clone()).await);
        Ok(runtime::MutationOutcome::from_parts(created, degradations))
    }

    pub async fn delete_profile(&self, uid: ProfileId) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.delete(uid).await?;
        Ok(self.after_commit(&report).await)
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
        Ok(self.after_commit(&report).await)
    }

    pub async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileId>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.reorder(ReorderOp::ByList(list)).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.refresh(uid, patch).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.patch_metadata(uid, patch).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.patch_remote_options(uid, patch).await?;
        Ok(self.after_commit(&report).await)
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
        Ok(self.after_commit(&report).await)
    }

    pub async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.set_current(uid).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.set_global_transforms(ids).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.set_valid_fields(fields).await?;
        Ok(self.after_commit(&report).await)
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

    pub async fn promoted_runtime(&self) -> Option<Arc<runtime::RuntimeSnapshot>> {
        self.inner.runtime.read().await.promoted.clone()
    }

    pub(crate) async fn runtime_lifecycle_state(&self) -> runtime::RuntimeLifecycleState {
        self.inner.runtime.read().await.clone()
    }

    async fn publish_promoted(&self, snapshot: Arc<runtime::RuntimeSnapshot>) -> Result<()> {
        let mut lifecycle = self.inner.runtime.write().await;
        if lifecycle
            .promoted
            .as_ref()
            .is_some_and(|current| current.revision >= snapshot.revision)
        {
            return Err(ClientError::Custom(format!(
                "runtime promoted revision must advance (current: {:?}, next: {})",
                lifecycle.promoted.as_ref().map(|item| item.revision.get()),
                snapshot.revision.get()
            )));
        }
        lifecycle.promoted = Some(snapshot);
        Ok(())
    }

    pub(crate) fn runtime_product_path(&self) -> &camino::Utf8Path {
        self.inner.runtime_paths.product()
    }

    pub(crate) async fn promote_existing_runtime_product(
        &self,
    ) -> Result<Arc<runtime::RuntimeSnapshot>> {
        self.reconcile_core()
            .await
            .map_err(client_error_from_core)?;
        self.promoted_runtime().await.ok_or_else(|| {
            ClientError::Custom("reconcile completed without publishing a runtime product".into())
        })
    }

    pub(crate) async fn start_promoted_runtime(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(client_error_from_core)
    }

    pub async fn rebuild_running_config(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map_err(client_error_from_core)?;
        self.inner.ui_sink.refresh_clash();
        // 用户决策 2026-07-06:所有 rebuild 统一触发(选项默认 false 门控)。
        self.inner.core.on_profile_change().await;
        Ok(())
    }

    pub(crate) async fn regenerate_runtime(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(client_error_from_core)
    }

    async fn regenerate_runtime_v2_inner(&self) -> Result<Arc<runtime::RuntimeSnapshot>> {
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        let profiles = self.inner.profiles.get().await?;
        let clash = self.get_clash_config().await?;
        let app = self.get_app_config().await?;
        let snapshot = self
            .derive_runtime_snapshot(revision, profiles, clash, app)
            .await?;
        runtime::write_product(
            self.inner.runtime_paths.product().as_std_path(),
            snapshot.product_bytes(),
        )
        .await
        .map_err(ClientError::Anyhow)?;
        self.publish_promoted(snapshot.clone()).await?;
        Ok(snapshot)
    }

    async fn regenerate_runtime_with(
        &self,
        lease: &mut dyn CoreLifecycleLease,
        revision: runtime::RuntimeRevision,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: NyanpasuAppConfig,
    ) -> Result<Arc<runtime::RuntimeSnapshot>> {
        let snapshot = self
            .derive_runtime_snapshot(revision, profiles, clash, app)
            .await?;
        let core = snapshot.target_core;
        let product_bytes = snapshot.product_bytes();
        // Candidate -> check -> promote -> PUBLISH (spec §5.2, P0-1): readers
        // only ever see checked-and-promoted configs; a rejected candidate
        // leaves both the product and the manager untouched. target core =
        // the same input snapshot the builder used (P0-3).
        let candidate = self
            .inner
            .runtime_paths
            .create_candidate(product_bytes)
            .await
            .map_err(ClientError::Anyhow)?;
        if candidate.bytes_sha256() != snapshot.product_sha256 {
            return Err(ClientError::Custom(
                "runtime snapshot hash does not match candidate bytes".into(),
            ));
        }
        let checked = lease
            .check_and_promote(&candidate, core, self.inner.runtime_paths.product())
            .await;
        if let Err(error) = candidate.cleanup().await {
            tracing::warn!(%error, "failed to remove candidate config");
        }
        checked.map_err(ClientError::Anyhow)?;
        self.publish_promoted(snapshot.clone()).await?;
        Ok(snapshot)
    }

    async fn derive_runtime_snapshot(
        &self,
        revision: runtime::RuntimeRevision,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: NyanpasuAppConfig,
    ) -> Result<Arc<runtime::RuntimeSnapshot>> {
        let resolved_ports = self
            .inner
            .ports
            .resolve(&clash)
            .map_err(ClientError::Anyhow)?;
        let profiles_dir = self.inner.profiles_dir.clone();
        let core = app.core;
        let builtin_enabled = app.enable_builtin_enhanced;
        let (data, yaml) = tokio::task::spawn_blocking(
            move || -> anyhow::Result<(runtime::RuntimeSnapshotData, String)> {
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
        .map_err(|error| ClientError::Custom(format!("runtime build task failed: {error}")))?
        .map_err(ClientError::Anyhow)?;
        let product_bytes: Arc<[u8]> = Arc::from(yaml.into_bytes());
        let snapshot = Arc::new(runtime::RuntimeSnapshot::from_data(
            revision,
            core,
            product_bytes.clone(),
            data,
        ));
        Ok(snapshot)
    }
}

fn utf8_path(path: PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::{
        mirror::{
            ClashLegacyBridge, NoopPreparedLegacyMirror, PreparedLegacyMirror, VergeLegacyBridge,
            WindowLegacyBridge,
        },
        profiles::ports::{
            CleanupOutcome, MaterializationReconcileReport, MockProfileFsPort,
            MockProfileMaterializationPort, MockRebuildNotifier, MockSubscriptionFetcher,
            PreparedCleanup, PreparedMaterialization, ProfileMaterializationPort,
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
    use std::{collections::BTreeMap, sync::Mutex as StdMutex};
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    struct IdleEndpoint;

    impl crate::core::actor_v2::endpoint::ControlEndpoint for IdleEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Local
        }

        fn submit<'a>(
            &'a self,
            submission: crate::core::actor_v2::endpoint::CoreSubmission,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            std::result::Result<
                nyanpasu_ipc::api::core::v2::OperationInfo,
                nyanpasu_core_manager::CoreError,
            >,
        > {
            Box::pin(async move { Ok(successful_reconcile(submission.envelope.operation_id)) })
        }

        fn wait_operation<'a>(
            &'a self,
            id: nyanpasu_core_manager::OperationId,
            _timeout: std::time::Duration,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            Option<nyanpasu_ipc::api::core::v2::OperationInfo>,
        > {
            Box::pin(async move { Some(successful_reconcile(id)) })
        }

        fn status<'a>(
            &'a self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            std::result::Result<
                crate::core::actor_v2::endpoint::CoreStatusSnapshot,
                nyanpasu_core_manager::CoreError,
            >,
        > {
            Box::pin(async {
                Ok(crate::core::actor_v2::endpoint::CoreStatusSnapshot {
                    state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped {
                        reason: None,
                    }),
                    state_changed_at: 0,
                    revision: None,
                    healthy: Some(true),
                })
            })
        }
    }

    pub(crate) struct TestControlEndpoint {
        fail: bool,
        submissions: std::sync::atomic::AtomicUsize,
    }

    impl TestControlEndpoint {
        pub(crate) fn succeeding() -> Arc<Self> {
            Arc::new(Self {
                fail: false,
                submissions: std::sync::atomic::AtomicUsize::new(0),
            })
        }

        pub(crate) fn failing() -> Arc<Self> {
            Arc::new(Self {
                fail: true,
                submissions: std::sync::atomic::AtomicUsize::new(0),
            })
        }

        pub(crate) fn submissions(&self) -> usize {
            self.submissions.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn operation(
            &self,
            id: nyanpasu_core_manager::OperationId,
        ) -> nyanpasu_ipc::api::core::v2::OperationInfo {
            if self.fail {
                nyanpasu_ipc::api::core::v2::OperationInfo {
                    id: id.to_string(),
                    phase: nyanpasu_ipc::api::core::v2::OperationPhase::Failed,
                    output: None,
                    error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                        kind: None,
                        message: "reconcile boom".into(),
                        retryable: false,
                    }),
                }
            } else {
                successful_reconcile(id)
            }
        }
    }

    impl crate::core::actor_v2::endpoint::ControlEndpoint for TestControlEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Local
        }

        fn submit<'a>(
            &'a self,
            submission: crate::core::actor_v2::endpoint::CoreSubmission,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            std::result::Result<
                nyanpasu_ipc::api::core::v2::OperationInfo,
                nyanpasu_core_manager::CoreError,
            >,
        > {
            Box::pin(async move {
                self.submissions
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(self.operation(submission.envelope.operation_id))
            })
        }

        fn wait_operation<'a>(
            &'a self,
            id: nyanpasu_core_manager::OperationId,
            _timeout: std::time::Duration,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            Option<nyanpasu_ipc::api::core::v2::OperationInfo>,
        > {
            Box::pin(async move { Some(self.operation(id)) })
        }

        fn status<'a>(
            &'a self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            std::result::Result<
                crate::core::actor_v2::endpoint::CoreStatusSnapshot,
                nyanpasu_core_manager::CoreError,
            >,
        > {
            Box::pin(async {
                Ok(crate::core::actor_v2::endpoint::CoreStatusSnapshot {
                    state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped {
                        reason: None,
                    }),
                    state_changed_at: 0,
                    revision: None,
                    healthy: Some(true),
                })
            })
        }
    }

    struct HostTransitionEndpoint {
        host: ExecutionHost,
        calls: Arc<StdMutex<Vec<&'static str>>>,
        operations:
            StdMutex<std::collections::HashMap<String, nyanpasu_ipc::api::core::v2::OperationInfo>>,
    }

    impl HostTransitionEndpoint {
        fn new(host: ExecutionHost, calls: Arc<StdMutex<Vec<&'static str>>>) -> Arc<Self> {
            Arc::new(Self {
                host,
                calls,
                operations: StdMutex::new(Default::default()),
            })
        }
    }

    impl crate::core::actor_v2::endpoint::ControlEndpoint for HostTransitionEndpoint {
        fn host(&self) -> ExecutionHost {
            self.host
        }

        fn submit<'a>(
            &'a self,
            submission: crate::core::actor_v2::endpoint::CoreSubmission,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            std::result::Result<
                nyanpasu_ipc::api::core::v2::OperationInfo,
                nyanpasu_core_manager::CoreError,
            >,
        > {
            Box::pin(async move {
                let output = match submission.envelope.command {
                    nyanpasu_core_manager::CoreCommand::Stop => {
                        if self.host == ExecutionHost::Service {
                            self.calls.lock().unwrap().push("handoff_to_local");
                        }
                        nyanpasu_ipc::api::core::v2::OperationOutputInfo::Stopped
                    }
                    nyanpasu_core_manager::CoreCommand::Reconcile(_) => {
                        self.calls.lock().unwrap().push(match self.host {
                            ExecutionHost::Local => "reconcile_local",
                            ExecutionHost::Service => "reconcile_service",
                        });
                        successful_reconcile(submission.envelope.operation_id)
                            .output
                            .unwrap()
                    }
                    _ => successful_reconcile(submission.envelope.operation_id)
                        .output
                        .unwrap(),
                };
                let info = nyanpasu_ipc::api::core::v2::OperationInfo {
                    id: submission.envelope.operation_id.to_string(),
                    phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
                    output: Some(output),
                    error: None,
                };
                self.operations
                    .lock()
                    .unwrap()
                    .insert(info.id.clone(), info.clone());
                Ok(info)
            })
        }

        fn wait_operation<'a>(
            &'a self,
            id: nyanpasu_core_manager::OperationId,
            _timeout: std::time::Duration,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            Option<nyanpasu_ipc::api::core::v2::OperationInfo>,
        > {
            Box::pin(async move {
                self.operations
                    .lock()
                    .unwrap()
                    .get(&id.to_string())
                    .cloned()
            })
        }

        fn status<'a>(
            &'a self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            'a,
            std::result::Result<
                crate::core::actor_v2::endpoint::CoreStatusSnapshot,
                nyanpasu_core_manager::CoreError,
            >,
        > {
            Box::pin(async {
                Ok(crate::core::actor_v2::endpoint::CoreStatusSnapshot {
                    state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Running {
                        epoch: 1,
                        pid: 7,
                    }),
                    state_changed_at: 0,
                    revision: None,
                    healthy: Some(true),
                })
            })
        }
    }

    struct HostTransitionServiceAdapter {
        endpoint: crate::core::actor_v2::endpoint::EndpointHandle,
        calls: Arc<StdMutex<Vec<&'static str>>>,
    }

    impl crate::core::actor_v2::service_actor::ServiceHostAdapter for HostTransitionServiceAdapter {
        fn probe(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            '_,
            std::result::Result<nyanpasu_ipc::types::StatusInfo<'static>, String>,
        > {
            Box::pin(async move {
                self.calls.lock().unwrap().push("ensure_ready");
                Ok(nyanpasu_ipc::types::StatusInfo {
                    name: std::borrow::Cow::Borrowed("test-service"),
                    version: std::borrow::Cow::Borrowed("2.0.0"),
                    status: nyanpasu_ipc::types::ServiceStatus::Running,
                    server: Some(nyanpasu_ipc::api::status::StatusResBody {
                        version: std::borrow::Cow::Borrowed("2.0.0"),
                        core_infos: nyanpasu_ipc::api::status::CoreInfos {
                            r#type: None,
                            state: nyanpasu_ipc::api::status::CoreState::Running,
                            state_changed_at: 0,
                            config_path: None,
                            controller: None,
                            health: None,
                            revision: None,
                            detail: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped {
                                reason: None,
                            }),
                        },
                        runtime_infos: nyanpasu_ipc::api::status::RuntimeInfos {
                            service_data_dir: std::borrow::Cow::Owned(Default::default()),
                            service_config_dir: std::borrow::Cow::Owned(Default::default()),
                            nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                            nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                        },
                        logs: None,
                    }),
                })
            })
        }

        fn install(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn uninstall(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn start_daemon(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn stop_daemon(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async move {
                self.calls.lock().unwrap().push("stop_daemon");
                Ok(())
            })
        }

        fn update(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
            self.endpoint.clone()
        }
    }

    struct OrderingVergeBridge {
        calls: Arc<StdMutex<Vec<&'static str>>>,
    }

    struct OrderingPreparedVergeBridge {
        calls: Arc<StdMutex<Vec<&'static str>>>,
    }

    impl PreparedLegacyMirror for OrderingPreparedVergeBridge {
        fn apply(self: Box<Self>) {
            self.calls.lock().unwrap().push("commit");
        }
    }

    impl VergeLegacyBridge for OrderingVergeBridge {
        fn prepare(
            &self,
            _snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(OrderingPreparedVergeBridge {
                calls: self.calls.clone(),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    fn successful_reconcile(
        id: nyanpasu_core_manager::OperationId,
    ) -> nyanpasu_ipc::api::core::v2::OperationInfo {
        nyanpasu_ipc::api::core::v2::OperationInfo {
            id: id.to_string(),
            phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
            output: Some(
                nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(
                    nyanpasu_ipc::api::core::v2::ReconcileOutcomeInfo {
                        outcome: nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Started,
                        revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                            epoch: 1,
                            generation: 1,
                            source_hash: "source".into(),
                            effective_hash: "effective".into(),
                        },
                        warning: None,
                        failed_apply: None,
                    },
                ),
            ),
            error: None,
        }
    }

    struct IdleServiceAdapter;

    impl crate::core::actor_v2::service_actor::ServiceHostAdapter for IdleServiceAdapter {
        fn probe(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<
            '_,
            std::result::Result<nyanpasu_ipc::types::StatusInfo<'static>, String>,
        > {
            Box::pin(async {
                Ok(nyanpasu_ipc::types::StatusInfo {
                    name: std::borrow::Cow::Borrowed("test-service"),
                    version: std::borrow::Cow::Borrowed("test"),
                    status: nyanpasu_ipc::types::ServiceStatus::NotInstalled,
                    server: None,
                })
            })
        }

        fn install(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn uninstall(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn start_daemon(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn stop_daemon(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn update(
            &self,
        ) -> crate::core::actor_v2::endpoint::BoxFuture<'_, std::result::Result<(), String>>
        {
            Box::pin(async { Ok(()) })
        }

        fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
            Arc::new(IdleEndpoint)
        }
    }

    struct UnusedLifecyclePort;

    #[async_trait]
    impl CoreLifecyclePort for UnusedLifecyclePort {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            anyhow::bail!("legacy lifecycle port must not be used by v2 client tests")
        }

        async fn status(&self) -> anyhow::Result<core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("legacy lifecycle status must not be used by v2 client tests")
        }

        async fn on_profile_change(&self) {}
    }

    pub(crate) fn test_v2_clients() -> (CoreClientV2, ServiceClient) {
        test_v2_clients_with_endpoint(Arc::new(IdleEndpoint))
    }

    pub(crate) fn test_v2_clients_with_endpoint(
        endpoint: crate::core::actor_v2::endpoint::EndpointHandle,
    ) -> (CoreClientV2, ServiceClient) {
        std::thread::spawn(move || {
            tauri::async_runtime::block_on(async {
                let core = CoreClientV2::spawn(endpoint).await.unwrap();
                let service = ServiceClient::spawn(Arc::new(IdleServiceAdapter), 0)
                    .await
                    .unwrap();
                (core, service)
            })
        })
        .join()
        .expect("test v2 client construction should not panic")
    }

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
        ) -> anyhow::Result<[u8; 32]> {
            self.inner.check_and_promote(candidate, target_core).await?;
            Ok(candidate.bytes_sha256())
        }

        async fn apply_candidate(
            &mut self,
            candidate: &runtime::CandidateFile,
            target_core: nyanpasu_config::application::ClashCore,
        ) -> anyhow::Result<()> {
            self.inner.check_and_promote(candidate, target_core).await
        }

        async fn apply_promoted(&mut self, _product: &camino::Utf8Path) -> anyhow::Result<()> {
            self.inner.apply_config().await
        }

        async fn restart(&mut self) -> anyhow::Result<()> {
            self.inner.restart_core().await
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
        ) -> anyhow::Result<[u8; 32]> {
            self.inner.check_and_promote(candidate, target_core).await?;
            Ok(candidate.bytes_sha256())
        }

        async fn apply_candidate(
            &mut self,
            candidate: &runtime::CandidateFile,
            target_core: nyanpasu_config::application::ClashCore,
        ) -> anyhow::Result<()> {
            self.inner.check_and_promote(candidate, target_core).await
        }

        async fn apply_promoted(&mut self, _product: &camino::Utf8Path) -> anyhow::Result<()> {
            self.inner.apply_config().await
        }

        async fn restart(&mut self) -> anyhow::Result<()> {
            self.inner.restart_core().await
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

    #[derive(Default)]
    struct CompensationLease {
        checked: Vec<Vec<u8>>,
        apply_calls: usize,
    }

    #[async_trait]
    impl CoreLifecycleLease for CompensationLease {
        async fn check_and_promote(
            &mut self,
            candidate: &runtime::CandidateFile,
            _target_core: nyanpasu_config::application::ClashCore,
            product: &camino::Utf8Path,
        ) -> anyhow::Result<[u8; 32]> {
            let bytes = tokio::fs::read(candidate.path()).await?;
            runtime::write_product(product.as_std_path(), &bytes).await?;
            self.checked.push(bytes);
            Ok(candidate.bytes_sha256())
        }

        async fn apply_candidate(
            &mut self,
            candidate: &runtime::CandidateFile,
            _target_core: nyanpasu_config::application::ClashCore,
        ) -> anyhow::Result<()> {
            self.checked.push(tokio::fs::read(candidate.path()).await?);
            self.apply_calls += 1;
            Ok(())
        }

        async fn apply_promoted(&mut self, _product: &camino::Utf8Path) -> anyhow::Result<()> {
            self.apply_calls += 1;
            Ok(())
        }

        async fn restart(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct BarrierCompensationLease {
        _guard: tokio::sync::OwnedMutexGuard<()>,
        check_entered: Option<tokio::sync::oneshot::Sender<()>>,
        release_check: Option<tokio::sync::oneshot::Receiver<()>>,
    }

    #[async_trait]
    impl CoreLifecycleLease for BarrierCompensationLease {
        async fn check_and_promote(
            &mut self,
            candidate: &runtime::CandidateFile,
            _target_core: nyanpasu_config::application::ClashCore,
            product: &camino::Utf8Path,
        ) -> anyhow::Result<[u8; 32]> {
            if let Some(sender) = self.check_entered.take() {
                let _ = sender.send(());
            }
            if let Some(receiver) = self.release_check.take() {
                let _ = receiver.await;
            }
            let bytes = tokio::fs::read(candidate.path()).await?;
            runtime::write_product(product.as_std_path(), &bytes).await?;
            Ok(candidate.bytes_sha256())
        }

        async fn apply_candidate(
            &mut self,
            _candidate: &runtime::CandidateFile,
            _target_core: nyanpasu_config::application::ClashCore,
        ) -> anyhow::Result<()> {
            if let Some(sender) = self.check_entered.take() {
                let _ = sender.send(());
            }
            if let Some(receiver) = self.release_check.take() {
                let _ = receiver.await;
            }
            Ok(())
        }

        async fn apply_promoted(&mut self, _product: &camino::Utf8Path) -> anyhow::Result<()> {
            Ok(())
        }

        async fn restart(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
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
        let (core_v2, service) = test_v2_clients();
        NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            Arc::new(MockProfileFsPort::new()),
            ports,
            dir.path().join("profiles"),
            RuntimePaths::from_resolver(&PathResolver::with_base_dirs(
                dir.path().into(),
                dir.path().join("data"),
            ))
            .unwrap(),
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            Arc::new(UnusedLifecyclePort),
            core_v2,
            service,
            system_dns,
            crate::client::runtime::new_runtime_lifecycle_store()
                .await
                .expect("runtime state store"),
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

    /// Like [`test_profiles_client_args`], but accepts an already-typed
    /// [`CoreLifecyclePort`] (e.g. process-backed S09 adapter) without the
    /// mockall `TestRunningCoreBridge` wrapper.
    pub(crate) fn test_client_args_with_lifecycle(
        dir: &TempDir,
        core: Arc<dyn CoreLifecyclePort>,
    ) -> ClientSetupArgs {
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let (core_v2, service) = test_v2_clients();
        ClientSetupArgs {
            paths,
            runtime_paths,
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core,
            core_v2,
            service,
            system_dns: Arc::new(NoopSystemDnsCache),
        }
    }

    pub(crate) fn test_client_args_with_endpoint(
        dir: &TempDir,
        endpoint: crate::core::actor_v2::endpoint::EndpointHandle,
    ) -> ClientSetupArgs {
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let (core_v2, service) = test_v2_clients_with_endpoint(endpoint);
        ClientSetupArgs {
            paths,
            runtime_paths,
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core: Arc::new(UnusedLifecyclePort),
            core_v2,
            service,
            system_dns: Arc::new(NoopSystemDnsCache),
        }
    }

    fn host_transition_client(
        dir: &TempDir,
        calls: Arc<StdMutex<Vec<&'static str>>>,
    ) -> NyanpasuClient {
        let local = HostTransitionEndpoint::new(ExecutionHost::Local, calls.clone());
        let service_endpoint = HostTransitionEndpoint::new(ExecutionHost::Service, calls.clone());
        let adapter = Arc::new(HostTransitionServiceAdapter {
            endpoint: service_endpoint,
            calls: calls.clone(),
        });
        let (core_v2, service) = std::thread::spawn(move || {
            tauri::async_runtime::block_on(async {
                let core = CoreClientV2::spawn(local).await.unwrap();
                let service = ServiceClient::spawn(adapter, 0).await.unwrap();
                (core, service)
            })
        })
        .join()
        .unwrap();
        let mut args = test_client_args_with_endpoint(dir, Arc::new(IdleEndpoint));
        args.bridges.verge = Arc::new(OrderingVergeBridge {
            calls: calls.clone(),
        });
        args.core_v2 = core_v2;
        args.service = service;
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        calls.lock().unwrap().clear();
        client
    }

    #[test]
    fn enabling_service_mode_commits_before_ensure_ready() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let client = host_transition_client(&dir, calls.clone());

        tauri::async_runtime::block_on(async {
            let outcome = client.set_execution_host(true).await.unwrap();
            assert!(matches!(outcome, runtime::MutationOutcome::Applied { .. }));
            assert!(client.get_app_config().await.unwrap().enable_service_mode);
        });

        let calls = calls.lock().unwrap();
        let commit = calls.iter().position(|call| *call == "commit").unwrap();
        let ensure = calls
            .iter()
            .position(|call| *call == "ensure_ready")
            .unwrap();
        assert!(commit < ensure, "calls: {calls:?}");
    }

    #[test]
    fn disabling_service_mode_hands_off_before_stopping_the_daemon() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let client = host_transition_client(&dir, calls.clone());

        tauri::async_runtime::block_on(async {
            client.set_execution_host(true).await.unwrap();
            calls.lock().unwrap().clear();
            let outcome = client.set_execution_host(false).await.unwrap();
            assert!(matches!(outcome, runtime::MutationOutcome::Applied { .. }));
            assert!(!client.get_app_config().await.unwrap().enable_service_mode);
        });

        let calls = calls.lock().unwrap();
        let handoff = calls
            .iter()
            .position(|call| *call == "handoff_to_local")
            .unwrap();
        let stop = calls
            .iter()
            .position(|call| *call == "stop_daemon")
            .unwrap();
        assert!(handoff < stop, "calls: {calls:?}");
    }

    /// A handoff adopts its target with the runtime stopped, so the switch is
    /// only finished once the core is reconciled onto the new host. Without
    /// that step both directions report success and leave nothing running.
    #[test]
    fn enabling_service_mode_reconciles_onto_the_adopted_host() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let client = host_transition_client(&dir, calls.clone());

        tauri::async_runtime::block_on(async {
            client.set_execution_host(true).await.unwrap();
        });

        let calls = calls.lock().unwrap();
        let ensure = calls
            .iter()
            .position(|call| *call == "ensure_ready")
            .unwrap();
        let reconcile = calls
            .iter()
            .position(|call| *call == "reconcile_service")
            .expect("the service host must be reconciled after adopting it");
        assert!(ensure < reconcile, "calls: {calls:?}");
    }

    #[test]
    fn disabling_service_mode_reconciles_local_before_stopping_the_daemon() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let client = host_transition_client(&dir, calls.clone());

        tauri::async_runtime::block_on(async {
            client.set_execution_host(true).await.unwrap();
            calls.lock().unwrap().clear();
            client.set_execution_host(false).await.unwrap();
        });

        let calls = calls.lock().unwrap();
        let handoff = calls
            .iter()
            .position(|call| *call == "handoff_to_local")
            .unwrap();
        let reconcile = calls
            .iter()
            .position(|call| *call == "reconcile_local")
            .expect("the local host must be reconciled after adopting it");
        let stop = calls
            .iter()
            .position(|call| *call == "stop_daemon")
            .unwrap();
        assert!(
            handoff < reconcile && reconcile < stop,
            "the core has to be running locally before the daemon goes away: {calls:?}"
        );
    }

    pub(crate) fn test_profiles_client_args(
        dir: &TempDir,
        core: Arc<dyn TestRunningCoreBridge>,
    ) -> ClientSetupArgs {
        test_client_args_with_lifecycle(dir, test_core_port(core))
    }

    fn minimal_file_profile_request() -> NewProfileRequest {
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
        let (core_v2, service) = test_v2_clients();
        let client = NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            file_service.clone() as Arc<dyn ProfileFsPort>,
            ports,
            paths.app_profiles_dir(),
            RuntimePaths::from_resolver(&paths).unwrap(),
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            Arc::new(UnusedLifecyclePort),
            core_v2,
            service,
            Arc::new(NoopSystemDnsCache),
            crate::client::runtime::new_runtime_lifecycle_store()
                .await
                .expect("runtime state store"),
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
        let (core_v2, service) = test_v2_clients();
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
            paths,
            runtime_paths,
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core: Arc::new(UnusedLifecyclePort),
            core_v2,
            service,
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
        let lifecycle = tauri::async_runtime::block_on(client.runtime_lifecycle_state());

        assert!(promoted.is_none());
        assert!(lifecycle.promoted.is_none());
    }

    #[tokio::test]
    async fn reconcile_publishes_the_runtime_product_read_model() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        client.reconcile_core().await.unwrap();
        assert!(client.promoted_runtime().await.is_some());
        assert!(client.runtime_product_path().exists());
    }

    #[tokio::test]
    async fn repeated_reconcile_advances_the_runtime_revision() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        client.reconcile_core().await.unwrap();
        let first = client.promoted_runtime().await.unwrap();
        client.reconcile_core().await.unwrap();
        let second = client.promoted_runtime().await.unwrap();
        assert!(second.revision > first.revision);
    }

    #[tokio::test]
    async fn reconcile_product_matches_the_published_snapshot() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        client.reconcile_core().await.unwrap();
        let snapshot = client.promoted_runtime().await.unwrap();
        assert_eq!(
            tokio::fs::read(client.runtime_product_path())
                .await
                .unwrap(),
            snapshot.product_bytes()
        );
    }

    #[tokio::test]
    async fn promote_existing_runtime_routes_through_reconcile() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        let snapshot = client.promote_existing_runtime_product().await.unwrap();
        assert!(Arc::ptr_eq(
            &snapshot,
            &client.promoted_runtime().await.unwrap()
        ));
    }

    #[tokio::test]
    async fn start_promoted_runtime_routes_through_reconcile() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        client.start_promoted_runtime().await.unwrap();
        assert!(client.promoted_runtime().await.is_some());
    }

    #[test]
    fn facade_add_activate_rebuilds_via_core_bridge() {
        let dir = tempdir().unwrap();
        let client = tauri::async_runtime::block_on(test_client_with_fetcher(
            &dir,
            Arc::new(MockSubscriptionFetcher::new()),
        ));

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
            client.rebuild_running_config().await.expect("reconcile");
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

            let lifecycle = client.runtime_lifecycle_state().await;
            assert!(
                lifecycle
                    .promoted
                    .as_ref()
                    .is_some_and(|current| current.identity_eq(promoted.as_ref()))
            );
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
        let endpoint = TestControlEndpoint::failing();
        let client =
            NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(&dir, endpoint))
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
                crate::client::runtime::DegradationPhase::RuntimeBuild
            );
            assert_eq!(degradations[0].code, "runtime_rebuild_failed");
            assert!(degradations[0].retryable);
            assert!(degradations[0].message.contains("reconcile boom"));
            let profiles = client.get_profiles().await.unwrap();
            assert_eq!(
                profiles.current.as_ref(),
                Some(&uid),
                "state stays committed"
            );
        });
    }

    /// Boot has to restore the persisted execution host -- the core actor
    /// always spawns on Local -- but only by adopting a daemon that is already
    /// up. Converging one here would install and start the service, raising a
    /// UAC prompt at every launch for a user who merely left the setting on,
    /// which the legacy `RunType` classification never did.
    #[test]
    fn boot_restore_adopts_only_an_already_ready_daemon() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let client =
            NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(&dir, endpoint))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.enable_service_mode = Some(true);
            client.patch_app_config(patch).await.unwrap();

            client.restore_execution_host().await.unwrap();

            assert_eq!(
                client.core_status().host,
                ExecutionHost::Local,
                "an absent daemon must not be installed and started by a boot restore"
            );
        });
    }

    #[test]
    fn legacy_regeneration_path_still_errors_on_rebuild_failure() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::failing();
        let client =
            NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(&dir, endpoint))
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
        let endpoint = TestControlEndpoint::succeeding();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
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
            client.activate_profile(Some(uid)).await.expect("activate");
            client.rebuild_running_config().await.expect("reconcile");
            assert!(endpoint.submissions() >= 1);
        });
    }

    #[test]
    fn core_status_is_projected_from_the_control_endpoint() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let client =
            NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(&dir, endpoint))
                .unwrap();
        tauri::async_runtime::block_on(async {
            for _ in 0..100 {
                if client.core_status().snapshot.is_some() {
                    break;
                }
                tokio::task::yield_now().await;
            }
            assert_eq!(client.core_status().host, ExecutionHost::Local);
        });
    }

    /// D5+P0-1 invariant: a failed check must leave the manager unpublished
    /// (product left untouched is proven by LegacyCoreBridge ordering + the
    /// promote atomicity unit test).
    #[test]
    fn failed_reconcile_keeps_the_committed_product_visible() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::failing();
        let client =
            NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(&dir, endpoint))
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
            // The desired product is committed before the control-plane operation.
            let _ = client.activate_profile(Some(uid)).await;
            let lifecycle = client.runtime_lifecycle_state().await;
            assert!(client.promoted_runtime().await.is_some());
            assert!(lifecycle.promoted.is_some());
        });
    }

    #[test]
    fn failed_reconcile_still_advances_the_promoted_read_model() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::failing();
        let client =
            NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(&dir, endpoint))
                .unwrap();

        tauri::async_runtime::block_on(async {
            let first = client
                .rebuild_running_config()
                .await
                .expect_err("reconcile fails");
            assert!(first.to_string().contains("reconcile boom"));
            let before = client.promoted_runtime().await.unwrap();
            let _ = client
                .rebuild_running_config()
                .await
                .expect_err("reconcile fails");
            let after = client.promoted_runtime().await.unwrap();
            assert!(after.revision > before.revision);
        });
    }

    #[test]
    fn boot_entrypoints_reconcile_the_current_desired_state() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();

        tauri::async_runtime::block_on(async {
            let promoted = client.promote_existing_runtime_product().await.unwrap();
            client.start_promoted_runtime().await.unwrap();
            assert!(client.promoted_runtime().await.unwrap().revision > promoted.revision);
            assert_eq!(endpoint.submissions(), 2);
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;

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
            let client = NyanpasuClient::with_parts(
                application,
                session_state,
                clash_config,
                profiles,
                Arc::new(MockProfileFsPort::new()),
                ports,
                dir.path().join("profiles"),
                RuntimePaths::from_resolver(&PathResolver::with_base_dirs(
                    dir.path().into(),
                    dir.path().join("data"),
                ))
                .unwrap(),
                Arc::new(crate::client::event_sink::NoopUiEventSink),
                Arc::new(UnusedLifecyclePort),
                Arc::new(NoopSystemDnsCache),
                crate::client::runtime::new_runtime_lifecycle_store()
                    .await
                    .expect("runtime state store"),
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;

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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(ok_fetch_without_name())).await;
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
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(ok_fetch_without_name())).await;
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
