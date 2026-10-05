mod app_lifecycle;
pub mod app_update;
mod application;
pub mod application_workflow;
mod clash_api;
mod clash_config;
mod clash_info;
mod clash_streams;
pub mod configuration_status;
pub mod convergence;
pub mod core_lifecycle;
mod direct_egress;
pub(crate) mod effects;
mod error;
mod event_sink;
pub mod hotkey;
pub(crate) mod jobs;
pub mod logs;
mod main_thread;
mod ports;
pub mod profiles;
pub mod runtime;
pub mod runtime_error;
pub mod runtime_inspection;
pub(crate) mod runtime_recovery;
mod session_state;
mod system_dns;
pub mod system_proxy;
mod traffic;
pub mod ui_effects;

use self::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    session_state::SessionStateClient,
};
use crate::{
    core::{
        actor_v2::{
            CoreClient as CoreClientV2, CoreStatusProjection,
            facade::{ReconcileReport, StopReport},
            service_actor::{ServiceClient, ServiceHostStatus},
        },
        backup::{
            self, BackupError, BackupInfo, BackupKind, BackupRequest, KEEP_MANUAL_BACKUPS,
            MANUAL_PREFIX, StorageSource,
        },
        storage::Storage,
    },
    service::profile_file::{ProfileFileService, SelfProxyPortSource},
    state::profiles::{
        CommitReport, NewProfileRequest, ProfileFileNotYamlSnafu, ProfileHasNoFileSnafu,
        ProfileNotFoundSnafu, ProfilesError, ReadProfileFileSnafu, RemoteProfileNeedsImportSnafu,
        ReorderOp,
        ports::{ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher},
    },
    utils::path::PathResolver,
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch},
    profile::{
        ProfileDefinition, ProfileId, ProfileMetadata, ProfileMetadataPatch, Profiles,
        RemoteProfileOptions, RemoteProfileOptionsPatch, TransformKind,
    },
    runtime::executor::ResolvedPortBindings,
};
use std::{path::PathBuf, sync::Arc};
use struct_patch::Patch as _;

pub(crate) use app_lifecycle::drain_on_shutdown;
pub use app_lifecycle::track_until_shutdown;
pub use clash_info::ClashInfo;
pub use direct_egress::{DirectEgress, DirectEgressProbe, HttpDirectEgressProbe};
pub use error::{ClientError, Result};
#[cfg(test)]
pub use event_sink::NoopUiEventSink;
pub use event_sink::{
    MainThreadHandoff, MainThreadWork, STATE_CHANGED_URI, StateChanged, TauriMainThread,
    TauriUiEventSink, UiEventSink,
};
pub use main_thread::MainThreadExecutor;
pub use ports::SessionPortResolver;
pub use runtime::RuntimePaths;
pub use runtime_error::RuntimeError;
#[cfg(test)]
pub use system_dns::{MockSystemDnsCache, NoopSystemDnsCache};
pub use system_dns::{OsSystemDnsCache, SystemDnsCache, SystemDnsError};
pub struct ClientSetupArgs {
    pub jobs: nyanpasu_jobs::JobsClient,
    pub bundle_metadata: crate::bundle::BundleMetadata,
    pub logging: logs::LoggingSetup,
    pub http_frontend: Option<crate::server::debug_http::Frontend>,
    pub http_routes: Arc<dyn crate::server::debug_http::HttpRoutes>,
    pub paths: PathResolver,
    /// Opened by the composition root, which also manages the same instance
    /// for the storage commands.
    pub storage: Storage,
    pub runtime_paths: RuntimePaths,
    pub ui_sink: Arc<dyn UiEventSink>,
    /// Built after the active proxy-port source exists in the composition root.
    pub app_update_backend_factory: Option<Arc<app_update::BackendFactory>>,
    pub app_update_event_sink: Option<Arc<dyn app_update::AppUpdateEventSink>>,
    pub core_v2: CoreClientV2,
    pub service: ServiceClient,
    pub system_dns: Arc<dyn SystemDnsCache>,
    pub direct_egress: Arc<dyn DirectEgressProbe>,
    pub geo_index: Arc<dyn crate::core::geo::CountryIndexSource>,
    pub os_proxy: Arc<dyn system_proxy::ports::OsProxyPort>,
    pub binary_installer: Arc<dyn core_lifecycle::ports::BinaryInstaller>,
    pub effects: Arc<dyn effects::ports::ApplicationEffectsPort>,
    pub window: Arc<dyn hotkey::ports::WindowControl>,
    pub accelerators: Arc<dyn hotkey::ports::AcceleratorValidator>,
    /// `None` disables traffic recording: the store could not be opened.
    pub traffic_store: Option<Arc<dyn nyanpasu_traffic::TrafficStore>>,
    /// The root shutdown token. The composition root owns it because some
    /// owners are spawned before the client; the client cancels it.
    pub shutdown: tokio_util::sync::CancellationToken,
    /// Every owner the shutdown waits for.
    pub tasks: tokio_util::task::TaskTracker,
}

#[derive(Clone)]
pub struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

async fn new_typed_config_clients(
    mutations: crate::state::mutation::MutationCoordinator,
    build_channel: crate::bundle::Channel,
    paths: PathResolver,
    shutdown: &tokio_util::sync::CancellationToken,
    tasks: &tokio_util::task::TaskTracker,
) -> anyhow::Result<(ApplicationClient, SessionStateClient, ClashConfigClient)> {
    let application = ApplicationClient::new(
        mutations.clone(),
        build_channel,
        utf8_path(paths.application_config_path())?,
        shutdown.child_token(),
        tasks,
    )
    .await?;

    let session_state = SessionStateClient::new(
        utf8_path(paths.session_state_path())?,
        shutdown.child_token(),
        tasks,
    )
    .await?;

    let clash_config = ClashConfigClient::new(
        mutations.clone(),
        utf8_path(paths.clash_config_path())?,
        shutdown.child_token(),
        tasks,
    )
    .await?;

    Ok((application, session_state, clash_config))
}

#[cfg(not(test))]
fn runtime_core_spec(
    core: &nyanpasu_config::application::ClashCore,
) -> std::result::Result<
    nyanpasu_core_manager::CoreSpec,
    crate::core::actor_v2::local_host::CoreSpecError,
> {
    crate::core::actor_v2::local_host::core_spec(core)
}

#[cfg(test)]
fn runtime_core_spec(
    core: &nyanpasu_config::application::ClashCore,
) -> std::result::Result<
    nyanpasu_core_manager::CoreSpec,
    crate::core::actor_v2::local_host::CoreSpecError,
> {
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
    debug_http: crate::server::debug_http::HttpServerClient,
    bundle_metadata: crate::bundle::BundleMetadata,
    core_logs: crate::core::logs::CoreLogsClient,
    app_logs: nyanpasu_logging::LogsClient,
    jobs: nyanpasu_jobs::JobsClient,
    service_logs: Arc<dyn logs::ServiceLogsPort>,
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    profiles: profiles::ProfilesClient,
    fs: Arc<dyn ProfileFsPort>,
    ports: Arc<SessionPortResolver>,
    profiles_dir: PathBuf,
    paths: PathResolver,
    storage: Storage,
    application_workflow: application_workflow::ApplicationWorkflowClient,
    core_api: CoreClientV2,
    proxies: crate::core::proxies::ProxiesClient,
    streams: crate::core::clash::ws::StreamsClient,
    traffic: Option<crate::core::traffic::TrafficClient>,
    updater: crate::core::updater::UpdaterClient,
    app_updater: app_update::AppUpdateClient,
    system_dns: Arc<dyn SystemDnsCache>,
    direct_egress: Arc<dyn DirectEgressProbe>,
    local_source: Arc<traffic::LocalSourceCache>,
    /// Held for its actor, which stops with the last handle; the traffic pump only holds the
    /// index it publishes.
    _geo_index: crate::core::geo::GeoIndexClient,
    os_proxy: Arc<dyn system_proxy::ports::OsProxyPort>,
    effects: effects::actor::EffectsClient,
    window: Arc<dyn hotkey::ports::WindowControl>,
    /// The platform's accelerator rule, used to reject a hotkey list before it
    /// is committed rather than after the effect has torn the old grabs down.
    accelerators: Arc<dyn hotkey::ports::AcceleratorValidator>,
    /// The root shutdown token; `request_shutdown` cancels it.
    shutdown: tokio_util::sync::CancellationToken,
    /// Every owner the shutdown waits for.
    tasks: tokio_util::task::TaskTracker,
}

impl NyanpasuClient {
    pub fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            jobs,
            bundle_metadata,
            logging,
            http_frontend,
            http_routes,
            paths,
            storage,
            runtime_paths,
            ui_sink,
            app_update_backend_factory,
            app_update_event_sink,
            core_v2,
            service,
            system_dns,
            direct_egress,
            geo_index,
            os_proxy,
            binary_installer,
            effects,
            window,
            accelerators,
            traffic_store,
            shutdown,
            tasks,
        } = args;
        let profiles_dir = paths.app_profiles_dir();
        let backup_paths = paths.clone();
        let instance_config_dir = paths.app_config_dir().to_path_buf();
        let script_dirs = crate::enhance::ScriptDirs::from_resolver(&paths);
        let profiles_path = utf8_path(paths.profiles_path())?;
        let runtime_paths_for_setup = runtime_paths.clone();
        let jobs_for_setup = jobs.clone();
        let mutations = crate::state::mutation::MutationCoordinator::pending();
        let wiring = mutations.clone();
        let (owner_shutdown, owner_tasks) = (shutdown.clone(), tasks.clone());
        let (application, session_state, clash_config, profiles, ports, fs) =
            tauri::async_runtime::block_on(async move {
                runtime_paths_for_setup
                    .cleanup_stale_candidates(std::time::Duration::from_secs(24 * 60 * 60))
                    .await
                    .context("failed to clean stale runtime candidates")?;
                let (application, session_state, clash_config) = new_typed_config_clients(
                    mutations.clone(),
                    bundle_metadata.release_channel,
                    paths.clone(),
                    &owner_shutdown,
                    &owner_tasks,
                )
                .await?;

                // No eager resolution: a pick nothing has applied is a
                // candidate, and a candidate must never be readable as the
                // active binding. The first reconcile resolves and confirms.
                let ports = Arc::new(SessionPortResolver::default());

                let file_service = Arc::new(ProfileFileService::new(
                    paths,
                    ports.clone() as Arc<dyn SelfProxyPortSource>,
                ));
                let profiles = profiles::ProfilesClient::new_with_jobs(
                    mutations.clone(),
                    profiles_path,
                    jobs_for_setup,
                    file_service.clone() as Arc<dyn ProfileFsPort>,
                    file_service.clone() as Arc<dyn SubscriptionFetcher>,
                    file_service.clone() as Arc<dyn ProfileMaterializationPort>,
                    owner_shutdown.child_token(),
                    &owner_tasks,
                )
                .await?;
                anyhow::Ok((
                    application,
                    session_state,
                    clash_config,
                    profiles,
                    ports,
                    file_service as Arc<dyn ProfileFsPort>,
                ))
            })?;
        tauri::async_runtime::block_on(Self::with_parts(
            Some(wiring),
            bundle_metadata,
            logging,
            jobs,
            application,
            session_state,
            clash_config,
            profiles,
            fs,
            ports,
            profiles_dir,
            instance_config_dir,
            backup_paths,
            storage,
            runtime_paths,
            script_dirs,
            ui_sink,
            app_update_backend_factory,
            app_update_event_sink,
            core_v2,
            service,
            system_dns,
            direct_egress,
            geo_index,
            os_proxy,
            binary_installer,
            effects,
            window,
            accelerators,
            http_frontend,
            http_routes,
            traffic_store,
            shutdown,
            tasks,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn with_parts(
        mutations: Option<crate::state::mutation::MutationCoordinator>,
        bundle_metadata: crate::bundle::BundleMetadata,
        logging: logs::LoggingSetup,
        jobs: nyanpasu_jobs::JobsClient,
        application: ApplicationClient,
        session_state: SessionStateClient,
        clash_config: ClashConfigClient,
        profiles: profiles::ProfilesClient,
        fs: Arc<dyn ProfileFsPort>,
        ports: Arc<SessionPortResolver>,
        profiles_dir: PathBuf,
        instance_config_dir: PathBuf,
        paths: PathResolver,
        storage: Storage,
        runtime_paths: RuntimePaths,
        script_dirs: crate::enhance::ScriptDirs,
        ui_sink: Arc<dyn UiEventSink>,
        app_update_backend_factory: Option<Arc<app_update::BackendFactory>>,
        app_update_event_sink: Option<Arc<dyn app_update::AppUpdateEventSink>>,
        core_v2: CoreClientV2,
        service: ServiceClient,
        system_dns: Arc<dyn SystemDnsCache>,
        direct_egress: Arc<dyn DirectEgressProbe>,
        geo_index: Arc<dyn crate::core::geo::CountryIndexSource>,
        os_proxy: Arc<dyn system_proxy::ports::OsProxyPort>,
        binary_installer: Arc<dyn core_lifecycle::ports::BinaryInstaller>,
        effects: Arc<dyn effects::ports::ApplicationEffectsPort>,
        window: Arc<dyn hotkey::ports::WindowControl>,
        accelerators: Arc<dyn hotkey::ports::AcceleratorValidator>,
        http_frontend: Option<crate::server::debug_http::Frontend>,
        http_routes: Arc<dyn crate::server::debug_http::HttpRoutes>,
        traffic_store: Option<Arc<dyn nyanpasu_traffic::TrafficStore>>,
        shutdown: tokio_util::sync::CancellationToken,
        tasks: tokio_util::task::TaskTracker,
    ) -> anyhow::Result<Self> {
        let debug_http =
            crate::server::debug_http::HttpServerClient::spawn(http_frontend, http_routes).await?;
        tasks.spawn({
            let (http, token) = (debug_http.clone(), shutdown.child_token());
            async move {
                token.cancelled().await;
                if let Err(error) = http.shutdown().await {
                    tracing::warn!(%error, "debug HTTP server shutdown failed");
                }
            }
        });
        let core_logs = crate::core::logs::CoreLogsClient::spawn(
            logging.core,
            application.snapshot().state.core_logs,
            shutdown.child_token(),
            &tasks,
        )
        .await?;
        // Before the effects owner, which hands it the capture level.
        let streams = crate::core::clash::ws::StreamsClient::spawn(
            core_v2.clone(),
            core_logs.clone(),
            clash_config.snapshot().state.overrides.log_level(),
            shutdown.child_token(),
            &tasks,
        )
        .await?;
        let app_logs = nyanpasu_logging::LogsClient::start(logging.files, logging.clock).await?;
        // The log client exposes no actor cell, so a tracked task stops it.
        tasks.spawn({
            let (logs, token) = (app_logs.clone(), shutdown.child_token());
            async move {
                token.cancelled().await;
                if let Err(error) = logs.shutdown().await {
                    tracing::warn!("the application log did not shut down cleanly: {error}");
                }
            }
        });
        let service_logs = logging.service;
        let effects = effects::actor::EffectsClient::spawn(
            effects::actor::EffectsArgs {
                port: Arc::new(effects::executor::CoreLogCaptureEffects::new(
                    effects,
                    streams.clone(),
                    core_logs.clone(),
                )),
                ui: ui_sink,
                initial: effects::plan::ApplicationEffectInputs::project(
                    &application.snapshot().state,
                    &clash_config.snapshot().state,
                    ports.confirmed(),
                ),
                shutdown: shutdown.child_token(),
            },
            &tasks,
        )
        .await?;
        let application_workflow = application_workflow::ApplicationWorkflowClient::spawn(
            application_workflow::ApplicationWorkflowArgs {
                notifications: Arc::new(effects.clone()),
                application: application.snapshot_handle(),
                clash: clash_config.snapshot_handle(),
                profiles: profiles.snapshot_handle(),
                core: core_v2.clone(),
                service,
                builder: Arc::new(application_workflow::adapters::FsRuntimeBuildAdapter {
                    profiles_dir: profiles_dir.clone(),
                    paths: runtime_paths.clone(),
                    scripts: script_dirs,
                }),
                validator: Arc::new(application_workflow::adapters::CoreCheckValidator::new(
                    core_v2.clone(),
                    runtime_paths,
                )),
                ports: ports.clone(),
                installer: binary_installer,
                ownership: core_lifecycle::Ownership::Unproven,
                instance_config_dir,
                shutdown: shutdown.child_token(),
                tasks: tasks.clone(),
            },
        )
        .await?;
        if let Some(mutations) = mutations {
            mutations.connect(application_workflow.clone(), Arc::new(effects.clone()));
        }
        let updater = crate::core::updater::UpdaterClient::spawn(
            Arc::new(crate::core::updater::HttpUpdaterBackend::new(
                std::env::current_exe()?
                    .parent()
                    .context("executable has no parent directory")?
                    .to_path_buf(),
                ports.clone(),
            )),
            Arc::new(application_workflow.clone()),
            shutdown.child_token(),
            &tasks,
        )
        .await?;
        let app_config = application.snapshot().state;
        let app_update_settings = app_update::AppUpdateSettings {
            channel: app_config
                .release_channel
                .unwrap_or(bundle_metadata.release_channel),
            sources: app_config.update_sources.clone(),
            auto_check: app_config.enable_auto_check_update,
            auto_download: app_config.enable_auto_download_update,
        };
        let app_update_supported = app_update_backend_factory.is_some()
            && !bundle_metadata.is_portable
            && (cfg!(any(target_os = "windows", target_os = "macos"))
                || (cfg!(target_os = "linux") && *crate::consts::IS_APPIMAGE));
        let app_update_backend = app_update_backend_factory
            .map(|factory| factory(ports.clone() as Arc<dyn SelfProxyPortSource>))
            .unwrap_or_else(|| Arc::new(app_update::UnavailableAppUpdateBackend));
        let app_update_events =
            app_update_event_sink.unwrap_or_else(|| Arc::new(app_update::NoopAppUpdateEventSink));
        let app_updater = app_update::AppUpdateClient::spawn(
            app_update::AppUpdateArgs {
                backend: app_update_backend,
                events: app_update_events,
                settings: app_update_settings,
                supported: app_update_supported,
                endpoints: crate::bundle::update_endpoints(
                    app_config
                        .release_channel
                        .unwrap_or(bundle_metadata.release_channel),
                ),
                shutdown: shutdown.child_token(),
            },
            &tasks,
        )
        .await?;
        let mut application_settings = application.subscribe_settings_changes();
        let settings_updater = app_updater.clone();
        let settings_shutdown = shutdown.child_token();
        let installed_channel = bundle_metadata.release_channel;
        tasks.spawn(async move {
            loop {
                tokio::select! {
                    () = settings_shutdown.cancelled() => break,
                    changed = application_settings.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        let config = application_settings.borrow_and_update().clone();
                        settings_updater.configure(app_update::AppUpdateSettings {
                            channel: config.release_channel.unwrap_or(installed_channel),
                            sources: config.update_sources,
                            auto_check: config.enable_auto_check_update,
                            auto_download: config.enable_auto_download_update,
                        });
                    }
                }
            }
        });
        let proxies = crate::core::proxies::ProxiesClient::spawn(
            core_v2.clone(),
            shutdown.child_token(),
            &tasks,
        )
        .await?;
        let geo_index = crate::core::geo::GeoIndexClient::spawn(
            crate::core::geo::GeoIndexArgs {
                source: geo_index,
                core: core_v2.clone(),
            },
            shutdown.child_token(),
            &tasks,
        )
        .await?;
        let local_source = Arc::new(traffic::LocalSourceCache::new(
            application.snapshot_handle(),
            clash_config.snapshot_handle(),
        ));
        let traffic = match traffic_store {
            Some(store) => {
                match crate::core::traffic::TrafficClient::spawn(
                    crate::core::traffic::TrafficArgs {
                        store,
                        profiles: Arc::new(traffic::SelectedProfile::new(
                            profiles.snapshot_handle(),
                        )),
                        retention: Arc::new(traffic::SettingsRetention::new(
                            application.snapshot_handle(),
                        )),
                        clock: Arc::new(traffic::SystemClock),
                        frames: streams.subscribe_connection_frames(),
                        geo: geo_index.subscribe(),
                        local_source: local_source.clone(),
                    },
                    shutdown.child_token(),
                    &tasks,
                )
                .await
                {
                    Ok(client) => Some(client),
                    // Recording is a side feature: an unreadable store disables it, not the app.
                    Err(error) => {
                        tracing::warn!("traffic recording is disabled: {error:#}");
                        None
                    }
                }
            }
            None => None,
        };
        Ok(Self {
            inner: Arc::new(NyanpasuClientInner {
                debug_http,
                bundle_metadata,
                core_logs,
                app_logs,
                jobs,
                service_logs,
                application,
                session_state,
                clash_config,
                profiles,
                fs,
                ports,
                profiles_dir,
                paths,
                storage,
                application_workflow,
                core_api: core_v2,
                proxies,
                streams,
                traffic,
                updater,
                app_updater,
                system_dns,
                direct_egress,
                local_source,
                _geo_index: geo_index,
                os_proxy,
                effects,
                window,
                accelerators,
                shutdown,
                tasks,
            }),
        })
    }

    pub async fn debug_http_status(
        &self,
    ) -> anyhow::Result<crate::server::debug_http::DebugHttpStatus> {
        self.inner.debug_http.status().await
    }
    pub async fn set_debug_http_enabled(
        &self,
        enabled: bool,
    ) -> anyhow::Result<crate::server::debug_http::DebugHttpStatus> {
        self.inner.debug_http.set_enabled(enabled).await
    }
    pub async fn shutdown_debug_http(&self) -> anyhow::Result<()> {
        self.inner.debug_http.set_enabled(false).await?;
        Ok(())
    }

    pub async fn release_channel(&self) -> Result<crate::bundle::Channel> {
        Ok(self
            .inner
            .bundle_metadata
            .release_channel
            .resolve(self.inner.application.snapshot().state.release_channel))
    }

    pub fn installed_release_channel(&self) -> crate::bundle::Channel {
        self.inner.bundle_metadata.release_channel
    }

    pub async fn get_app_update_state(&self) -> Result<app_update::AppUpdateSnapshot> {
        Ok(self.inner.app_updater.state().await?)
    }

    pub async fn check_app_update(&self) -> Result<app_update::AppUpdateSnapshot> {
        Ok(self.inner.app_updater.check().await?)
    }

    pub async fn download_app_update(&self) -> Result<app_update::AppUpdateSnapshot> {
        Ok(self.inner.app_updater.download().await?)
    }

    pub async fn cancel_app_update_download(&self) -> Result<app_update::AppUpdateSnapshot> {
        Ok(self.inner.app_updater.cancel_download().await?)
    }

    pub async fn install_app_update(&self) -> Result<app_update::AppUpdateSnapshot> {
        Ok(self.inner.app_updater.install().await?)
    }

    pub async fn discard_app_update_package(&self) -> Result<app_update::AppUpdateSnapshot> {
        Ok(self.inner.app_updater.discard().await?)
    }

    pub async fn set_release_channel(
        &self,
        channel: crate::bundle::Channel,
    ) -> Result<runtime::MutationOutcome<()>> {
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.release_channel = Some(Some(channel));
        self.patch_app_config(patch).await
    }

    /// Creates a manual backup and keeps the newest `KEEP_MANUAL_BACKUPS` of them.
    /// A failed prune is logged and does not fail the backup it follows.
    pub async fn create_config_backup(&self) -> std::result::Result<BackupInfo, BackupError> {
        let (paths, storage) = (self.inner.paths.clone(), self.inner.storage.clone());
        crate::utils::blocking::join(
            tokio::task::spawn_blocking(move || {
                let info = backup::create_backup(&BackupRequest {
                    paths: &paths,
                    storage: StorageSource::Live(&storage),
                    kind: BackupKind::Manual,
                    now: time::OffsetDateTime::now_utc(),
                })?;
                if let Err(error) =
                    backup::prune_backups(&paths.backups_dir(), MANUAL_PREFIX, KEEP_MANUAL_BACKUPS)
                {
                    tracing::warn!(%error, "failed to prune old manual backups");
                }
                Ok(info)
            })
            .await,
        )
    }

    /// `<data>/backups`, created when it does not exist yet.
    pub fn backups_dir(&self) -> std::io::Result<PathBuf> {
        let dir = self.inner.paths.backups_dir();
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn is_portable(&self) -> bool {
        self.inner.bundle_metadata.is_portable
    }

    pub async fn get_app_config(&self) -> Result<NyanpasuAppConfig> {
        Ok(self.inner.application.snapshot().state)
    }

    /// The committed application config, for boundary code that runs
    /// synchronously (window creation, window event handlers) and cannot await.
    pub fn app_config_snapshot(&self) -> NyanpasuAppConfig {
        self.inner.application.snapshot().state
    }

    pub async fn reconcile_core(&self) -> std::result::Result<ReconcileReport, RuntimeError> {
        self.inner.application_workflow.reconcile().await
    }

    // No UI entry issues an explicit stop yet; this is the facade entry to the
    // explicit stop intent the workflow honours (TCC V11, T10 S18 / §1.4).
    #[allow(dead_code)]
    pub async fn stop_core(&self) -> std::result::Result<StopReport, RuntimeError> {
        self.inner.application_workflow.stop_core().await
    }

    /// Select the core through the application source transaction.
    pub async fn update_core(
        &self,
        core: nyanpasu_config::application::ClashCore,
    ) -> Result<runtime::MutationOutcome<()>> {
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.core = Some(core);
        self.patch_app_config(patch).await
    }
    pub fn core_status(&self) -> CoreStatusProjection {
        self.inner.application_workflow.core_status()
    }
    pub fn subscribe_core_events(&self) -> tokio::sync::broadcast::Receiver<CoreStatusProjection> {
        self.inner.application_workflow.core_events()
    }
    pub fn service_status(&self) -> ServiceHostStatus {
        self.inner.application_workflow.service_status()
    }
    pub fn subscribe_service_events(&self) -> tokio::sync::watch::Receiver<ServiceHostStatus> {
        self.inner.application_workflow.service_events()
    }
    pub async fn install_service(&self) -> std::result::Result<(), RuntimeError> {
        self.inner.application_workflow.install_service().await
    }

    pub async fn start_service(&self) -> std::result::Result<(), RuntimeError> {
        self.inner.application_workflow.start_service().await
    }

    pub async fn stop_service(&self) -> std::result::Result<(), RuntimeError> {
        self.inner.application_workflow.stop_service().await
    }

    pub async fn restart_service(&self) -> std::result::Result<(), RuntimeError> {
        self.inner.application_workflow.restart_service().await
    }

    pub async fn uninstall_service(&self) -> std::result::Result<(), RuntimeError> {
        self.inner.application_workflow.uninstall_service().await
    }

    pub async fn fetch_latest_core_versions(
        &self,
    ) -> Result<crate::core::updater::ManifestVersionLatest> {
        Ok(self.inner.updater.fetch_latest().await?)
    }

    pub async fn download_core_update(
        &self,
        core: nyanpasu_config::application::ClashCore,
    ) -> Result<usize> {
        Ok(self.inner.updater.update(core).await?)
    }

    pub async fn inspect_updater(&self, id: usize) -> Result<crate::core::updater::UpdaterSummary> {
        Ok(self.inner.updater.inspect(id).await?)
    }

    /// The proxy settings the OS holds right now, whoever wrote them.
    ///
    /// Read straight from the port rather than through the system proxy
    /// actor: its mailbox is held for the whole of a PAC download, which
    /// would stall a status poll behind it.
    pub async fn get_os_proxy(
        &self,
    ) -> std::result::Result<system_proxy::ports::OsProxyConfig, system_proxy::ports::OsProxyError>
    {
        let os_proxy = self.inner.os_proxy.clone();
        crate::utils::blocking::join(tokio::task::spawn_blocking(move || os_proxy.get()).await)
    }

    pub async fn flush_system_dns_cache(&self) -> std::result::Result<(), SystemDnsError> {
        let system_dns = self.inner.system_dns.clone();
        crate::utils::blocking::join(tokio::task::spawn_blocking(move || system_dns.flush()).await)
    }

    /// A caller-triggered probe. Traffic consumes the result without waiting for network IO.
    pub async fn probe_direct_egress(&self) -> Result<DirectEgress> {
        let cache = &self.inner.local_source;
        if let Some(blocked) = cache.blocked() {
            cache.set(&blocked);
            return Ok(blocked);
        }
        let probe = &self.inner.direct_egress;
        let (ipv4, ipv6) = tokio::join!(probe.ipv4(), probe.ipv6());
        // Permission or TUN may change while the requests are in flight.
        let result = cache
            .blocked()
            .unwrap_or(DirectEgress::Probed { ipv4, ipv6 });
        cache.set(&result);
        Ok(result)
    }

    pub async fn patch_app_config(
        &self,
        patch: NyanpasuAppConfigPatch,
    ) -> Result<runtime::MutationOutcome<()>> {
        if let Some(hotkeys) = patch.hotkeys.as_deref() {
            hotkey::validate_bindings(hotkeys, self.inner.accelerators.as_ref())?;
        }
        let client = self.inner.application.clone();
        Ok(client.patch(patch).await?.outcome())
    }

    pub async fn retry_runtime_now(&self) -> std::result::Result<(), RuntimeError> {
        self.inner.application_workflow.retry_runtime().await
    }

    pub fn retry_effect_now(
        &self,
        kind: effects::plan::EffectKind,
    ) -> std::result::Result<(), effects::error::EffectsError> {
        self.inner.effects.retry_now(kind)
    }

    pub async fn save_main_window_geometry(
        &self,
        geometry: nyanpasu_config::state::window::WindowState,
    ) -> Result<runtime::MutationOutcome<()>> {
        let snapshot = self.inner.session_state.save_main_window(geometry).await?;
        Ok(
            runtime::MutationOutcome::from_parts((), Vec::new()).with_commit(
                runtime::CommitReceipt {
                    operation_id: None,
                    domain: "session".into(),
                    source_version: snapshot.version,
                    runtime: runtime::RuntimeCommitStatus::Unchanged,
                },
            ),
        )
    }

    /// Queues a save of the main window's geometry; nothing waits for it.
    pub fn queue_main_window_geometry_save(
        &self,
        geometry: nyanpasu_config::state::window::WindowState,
    ) -> Result<()> {
        self.inner.session_state.queue_main_window_save(geometry)?;
        Ok(())
    }

    /// The geometry the main window reopens with, as last saved.
    pub fn main_window_geometry(&self) -> Option<nyanpasu_config::state::window::WindowState> {
        self.inner.session_state.main_window_geometry()
    }

    pub async fn get_clash_config(&self) -> Result<ClashConfig> {
        Ok(self.inner.clash_config.snapshot().state)
    }

    pub fn clash_info(&self) -> ClashInfo {
        ClashInfo::derive(
            self.session_ports().as_ref(),
            &self.inner.clash_config.snapshot().state,
        )
    }

    pub async fn patch_runtime_overrides(
        &self,
        patch: nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch,
    ) -> Result<runtime::MutationOutcome<()>> {
        let client = self.inner.clash_config.clone();
        let outcome = client.patch_overrides(patch).await?.outcome();
        self.request_proxy_refresh();
        Ok(outcome)
    }

    pub async fn patch_clash_config(
        &self,
        patch: ClashConfigPatch,
    ) -> Result<runtime::MutationOutcome<()>> {
        let client = self.inner.clash_config.clone();
        Ok(client.patch(patch).await?.outcome())
    }

    // ---- profiles domain (PR-3 T07) ----

    pub async fn get_profiles(&self) -> Result<Arc<Profiles>> {
        Ok(self.inner.profiles.snapshot())
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
                application_workflow::profiles::map_profile_degradation(degradation)
            })
            .collect();
        degradations.extend(report.runtime_degradations.clone());
        degradations
    }

    async fn after_commit(&self, report: &CommitReport) -> runtime::MutationOutcome<()> {
        runtime::MutationOutcome::from_parts(
            (),
            self.collect_post_commit_degradations(report).await,
        )
        .with_commit(report.receipt.clone())
    }

    /// Public wire for a post-commit auto-activation hard failure. Create/import
    /// already committed the profile, so this must never become `Err` that erases
    /// the `ProfileId`. VersionConflict is not special-cased as success.
    fn auto_activation_failure_degradation(
        profile: ProfileId,
        error: ProfilesError,
    ) -> runtime::Degradation {
        tracing::warn!(
            %error,
            "profile auto-activation failed after commit; retaining committed profile id",
        );
        runtime::Degradation {
            phase: runtime::DegradationPhase::SystemEffect,
            message: error.to_string(),
            reason: runtime::DegradationReason::ProfileAutoActivationFailed {
                profile,
                cause: Arc::new(error),
            },
            // Activation can be retried via activate_profile / set_current; even
            // VersionConflict is a transient CAS race, not a permanent rejection.
            retryable: true,
        }
    }

    /// Shared create/import post-commit auto-activation protocol:
    /// - `Ok(Some(report))` → merge report (and rebuild) degradations
    /// - `Ok(None)` → existing current won; no degradation
    /// - `Err(_)` → committed degradation, profile id retained by the caller
    async fn try_auto_activate_if_none(&self, uid: ProfileId) -> runtime::MutationOutcome<()> {
        // The conditional stays atomic inside the profiles actor: a facade-level
        // read-then-write could lose a concurrent selection.
        let report = match self.inner.profiles.set_current_if_none(uid.clone()).await {
            Ok(None) => return runtime::MutationOutcome::from_parts((), Vec::new()),
            Ok(Some(report)) => report,
            Err(error) => {
                return runtime::MutationOutcome::from_parts(
                    (),
                    vec![Self::auto_activation_failure_degradation(uid, error)],
                );
            }
        };
        self.after_commit(&report).await
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
            return Err(RemoteProfileNeedsImportSnafu.build().into());
        }
        let report = self.inner.profiles.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .expect("an add commits the uid it generated");
        Ok(runtime::MutationOutcome::from_parts(
            created,
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone()))
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
            outcome = outcome.append_commit_result(self.try_auto_activate_if_none(uid).await);
        }
        Ok(outcome)
    }

    /// Import a remote subscription via actor-owned fetch-before-commit. A
    /// `None` `transform` imports a Config File and auto-activates it when
    /// nothing is current; `Some` imports a Transform of that kind, which is
    /// never activatable.
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
        transform: Option<TransformKind>,
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
                transform,
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
            .expect("an import commits the uid it generated");
        let outcome = runtime::MutationOutcome::from_parts(
            created.clone(),
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone());
        if transform.is_some() {
            return Ok(outcome);
        }
        Ok(outcome.append_commit_result(self.try_auto_activate_if_none(created).await))
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
        let report = self.inner.profiles.sync(uid, patch).await?;
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
        let snapshot = self.inner.profiles.snapshot();
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfileNotFoundSnafu { uid: uid.clone() }.build())?;
        let source = item
            .definition
            .source()
            .ok_or_else(|| ProfileHasNoFileSnafu { uid }.build())?;
        Ok(self
            .inner
            .profiles_dir
            .join(source.materialized().file.as_path()))
    }

    pub async fn read_profile_file(&self, uid: ProfileId) -> Result<String> {
        let snapshot = self.inner.profiles.snapshot();
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfileNotFoundSnafu { uid: uid.clone() }.build())?;
        let source = item
            .definition
            .source()
            .ok_or_else(|| ProfileHasNoFileSnafu { uid: uid.clone() }.build())?;
        let raw = snafu::ResultExt::context(
            self.inner.fs.read(&source.materialized().file),
            ReadProfileFileSnafu { uid: uid.clone() },
        )?;
        match &item.definition {
            ProfileDefinition::Config { .. } => Ok(snafu::ResultExt::context(
                crate::service::profile_file::normalize_yaml_document(&raw),
                ProfileFileNotYamlSnafu { uid },
            )?),
            ProfileDefinition::Transform { .. } => Ok(raw),
        }
    }

    pub async fn save_profile_file(
        &self,
        uid: ProfileId,
        data: String,
    ) -> Result<runtime::MutationOutcome<()>> {
        let report = self.inner.profiles.save_file(uid, data).await?;
        Ok(self.after_commit(&report).await)
    }

    /// The confirmed port binding, or `None` when no instance is known to be
    /// listening. A candidate resolution never shows up here.
    pub fn session_ports(&self) -> Option<ResolvedPortBindings> {
        self.inner.ports.confirmed()
    }

    pub async fn promoted_runtime(&self) -> Option<Arc<runtime::RuntimeSnapshot>> {
        self.inner.application_workflow.runtime().promoted
    }
}

fn utf8_path(path: PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

#[async_trait::async_trait]
impl crate::core::updater::ports::CoreUpdateInstaller
    for application_workflow::ApplicationWorkflowClient
{
    async fn install(
        &self,
        artifact: core_lifecycle::ports::PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        Ok(self.replace_binary(artifact).await?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        client::system_proxy::ports::{MockOsProxyPort, OsProxyConfig, OsProxyError, OsProxyPort},
        core::actor_v2::endpoint::ExecutionHost,
        state::profiles::{
            error::SubscriptionFetchError,
            ports::{
                CleanupOutcome, MaterializationReconcileReport, MockProfileFsPort,
                MockProfileMaterializationPort, MockSubscriptionFetcher, PreparedCleanup,
                PreparedMaterialization, ProfileMaterializationPort,
            },
        },
    };
    use camino::Utf8PathBuf;
    use nyanpasu_config::{
        clash::config::{ClashConfig, clash_strategy::PortStrategy},
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileMetadata, ProfileSource, SubscriptionInfo,
        },
        state::window::WindowState,
    };
    use std::sync::Mutex as StdMutex;
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    struct IdleEndpoint;

    #[async_trait::async_trait]
    impl crate::core::actor_v2::endpoint::ControlEndpoint for IdleEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Local
        }

        async fn submit(
            &self,
            submission: crate::core::actor_v2::endpoint::CoreSubmission,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::v2::OperationInfo,
            nyanpasu_core_manager::CoreError,
        > {
            Ok(successful_reconcile(submission.envelope.operation_id))
        }

        async fn wait_operation(
            &self,
            id: nyanpasu_core_manager::OperationId,
            _timeout: std::time::Duration,
        ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
            Some(successful_reconcile(id))
        }

        async fn status(
            &self,
        ) -> std::result::Result<
            crate::core::actor_v2::endpoint::CoreStatusSnapshot,
            nyanpasu_core_manager::CoreError,
        > {
            Ok(crate::core::actor_v2::endpoint::CoreStatusSnapshot {
                controller: None,
                state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                state_changed_at: 0,
                revision: None,
                source_hash: None,
                healthy: Some(true),
                applied_kind: None,
            })
        }
    }

    pub(crate) struct TestControlEndpoint {
        /// Which host this endpoint claims to be. A graph that moves the
        /// runtime between hosts needs two of these, and the verification of a
        /// restore compares the host the receipt names with the one that
        /// answered.
        host: ExecutionHost,
        fail: std::sync::atomic::AtomicBool,
        failure_kind: StdMutex<Option<&'static str>>,
        effective_enabled: std::sync::atomic::AtomicBool,
        effective_queries: std::sync::atomic::AtomicUsize,
        submissions: std::sync::atomic::AtomicUsize,
        pub(crate) local_ipc: StdMutex<Option<nyanpasu_core_manager::LocalIpcSettings>>,
        /// The endpoint's current applied revision (finding 2's CAS
        /// enforcement). Starts non-`None` so the router's very first pump
        /// read seeds the projection with a real baseline, and advances on
        /// every accepted reconcile so a stale `expected_applied` can be
        /// told apart from a fresh one the way the real runtime's
        /// `apply_config` does.
        revision: StdMutex<nyanpasu_ipc::api::status::RevisionIdInfo>,
        /// The source identity of that same revision. Tracked separately
        /// because the two move independently: a restart re-stamps the
        /// epoch-specific controller endpoint into the effective document
        /// while the configuration it came from is unchanged.
        source_hash: StdMutex<String>,
        /// Terminal results, keyed by operation id, computed once at
        /// `submit` time so `wait_operation` replays that decision instead
        /// of re-running (and re-advancing) the CAS check.
        operations:
            StdMutex<std::collections::HashMap<String, nyanpasu_ipc::api::core::v2::OperationInfo>>,
        /// What `status()` reports for `state` and `applied_kind` (R5): the
        /// stop-decision tests script the host's applied identity
        /// independently of this fake's CAS bookkeeping. Defaults to
        /// `(None, None)` -- unknown state, unknown applied kind -- the same
        /// starting point a brand new endpoint the router has not yet
        /// classified would report, which the production rule already
        /// treats conservatively as "not proven stopped".
        status_override: StdMutex<(
            Option<nyanpasu_ipc::api::status::CoreStateDetail>,
            Option<nyanpasu_core_manager::CoreKind>,
        )>,
        /// Scripts whether the next `Recover` submission fails (R6a): the
        /// death-proof step must abort
        /// `ApplicationWorkflowClient::replace_binary` before the installer
        /// runs when recovery itself cannot prove the core is dead.
        /// Defaults to `false` so existing tests, which never scripted
        /// `Recover` before it became an unconditional step, keep seeing it
        /// succeed.
        recover_should_fail: std::sync::atomic::AtomicBool,
        result_missing: std::sync::atomic::AtomicBool,
        /// Every advisory check this endpoint was asked to run, so a test can
        /// compare what the check saw with what the reconcile submitted.
        checks: StdMutex<Vec<crate::core::actor_v2::endpoint::CheckSubmission>>,
        /// What the next check answers. The default mirrors the in-process
        /// control plane accepting a document.
        check_answer: StdMutex<TestCheckAnswer>,
        /// The config bytes of every reconcile, so a test can prove the check
        /// and the apply consumed the same document.
        reconciled: StdMutex<Vec<Vec<u8>>>,
        /// Scripts the runtime answering `RolledBack`: the request failed and
        /// the manager put itself back on whatever it was already running, so
        /// the tracked revision does not advance.
        rolls_back: std::sync::atomic::AtomicBool,
        /// Scripts `status()` failing, which is what an endpoint that stopped
        /// answering looks like to a caller that needs a fresh observation.
        status_fails: std::sync::atomic::AtomicBool,
        status_reads: std::sync::atomic::AtomicUsize,
    }

    /// The three things a host can say about a candidate document.
    #[derive(Clone)]
    pub(crate) enum TestCheckAnswer {
        Pass,
        Reject(nyanpasu_core_manager::CoreError),
        Unsupported,
        /// The host accepts the request and never answers, the shape of a
        /// wedged binary or a daemon that stopped responding.
        Hang,
    }

    impl TestControlEndpoint {
        pub(crate) fn succeeding() -> Arc<Self> {
            Self::succeeding_on(ExecutionHost::Local)
        }

        /// The same endpoint, owned by `host`.
        pub(crate) fn succeeding_on(host: ExecutionHost) -> Arc<Self> {
            Arc::new(Self {
                host,
                fail: std::sync::atomic::AtomicBool::new(false),
                failure_kind: StdMutex::new(None),
                effective_enabled: std::sync::atomic::AtomicBool::new(false),
                effective_queries: std::sync::atomic::AtomicUsize::new(0),
                submissions: std::sync::atomic::AtomicUsize::new(0),
                local_ipc: StdMutex::new(None),
                revision: StdMutex::new(Self::initial_revision()),
                source_hash: StdMutex::new("source".to_owned()),
                operations: StdMutex::new(std::collections::HashMap::new()),
                status_override: StdMutex::new((None, None)),
                recover_should_fail: std::sync::atomic::AtomicBool::new(false),
                result_missing: std::sync::atomic::AtomicBool::new(false),
                checks: StdMutex::new(Vec::new()),
                check_answer: StdMutex::new(TestCheckAnswer::Pass),
                reconciled: StdMutex::new(Vec::new()),
                rolls_back: std::sync::atomic::AtomicBool::new(false),
                status_fails: std::sync::atomic::AtomicBool::new(false),
                status_reads: std::sync::atomic::AtomicUsize::new(0),
            })
        }

        /// Boots `client` the way setup does: StartupReconcile proves the
        /// owner and applies the committed configuration. A host that has not
        /// been told otherwise boots with its core stopped, as a fresh one
        /// does.
        pub(crate) async fn prime(&self, client: &NyanpasuClient) {
            let failed = self.fail.swap(false, std::sync::atomic::Ordering::SeqCst);
            {
                let mut status = self.status_override.lock().unwrap();
                if status.0.is_none() {
                    status.0 =
                        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None });
                }
            }
            let report = client.startup_reconcile().await;
            assert_eq!(
                report.outcome,
                application_workflow::startup::StartupOutcome::Ready,
                "fixture boot: {report:?}"
            );
            self.fail.store(failed, std::sync::atomic::Ordering::SeqCst);
            self.submissions
                .store(0, std::sync::atomic::Ordering::SeqCst);
            self.checks.lock().unwrap().clear();
            self.reconciled.lock().unwrap().clear();
        }

        pub(crate) fn failing() -> Arc<Self> {
            Arc::new(Self {
                host: ExecutionHost::Local,
                fail: std::sync::atomic::AtomicBool::new(true),
                failure_kind: StdMutex::new(None),
                effective_enabled: std::sync::atomic::AtomicBool::new(false),
                effective_queries: std::sync::atomic::AtomicUsize::new(0),
                submissions: std::sync::atomic::AtomicUsize::new(0),
                local_ipc: StdMutex::new(None),
                revision: StdMutex::new(Self::initial_revision()),
                source_hash: StdMutex::new("source".to_owned()),
                operations: StdMutex::new(std::collections::HashMap::new()),
                status_override: StdMutex::new((None, None)),
                recover_should_fail: std::sync::atomic::AtomicBool::new(false),
                result_missing: std::sync::atomic::AtomicBool::new(false),
                checks: StdMutex::new(Vec::new()),
                check_answer: StdMutex::new(TestCheckAnswer::Pass),
                reconciled: StdMutex::new(Vec::new()),
                rolls_back: std::sync::atomic::AtomicBool::new(false),
                status_fails: std::sync::atomic::AtomicBool::new(false),
                status_reads: std::sync::atomic::AtomicUsize::new(0),
            })
        }

        fn initial_revision() -> nyanpasu_ipc::api::status::RevisionIdInfo {
            nyanpasu_ipc::api::status::RevisionIdInfo {
                epoch: 1,
                generation: 1,
                effective_hash: "effective".into(),
            }
        }

        pub(crate) fn set_failure(&self, kind: Option<&'static str>) {
            self.fail
                .store(kind.is_some(), std::sync::atomic::Ordering::SeqCst);
            *self.failure_kind.lock().unwrap() = kind;
        }
        pub(crate) fn submissions(&self) -> usize {
            self.submissions.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// Scripts `status()` as unreadable from now on.
        pub(crate) fn status_reads(&self) -> usize {
            self.status_reads.load(std::sync::atomic::Ordering::SeqCst)
        }

        pub(crate) fn set_status_fails(&self, fails: bool) {
            self.status_fails
                .store(fails, std::sync::atomic::Ordering::SeqCst);
        }

        /// Scripts the next reconciles as rolled back.
        pub(crate) fn set_rolls_back(&self, rolls_back: bool) {
            self.rolls_back
                .store(rolls_back, std::sync::atomic::Ordering::SeqCst);
        }

        /// Scripts whether the host serves effective-config snapshots at all.
        /// Without one the apply is recorded but never inspected, so a test
        /// that needs `state.applied` to advance has to turn this on.
        pub(crate) fn set_effective_enabled(&self, enabled: bool) {
            self.effective_enabled
                .store(enabled, std::sync::atomic::Ordering::SeqCst);
        }

        /// Moves the effective revision the endpoint reports as applied. On its
        /// own this is what a restart of the *same* configuration looks like:
        /// the managed controller endpoint carries the epoch, so a new epoch
        /// always brings a new effective hash.
        pub(crate) fn set_effective_hash(&self, hash: &str) {
            self.revision.lock().unwrap().effective_hash = hash.into();
        }

        /// Moves the source identity the endpoint reports as applied, standing
        /// in for a core that is running a different document than the app
        /// expects.
        pub(crate) fn set_source_hash(&self, hash: &str) {
            *self.source_hash.lock().unwrap() = hash.to_owned();
        }

        pub(crate) fn set_check_answer(&self, answer: TestCheckAnswer) {
            *self.check_answer.lock().unwrap() = answer;
        }

        /// The documents this endpoint was asked to check, in order.
        pub(crate) fn checked(&self) -> Vec<crate::core::actor_v2::endpoint::CheckSubmission> {
            self.checks.lock().unwrap().clone()
        }

        /// The config bytes of every reconcile this endpoint was asked to run.
        pub(crate) fn reconciled_bytes(&self) -> Vec<Vec<u8>> {
            self.reconciled.lock().unwrap().clone()
        }

        /// Scripts what `status()` reports next (R5 stop-decision tests).
        pub(crate) fn set_status(
            &self,
            state: Option<nyanpasu_ipc::api::status::CoreStateDetail>,
            applied_kind: Option<nyanpasu_core_manager::CoreKind>,
        ) {
            *self.status_override.lock().unwrap() = (state, applied_kind);
        }

        /// Scripts whether the next `Recover` submission fails (R6a tests).
        pub(crate) fn set_result_missing(&self, missing: bool) {
            self.result_missing
                .store(missing, std::sync::atomic::Ordering::SeqCst);
        }

        pub(crate) fn set_recover_should_fail(&self, should_fail: bool) {
            self.recover_should_fail
                .store(should_fail, std::sync::atomic::Ordering::SeqCst);
        }

        /// Computes and records the terminal result for `submission`.
        /// `Reconcile` enforces CAS the way the real runtime's
        /// `apply_config` does: `expected_applied: None` skips the check, a
        /// `Some(r)` that disagrees with the tracked revision fails with
        /// `revision_conflict`, and an accepted reconcile advances it.
        fn operation(
            &self,
            submission: &crate::core::actor_v2::endpoint::CoreSubmission,
        ) -> nyanpasu_ipc::api::core::v2::OperationInfo {
            let id = submission.envelope.operation_id.to_string();
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                return nyanpasu_ipc::api::core::v2::OperationInfo {
                    id,
                    phase: nyanpasu_ipc::api::core::v2::OperationPhase::Failed,
                    output: None,
                    error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                        kind: self.failure_kind.lock().unwrap().map(Into::into),
                        message: "reconcile boom".into(),
                        retryable: false,
                    }),
                };
            }
            // F4 regression coverage needs a real `Stopped` answer: the
            // updater's replacement step stops the core before copying the
            // binary, and the facade rejects any other output for `Stop`.
            if matches!(
                submission.envelope.command,
                nyanpasu_core_manager::CoreCommand::Stop
            ) {
                // A host that stopped its core says so when it is next read,
                // which is the proof a retired or stopped owner is held to.
                self.status_override.lock().unwrap().0 =
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None });
                return nyanpasu_ipc::api::core::v2::OperationInfo {
                    id,
                    phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
                    output: Some(nyanpasu_ipc::api::core::v2::OperationOutputInfo::Stopped),
                    error: None,
                };
            }
            // R6a: `ApplicationWorkflowClient::replace_binary` now submits
            // `Recover` unconditionally as the death proof, and the facade
            // rejects any other output for it. Scriptable to fail so a test
            // can prove the replacement aborts before its installer runs when
            // recovery cannot prove the core dead.
            if matches!(
                submission.envelope.command,
                nyanpasu_core_manager::CoreCommand::Recover
            ) {
                if self
                    .recover_should_fail
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    return nyanpasu_ipc::api::core::v2::OperationInfo {
                        id,
                        phase: nyanpasu_ipc::api::core::v2::OperationPhase::Failed,
                        output: None,
                        error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                            kind: None,
                            message: "recover boom".into(),
                            retryable: false,
                        }),
                    };
                }
                return nyanpasu_ipc::api::core::v2::OperationInfo {
                    id,
                    phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
                    output: Some(nyanpasu_ipc::api::core::v2::OperationOutputInfo::Recovered),
                    error: None,
                };
            }
            let nyanpasu_core_manager::CoreCommand::Reconcile(request) =
                &submission.envelope.command
            else {
                return successful_reconcile(submission.envelope.operation_id);
            };
            *self.local_ipc.lock().unwrap() = request.options.local_ipc;
            let nyanpasu_core_manager::ConfigInput::Inline { bytes, .. } = &request.config;
            self.reconciled.lock().unwrap().push(bytes.clone());
            let mut current = self.revision.lock().unwrap();
            if let Some(expected) = &request.expected_applied {
                let stale = expected.epoch.get() != current.epoch
                    || expected.generation != current.generation
                    || expected.effective_hash != current.effective_hash;
                if stale {
                    return nyanpasu_ipc::api::core::v2::OperationInfo {
                        id,
                        phase: nyanpasu_ipc::api::core::v2::OperationPhase::Failed,
                        output: None,
                        error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                            kind: Some("revision_conflict".into()),
                            message: format!(
                                "revision conflict: expected epoch={} generation={} \
                                 effective_hash={}, actual epoch={} generation={} \
                                 effective_hash={}",
                                expected.epoch.get(),
                                expected.generation,
                                expected.effective_hash,
                                current.epoch,
                                current.generation,
                                current.effective_hash,
                            ),
                            retryable: true,
                        }),
                    };
                }
            }
            let rolled_back = self.rolls_back.load(std::sync::atomic::Ordering::SeqCst);
            if !rolled_back {
                current.generation += 1;
                *self.source_hash.lock().unwrap() = nyanpasu_core_manager::payload_digest(bytes);
                *self.status_override.lock().unwrap() = (
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Running {
                        epoch: current.epoch,
                        pid: 7,
                    }),
                    Some(request.core.kind),
                );
            }
            let applied = current.clone();
            nyanpasu_ipc::api::core::v2::OperationInfo {
                id,
                phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
                output: Some(
                    nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(
                        nyanpasu_ipc::api::core::v2::ReconcileOutcomeInfo {
                            outcome: if rolled_back {
                                nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::RolledBack
                            } else {
                                nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Started
                            },
                            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                                epoch: applied.epoch,
                                generation: applied.generation,
                                source_hash: self.source_hash.lock().unwrap().clone(),
                                effective_hash: applied.effective_hash,
                            },
                            warning: None,
                            failed_apply: rolled_back
                                .then(|| "scripted: the target would not start".to_owned()),
                        },
                    ),
                ),
                error: None,
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::core::actor_v2::endpoint::ControlEndpoint for TestControlEndpoint {
        fn host(&self) -> ExecutionHost {
            self.host
        }

        async fn check_config(
            &self,
            submission: crate::core::actor_v2::endpoint::CheckSubmission,
        ) -> crate::core::actor_v2::endpoint::CheckSupport {
            use crate::core::actor_v2::endpoint::CheckSupport;
            let answer = self.check_answer.lock().unwrap().clone();
            self.checks.lock().unwrap().push(submission);
            match answer {
                TestCheckAnswer::Pass => CheckSupport::Ran(Ok(())),
                TestCheckAnswer::Reject(error) => CheckSupport::Ran(Err(error)),
                TestCheckAnswer::Unsupported => CheckSupport::Unsupported {
                    reason: "scripted: this host exposes no config check".into(),
                },
                TestCheckAnswer::Hang => std::future::pending().await,
            }
        }

        async fn submit(
            &self,
            submission: crate::core::actor_v2::endpoint::CoreSubmission,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::v2::OperationInfo,
            nyanpasu_core_manager::CoreError,
        > {
            self.submissions
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let info = self.operation(&submission);
            self.operations
                .lock()
                .unwrap()
                .insert(info.id.clone(), info.clone());
            Ok(info)
        }

        async fn wait_operation(
            &self,
            id: nyanpasu_core_manager::OperationId,
            _timeout: std::time::Duration,
        ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
            if self
                .result_missing
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return None;
            }
            self.operations
                .lock()
                .unwrap()
                .get(&id.to_string())
                .cloned()
        }

        async fn effective_config(
            &self,
        ) -> std::result::Result<
            Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>,
            nyanpasu_core_manager::CoreError,
        > {
            use std::sync::atomic::Ordering;
            if !self.effective_enabled.load(Ordering::SeqCst) {
                return Ok(None);
            }
            if self.effective_queries.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(nyanpasu_core_manager::CoreError::new(
                    nyanpasu_core_manager::CoreErrorKind::BackendUnavailable,
                    "snapshot temporarily unavailable",
                    true,
                ));
            }
            let current = self.revision.lock().unwrap().clone();
            Ok(Some(nyanpasu_ipc::api::core::v2::CoreEffectiveConfig {
                instance_id: "test-instance".into(),
                revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                    epoch: current.epoch,
                    generation: current.generation,
                    source_hash: self.source_hash.lock().unwrap().clone(),
                    effective_hash: current.effective_hash,
                },
                config: "mode: rule\nexternal-controller-unix: /tmp/recovered.sock\n".into(),
            }))
        }

        async fn status(
            &self,
        ) -> std::result::Result<
            crate::core::actor_v2::endpoint::CoreStatusSnapshot,
            nyanpasu_core_manager::CoreError,
        > {
            self.status_reads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.status_fails.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(nyanpasu_core_manager::CoreError::new(
                    nyanpasu_core_manager::CoreErrorKind::BackendUnavailable,
                    "scripted: the host stopped answering status reads",
                    true,
                ));
            }
            let (state, applied_kind) = self.status_override.lock().unwrap().clone();
            Ok(crate::core::actor_v2::endpoint::CoreStatusSnapshot {
                controller: None,
                state,
                state_changed_at: 0,
                revision: Some(self.revision.lock().unwrap().clone()),
                source_hash: Some(self.source_hash.lock().unwrap().clone()),
                healthy: Some(true),
                applied_kind,
            })
        }
    }

    pub(crate) struct HostTransitionEndpoint {
        host: ExecutionHost,
        calls: Arc<StdMutex<Vec<&'static str>>>,
        delegate: Arc<TestControlEndpoint>,
    }
    impl HostTransitionEndpoint {
        pub(crate) fn new(
            host: ExecutionHost,
            calls: Arc<StdMutex<Vec<&'static str>>>,
        ) -> Arc<Self> {
            Arc::new(Self {
                host,
                calls,
                delegate: TestControlEndpoint::succeeding_on(host),
            })
        }

        /// A freshly launched host with no core running, which is what
        /// startup finds.
        pub(crate) fn stopped(
            host: ExecutionHost,
            calls: Arc<StdMutex<Vec<&'static str>>>,
        ) -> Arc<Self> {
            let endpoint = Self::new(host, calls);
            endpoint.delegate.set_status(
                Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                None,
            );
            endpoint
        }
    }
    #[async_trait::async_trait]
    impl crate::core::actor_v2::endpoint::ControlEndpoint for HostTransitionEndpoint {
        fn host(&self) -> ExecutionHost {
            self.host
        }
        async fn submit(
            &self,
            submission: crate::core::actor_v2::endpoint::CoreSubmission,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::v2::OperationInfo,
            nyanpasu_core_manager::CoreError,
        > {
            match &submission.envelope.command {
                nyanpasu_core_manager::CoreCommand::Stop if self.host == ExecutionHost::Service => {
                    self.calls.lock().unwrap().push("handoff_to_local")
                }
                nyanpasu_core_manager::CoreCommand::Reconcile(_) => self
                    .calls
                    .lock()
                    .unwrap()
                    .push(if self.host == ExecutionHost::Service {
                        "reconcile_service"
                    } else {
                        "reconcile_local"
                    }),
                _ => {}
            }
            self.delegate.submit(submission).await
        }
        async fn wait_operation(
            &self,
            id: nyanpasu_core_manager::OperationId,
            timeout: std::time::Duration,
        ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
            self.delegate.wait_operation(id, timeout).await
        }
        async fn status(
            &self,
        ) -> std::result::Result<
            crate::core::actor_v2::endpoint::CoreStatusSnapshot,
            nyanpasu_core_manager::CoreError,
        > {
            self.delegate.status().await
        }
    }

    pub(crate) struct HostTransitionServiceAdapter {
        pub endpoint: crate::core::actor_v2::endpoint::EndpointHandle,
        pub calls: Arc<StdMutex<Vec<&'static str>>>,
        pub stopped: std::sync::atomic::AtomicBool,
        /// The config dir the daemon reports it was installed with.
        pub installed_for: PathBuf,
    }

    #[async_trait::async_trait]
    impl crate::core::actor_v2::service_actor::ServiceHostAdapter for HostTransitionServiceAdapter {
        async fn probe(
            &self,
        ) -> std::result::Result<
            nyanpasu_ipc::types::StatusInfo<'static>,
            crate::core::service::control::ServiceCommandError,
        > {
            if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(nyanpasu_ipc::types::StatusInfo {
                    name: "test-service".into(),
                    version: "2.0.0".into(),
                    status: nyanpasu_ipc::types::ServiceStatus::Stopped,
                    server: None,
                });
            }
            self.calls.lock().unwrap().push("ensure_ready");
            Ok(nyanpasu_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("test-service"),
                version: std::borrow::Cow::Borrowed("2.0.0"),
                status: nyanpasu_ipc::types::ServiceStatus::Running,
                server: Some(nyanpasu_ipc::api::status::StatusResBody {
                    log_query_version: None,
                    version: std::borrow::Cow::Borrowed("2.0.0"),
                    core_infos: nyanpasu_ipc::api::status::CoreInfos {
                        instance_id: None,
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
                        nyanpasu_config_dir: std::borrow::Cow::Owned(self.installed_for.clone()),
                        nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                    },
                    logs: None,
                }),
            })
        }

        async fn install(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.calls.lock().unwrap().push("install");
            Ok(())
        }

        async fn uninstall(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.calls.lock().unwrap().push("uninstall");
            Ok(())
        }

        async fn start_daemon(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.stopped
                .store(false, std::sync::atomic::Ordering::SeqCst);
            self.calls.lock().unwrap().push("start_daemon");
            Ok(())
        }

        async fn stop_daemon(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.stopped
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.calls.lock().unwrap().push("stop_daemon");
            Ok(())
        }

        async fn update(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            Ok(())
        }

        fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
            self.endpoint.clone()
        }
    }

    pub(crate) fn successful_reconcile(
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

    pub(crate) struct IdleServiceAdapter;

    #[async_trait::async_trait]
    impl crate::core::actor_v2::service_actor::ServiceHostAdapter for IdleServiceAdapter {
        async fn probe(
            &self,
        ) -> std::result::Result<
            nyanpasu_ipc::types::StatusInfo<'static>,
            crate::core::service::control::ServiceCommandError,
        > {
            Ok(nyanpasu_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("test-service"),
                version: std::borrow::Cow::Borrowed("test"),
                status: nyanpasu_ipc::types::ServiceStatus::NotInstalled,
                server: None,
            })
        }

        async fn install(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            Ok(())
        }

        async fn uninstall(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            Ok(())
        }

        async fn start_daemon(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            Ok(())
        }

        async fn stop_daemon(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            Ok(())
        }

        async fn update(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            Ok(())
        }

        fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
            Arc::new(IdleEndpoint)
        }
    }

    pub(crate) fn test_v2_clients() -> (CoreClientV2, ServiceClient) {
        test_v2_clients_with_endpoint(test_idle_endpoint())
    }

    /// A core that answers nothing, for tests whose subject is the facade
    /// rather than the running core.
    pub(crate) fn test_idle_endpoint() -> crate::core::actor_v2::endpoint::EndpointHandle {
        Arc::new(IdleEndpoint)
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

    /// A clash config for tests that resolve ports. The default binds a fixed
    /// mixed port, which parallel tests would contend for; port 0 never does.
    pub(crate) fn test_clash_config() -> ClashConfig {
        ClashConfig {
            mixed_port: PortStrategy::new_allow_fallback(0),
            ..ClashConfig::default()
        }
    }

    /// Seeds `path` with [`test_clash_config`], which the clash config client
    /// then loads instead of creating the default.
    fn test_backup_deps(dir: &TempDir) -> (PathResolver, Storage) {
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        std::fs::create_dir_all(paths.app_data_dir()).unwrap();
        let storage = Storage::try_new(&paths.storage_path()).unwrap();
        (paths, storage)
    }

    fn seed_test_clash_config(path: impl AsRef<std::path::Path>) {
        std::fs::write(path, serde_yaml::to_string(&test_clash_config()).unwrap()).unwrap();
    }

    pub(crate) async fn test_typed_config_clients(
        dir: &TempDir,
    ) -> (ApplicationClient, SessionStateClient, ClashConfigClient) {
        let application = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            crate::bundle::Channel::Stable,
            temp_config_path(dir, "application.yaml"),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("application client should be created");
        let session_state = SessionStateClient::new(
            temp_config_path(dir, "session-state.yaml"),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("session state client should be created");
        seed_test_clash_config(dir.path().join("clash-config.yaml"));
        let clash_config = ClashConfigClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_config_path(dir, "clash-config.yaml"),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("clash config client should be created");

        (application, session_state, clash_config)
    }

    pub(crate) fn test_materialization_port() -> Arc<dyn ProfileMaterializationPort> {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
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

    #[test]
    fn logs_facade_reads_app_files_and_degrades_service_independently() {
        use crate::client::logs::LogSource;
        use nyanpasu_logging::{Direction, Filter, LogError, OpenLogs, QueryLogs};
        let dir = tempdir().unwrap();
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        std::fs::create_dir_all(paths.app_logs_dir()).unwrap();
        std::fs::write(
            paths
                .app_logs_dir()
                .join("clash-nyanpasu.2026-09-11.app.log"),
            b"{\"level\":\"INFO\",\"fields\":{\"message\":\"app own log\"}}\n",
        )
        .unwrap();
        let client = tauri::async_runtime::block_on(test_client(&dir));
        tauri::async_runtime::block_on(async {
            assert_eq!(
                client.list_log_files(LogSource::App).await.unwrap().len(),
                1
            );
            assert_eq!(
                client.list_log_files(LogSource::Service).await.unwrap_err(),
                LogError::Unsupported
            );
            let session = client
                .open_log_session(
                    LogSource::App,
                    "window".into(),
                    OpenLogs {
                        request_id: "first".into(),
                        file: None,
                    },
                )
                .await
                .unwrap();
            let page = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                loop {
                    let page = client
                        .query_logs(
                            LogSource::App,
                            "window".into(),
                            QueryLogs {
                                session: session.id.clone(),
                                filter: Filter::default(),
                                direction: Direction::Latest,
                                cursor: None,
                                limit: 200,
                            },
                        )
                        .await
                        .unwrap();
                    if !page.building {
                        break page;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(page.rows[0].message, "app own log");
            client
                .close_log_session(LogSource::App, "window".into(), session.id)
                .await
                .unwrap();
        });
    }

    async fn test_client_with_system_dns(
        dir: &TempDir,
        system_dns: Arc<dyn SystemDnsCache>,
    ) -> NyanpasuClient {
        test_client_with_ports(dir, system_dns, Arc::new(MockOsProxyPort::new())).await
    }

    async fn test_client_with_ports(
        dir: &TempDir,
        system_dns: Arc<dyn SystemDnsCache>,
        os_proxy: Arc<dyn OsProxyPort>,
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        test_client_from_typed_clients(
            dir,
            application,
            session_state,
            clash_config,
            system_dns,
            Arc::new(direct_egress::MockDirectEgressProbe::new()),
            Arc::new(crate::core::geo::NoopCountryIndexSource),
            os_proxy,
        )
        .await
    }

    async fn test_client_from_typed_clients(
        dir: &TempDir,
        application: ApplicationClient,
        session_state: SessionStateClient,
        clash_config: ClashConfigClient,
        system_dns: Arc<dyn SystemDnsCache>,
        direct_egress: Arc<dyn DirectEgressProbe>,
        geo_index: Arc<dyn crate::core::geo::CountryIndexSource>,
        os_proxy: Arc<dyn OsProxyPort>,
    ) -> NyanpasuClient {
        let profiles = profiles::ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_config_path(dir, "profiles.yaml"),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("profiles client should be created");
        let ports = Arc::new(SessionPortResolver::default());
        let (core_v2, service) = test_v2_clients();
        let (backup_paths, storage) = test_backup_deps(&dir);
        NyanpasuClient::with_parts(
            None,
            crate::bundle::BundleMetadata {
                is_portable: false,
                is_fixed_webview: false,
                release_channel: crate::bundle::Channel::Stable,
            },
            logs::test_setup(
                PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"))
                    .app_logs_dir(),
            ),
            profiles.jobs(),
            application,
            session_state,
            clash_config,
            profiles,
            Arc::new(MockProfileFsPort::new()),
            ports,
            dir.path().join("profiles"),
            PathBuf::new(),
            backup_paths,
            storage,
            RuntimePaths::from_resolver(&PathResolver::with_base_dirs(
                dir.path().into(),
                dir.path().join("data"),
            ))
            .unwrap(),
            crate::enhance::ScriptDirs::under(dir.path()),
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            None,
            None,
            core_v2,
            service,
            system_dns,
            direct_egress,
            geo_index,
            os_proxy,
            Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
            Arc::new(effects::ports::NoopApplicationEffects),
            Arc::new(hotkey::ports::MockWindowControl::new()),
            Arc::new(hotkey::adapters::PlatformAcceleratorValidator),
            None,
            Arc::new(|| anyhow::bail!("HTTP routes are unavailable")),
            None,
            tokio_util::sync::CancellationToken::new(),
            tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap()
    }

    /// The application workflow applies committed state; it is not a second
    /// commit point for a configuration domain (design D1/§3.1). Holding a
    /// domain client, the facade that owns all three, or an actor's raw message
    /// enum is what would make it one, so the production sources under the
    /// workflow must not name any of them. Its `tests/` directories are skipped:
    /// a fixture legitimately builds the real clients to seed a graph.
    #[test]
    fn application_workflow_sources_hold_no_source_config_client() {
        // A domain client is the direct way to write a source domain. The facade
        // reaches all three, and an actor's own message enum reaches one without
        // its typed client, so naming any of them is a way to commit.
        const FORBIDDEN: [&str; 10] = [
            "ApplicationClient",
            "ClashConfigClient",
            "ProfilesClient",
            "NyanpasuClient",
            "ApplicationActorMessage",
            "ClashConfigActorMessage",
            "ProfilesActorMessage",
            "SessionStateActorMessage",
            "CoreActorMessage",
            "ServiceActorMessage",
        ];

        fn collect(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("workflow sources should be readable") {
                let path = entry.expect("directory entry should be readable").path();
                if path.is_dir() {
                    if path.file_name() != Some(std::ffi::OsStr::new("tests")) {
                        collect(&path, files);
                    }
                } else if path.extension() == Some(std::ffi::OsStr::new("rs")) {
                    files.push(path);
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/client");
        let mut files = Vec::new();
        collect(&root.join("application_workflow"), &mut files);
        collect(&root.join("core_lifecycle"), &mut files);
        assert!(!files.is_empty(), "the scan found no workflow sources");

        let mut offenders = Vec::new();
        for file in files {
            let source = std::fs::read_to_string(&file).expect("workflow source should be UTF-8");
            for (number, line) in source.lines().enumerate() {
                for name in FORBIDDEN {
                    if line.contains(name) {
                        offenders.push(format!(
                            "{}:{}: {}",
                            file.display(),
                            number + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "the application workflow must read source config through StateSnapshot handles, \
             reach the actors through their typed clients, and let the facade commit:\n{}",
            offenders.join("\n")
        );
    }

    /// Required subscriber that holds the transaction open inside `on_prepare`
    /// until the test releases it, so the owning actor's mailbox is provably
    /// blocked while the assertion runs.
    struct ParkedPrepare {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl nyanpasu_core::state::StateAckSubscriber<NyanpasuAppConfig> for ParkedPrepare {
        fn name(&self) -> nyanpasu_core::state::SubscriberName<'_> {
            "test-parked-prepare".into()
        }

        async fn on_prepare(
            &self,
            _change: nyanpasu_core::state::StateChange<NyanpasuAppConfig>,
        ) -> nyanpasu_core::state::Ack {
            self.entered.notify_one();
            self.release.notified().await;
            nyanpasu_core::state::Ack::Ok
        }
    }

    #[tokio::test]
    async fn committed_config_reads_do_not_wait_for_a_parked_required_prepare() {
        use futures_util::FutureExt;

        let dir = tempdir().expect("tempdir should be created");
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mut manager =
            nyanpasu_core::state::PersistentStateManagerSetup::<NyanpasuAppConfig>::builder()
                .config_path(temp_config_path(&dir, "application.yaml"))
                .assemble()
                .from_state(NyanpasuAppConfig::default())
                .await
                .expect("application manager should initialize");
        manager.add_subscriber(Box::new(ParkedPrepare {
            entered: entered.clone(),
            release: release.clone(),
        }));
        let application = ApplicationClient::from_manager(
            crate::state::mutation::MutationCoordinator::isolated(),
            manager,
            crate::bundle::Channel::Stable,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("application client should be created");
        let session_state = SessionStateClient::new(
            temp_config_path(&dir, "session-state.yaml"),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("session state client should be created");
        seed_test_clash_config(dir.path().join("clash-config.yaml"));
        let clash_config = ClashConfigClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_config_path(&dir, "clash-config.yaml"),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("clash config client should be created");
        let client = test_client_from_typed_clients(
            &dir,
            application,
            session_state,
            clash_config,
            Arc::new(NoopSystemDnsCache),
            Arc::new(direct_egress::MockDirectEgressProbe::new()),
            Arc::new(crate::core::geo::NoopCountryIndexSource),
            Arc::new(MockOsProxyPort::new()),
        )
        .await;

        assert!(!client.get_app_config().await.unwrap().enable_system_proxy);

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let patching = {
            let client = client.clone();
            tokio::spawn(async move { client.patch_app_config(patch).await })
        };

        // The transaction now owns the actor and is parked mid-prepare.
        entered.notified().await;
        let parked = client
            .get_app_config()
            .now_or_never()
            .expect("a committed read must not await the parked actor")
            .expect("committed read should succeed");
        assert!(
            !parked.enable_system_proxy,
            "an unfinished transaction must not be visible as committed state"
        );

        release.notify_one();
        patching
            .await
            .expect("patch task should join")
            .expect("patch should commit");
        assert!(client.get_app_config().await.unwrap().enable_system_proxy);
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
    async fn get_os_proxy_reads_through_the_injected_port() {
        let dir = tempdir().expect("tempdir should be created");
        let mut os_proxy = MockOsProxyPort::new();
        os_proxy.expect_get().times(1).returning(|| {
            Ok(OsProxyConfig {
                enable: true,
                host: "127.0.0.1".into(),
                port: 7890,
                bypass: "localhost".into(),
            })
        });
        let client =
            test_client_with_ports(&dir, Arc::new(NoopSystemDnsCache), Arc::new(os_proxy)).await;

        let read = client.get_os_proxy().await.expect("the port answers");
        assert_eq!((read.enable, read.port), (true, 7890));
    }

    #[tokio::test]
    async fn get_os_proxy_reports_the_port_failure_as_its_own_error() {
        let dir = tempdir().expect("tempdir should be created");
        let mut os_proxy = MockOsProxyPort::new();
        os_proxy.expect_get().times(1).returning(|| {
            Err(OsProxyError::ReadOsProxy {
                source: "denied".into(),
            })
        });
        let client =
            test_client_with_ports(&dir, Arc::new(NoopSystemDnsCache), Arc::new(os_proxy)).await;

        let error = client.get_os_proxy().await.unwrap_err();
        assert!(matches!(error, OsProxyError::ReadOsProxy { .. }));
    }

    #[tokio::test]
    async fn flush_system_dns_cache_propagates_adapter_failure() {
        let dir = tempdir().expect("tempdir should be created");
        let mut system_dns = MockSystemDnsCache::new();
        system_dns.expect_flush().times(1).returning(|| {
            Err(SystemDnsError::FlushRejected {
                command: "ipconfig.exe",
                code: Some(5),
            })
        });
        let client = test_client_with_system_dns(&dir, Arc::new(system_dns)).await;

        let error = client.flush_system_dns_cache().await.unwrap_err();
        assert!(matches!(
            error,
            SystemDnsError::FlushRejected {
                command: "ipconfig.exe",
                code: Some(5)
            }
        ));
    }

    async fn test_client_with_direct_egress(
        dir: &TempDir,
        direct_egress: Arc<dyn DirectEgressProbe>,
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        test_client_from_typed_clients(
            dir,
            application,
            session_state,
            clash_config,
            Arc::new(NoopSystemDnsCache),
            direct_egress,
            Arc::new(crate::core::geo::NoopCountryIndexSource),
            Arc::new(MockOsProxyPort::new()),
        )
        .await
    }

    #[tokio::test]
    async fn probe_direct_egress_reports_each_family_from_the_injected_probe() {
        let dir = tempdir().expect("tempdir should be created");
        let address = std::net::Ipv4Addr::new(203, 0, 113, 7);
        let mut probe = direct_egress::MockDirectEgressProbe::new();
        probe
            .expect_ipv4()
            .times(1)
            .returning(move || Some(address));
        probe.expect_ipv6().times(1).returning(|| None);
        let client = test_client_with_direct_egress(&dir, Arc::new(probe)).await;
        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(true),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();

        assert_eq!(
            client.probe_direct_egress().await.unwrap(),
            DirectEgress::Probed {
                ipv4: Some(address),
                ipv6: None,
            }
        );
    }

    #[tokio::test]
    async fn probe_direct_egress_does_not_probe_while_tun_mode_is_enabled() {
        let dir = tempdir().expect("tempdir should be created");
        let mut probe = direct_egress::MockDirectEgressProbe::new();
        probe.expect_ipv4().never();
        probe.expect_ipv6().never();
        let client = test_client_with_direct_egress(&dir, Arc::new(probe)).await;
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        client
            .patch_clash_config(patch)
            .await
            .expect("clash patch should succeed");

        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(true),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();

        assert_eq!(
            client.probe_direct_egress().await.unwrap(),
            DirectEgress::TunEnabled
        );
    }

    #[tokio::test]
    async fn direct_egress_only_probes_on_request_and_failed_answers_clear_the_cache() {
        use crate::core::traffic::LocalSourceLocation;
        let dir = tempdir().unwrap();
        let mut probe = direct_egress::MockDirectEgressProbe::new();
        let mut sequence = mockall::Sequence::new();
        probe
            .expect_ipv4()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Some("203.0.113.7".parse().unwrap()));
        probe
            .expect_ipv4()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| None);
        probe.expect_ipv6().times(2).returning(|| None);
        let client = test_client_with_direct_egress(&dir, Arc::new(probe)).await;
        assert_eq!(
            client.probe_direct_egress().await.unwrap(),
            DirectEgress::Disabled
        );
        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(true),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();
        assert_eq!(client.inner.local_source.addresses().ipv4, None);
        client.probe_direct_egress().await.unwrap();
        assert_eq!(
            client.inner.local_source.addresses().ipv4,
            Some("203.0.113.7".parse().unwrap())
        );

        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(false),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();
        assert_eq!(client.inner.local_source.addresses().ipv4, None);
        assert_eq!(
            client.probe_direct_egress().await.unwrap(),
            DirectEgress::Disabled
        );
        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(true),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();
        client.probe_direct_egress().await.unwrap();
        assert_eq!(client.inner.local_source.addresses().ipv4, None);
    }

    #[tokio::test]
    async fn direct_egress_discards_an_answer_when_tun_is_enabled_during_the_request() {
        use crate::core::traffic::LocalSourceLocation;
        struct WaitingProbe {
            started: tokio::sync::Notify,
            release: tokio::sync::Notify,
        }
        #[async_trait::async_trait]
        impl DirectEgressProbe for WaitingProbe {
            async fn ipv4(&self) -> Option<std::net::Ipv4Addr> {
                self.started.notify_one();
                self.release.notified().await;
                Some("203.0.113.7".parse().unwrap())
            }
            async fn ipv6(&self) -> Option<std::net::Ipv6Addr> {
                None
            }
        }
        let dir = tempdir().unwrap();
        let probe = Arc::new(WaitingProbe {
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let client = test_client_with_direct_egress(&dir, probe.clone()).await;
        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(true),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();
        let request = tokio::spawn({
            let client = client.clone();
            async move { client.probe_direct_egress().await }
        });
        probe.started.notified().await;
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        client.patch_clash_config(patch).await.unwrap();
        probe.release.notify_one();
        assert_eq!(request.await.unwrap().unwrap(), DirectEgress::TunEnabled);
        assert_eq!(client.inner.local_source.addresses().ipv4, None);
    }

    pub(crate) fn test_client_args_with_endpoint(
        dir: &TempDir,
        endpoint: crate::core::actor_v2::endpoint::EndpointHandle,
    ) -> ClientSetupArgs {
        let (paths, storage) = test_backup_deps(dir);
        seed_test_clash_config(paths.clash_config_path());
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let (shutdown, tasks) = (
            tokio_util::sync::CancellationToken::new(),
            tokio_util::task::TaskTracker::new(),
        );
        let (core_v2, service) = test_v2_clients_with_endpoint(endpoint);
        ClientSetupArgs {
            jobs: std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        tauri::async_runtime::block_on(jobs::test_client_with_owner(
                            shutdown.clone(),
                            &tasks,
                        ))
                    })
                    .join()
                    .unwrap()
            }),
            bundle_metadata: crate::bundle::BundleMetadata {
                is_portable: false,
                is_fixed_webview: false,
                release_channel: crate::bundle::Channel::Stable,
            },
            logging: logs::test_setup(paths.app_logs_dir()),
            http_frontend: None,
            http_routes: Arc::new(|| anyhow::bail!("HTTP routes are unavailable")),
            paths,
            storage,
            runtime_paths,
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            app_update_backend_factory: None,
            app_update_event_sink: None,
            core_v2,
            service,
            system_dns: Arc::new(NoopSystemDnsCache),
            direct_egress: Arc::new(direct_egress::MockDirectEgressProbe::new()),
            geo_index: Arc::new(crate::core::geo::NoopCountryIndexSource),
            os_proxy: Arc::new(MockOsProxyPort::new()),
            binary_installer: Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
            effects: Arc::new(effects::ports::NoopApplicationEffects),
            // A mock rather than a no-op: a test that dispatches a window
            // action without saying so should fail, not pass silently.
            window: Arc::new(hotkey::ports::MockWindowControl::new()),
            // The real rule: a test that writes a hotkey the platform cannot
            // parse should fail here, exactly as the app would.
            accelerators: Arc::new(hotkey::adapters::PlatformAcceleratorValidator),
            traffic_store: None,
            shutdown,
            tasks,
        }
    }

    fn host_transition_client(
        dir: &TempDir,
        calls: Arc<StdMutex<Vec<&'static str>>>,
    ) -> NyanpasuClient {
        host_transition_client_seeded(dir, calls, false)
    }

    /// Records "persisted" before every readiness probe that finds service
    /// mode already saved, so a test can tell a probe taken before the commit
    /// from one taken after it.
    struct PersistedSwitchProbe {
        delegate: HostTransitionServiceAdapter,
        application_config: PathBuf,
    }

    #[async_trait::async_trait]
    impl crate::core::actor_v2::service_actor::ServiceHostAdapter for PersistedSwitchProbe {
        async fn probe(
            &self,
        ) -> std::result::Result<
            nyanpasu_ipc::types::StatusInfo<'static>,
            crate::core::service::control::ServiceCommandError,
        > {
            let persisted = std::fs::read(&self.application_config)
                .ok()
                .and_then(|bytes| serde_yaml::from_slice::<serde_yaml::Value>(&bytes).ok())
                .and_then(|config| config.get("enable_service_mode")?.as_bool())
                .unwrap_or(false);
            if persisted {
                self.delegate.calls.lock().unwrap().push("persisted");
            }
            self.delegate.probe().await
        }

        async fn install(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.delegate.install().await
        }

        async fn uninstall(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.delegate.uninstall().await
        }

        async fn start_daemon(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.delegate.start_daemon().await
        }

        async fn stop_daemon(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.delegate.stop_daemon().await
        }

        async fn update(
            &self,
        ) -> std::result::Result<(), crate::core::service::control::ServiceCommandError> {
            self.delegate.update().await
        }

        fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
            self.delegate.endpoint()
        }
    }

    fn host_transition_client_seeded(
        dir: &TempDir,
        calls: Arc<StdMutex<Vec<&'static str>>>,
        service_seed: bool,
    ) -> NyanpasuClient {
        let mut args = test_client_args_with_endpoint(dir, Arc::new(IdleEndpoint));
        let local = HostTransitionEndpoint::stopped(ExecutionHost::Local, calls.clone());
        let service_endpoint =
            HostTransitionEndpoint::stopped(ExecutionHost::Service, calls.clone());
        let adapter = Arc::new(PersistedSwitchProbe {
            delegate: HostTransitionServiceAdapter {
                endpoint: service_endpoint,
                calls: calls.clone(),
                stopped: std::sync::atomic::AtomicBool::new(false),
                installed_for: args.paths.app_config_dir().to_path_buf(),
            },
            application_config: args.paths.application_config_path(),
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
        args.core_v2 = core_v2;
        args.service = service;
        if service_seed {
            let seed = NyanpasuAppConfig {
                enable_service_mode: true,
                ..Default::default()
            };
            std::fs::write(
                args.paths.application_config_path(),
                serde_yaml::to_string(&seed).unwrap(),
            )
            .unwrap();
        }
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        if !service_seed {
            let report = tauri::async_runtime::block_on(client.startup_reconcile());
            assert_eq!(
                report.outcome,
                application_workflow::startup::StartupOutcome::Ready,
                "{report:?}"
            );
        }
        calls.lock().unwrap().clear();
        client
    }

    /// Switches the execution host the way the settings page does: an
    /// `enable_service_mode` patch through the application transaction.
    pub(crate) async fn set_service_mode(
        client: &NyanpasuClient,
        enabled: bool,
    ) -> Result<runtime::MutationOutcome<()>> {
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_service_mode = Some(enabled);
        client.patch_app_config(patch).await
    }

    #[test]
    fn enabling_service_mode_ensures_runtime_before_commit() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let client = host_transition_client(&dir, calls.clone());

        tauri::async_runtime::block_on(async {
            let outcome = set_service_mode(&client, true).await.unwrap();
            assert!(matches!(
                outcome,
                runtime::MutationOutcome::Committed { .. }
            ));
            assert!(client.get_app_config().await.unwrap().enable_service_mode);
        });

        let calls = calls.lock().unwrap();
        let ensure = calls
            .iter()
            .position(|call| *call == "ensure_ready")
            .unwrap();
        assert!(
            calls
                .iter()
                .position(|call| *call == "persisted")
                .is_none_or(|persisted| ensure < persisted),
            "service mode was saved before the daemon was ready: {calls:?}"
        );
    }

    #[test]
    fn disabling_service_mode_hands_off_before_stopping_the_daemon() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let client = host_transition_client(&dir, calls.clone());

        tauri::async_runtime::block_on(async {
            set_service_mode(&client, true).await.unwrap();
            calls.lock().unwrap().clear();
            let outcome = set_service_mode(&client, false).await.unwrap();
            assert!(matches!(
                outcome,
                runtime::MutationOutcome::Committed { .. }
            ));
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
            set_service_mode(&client, true).await.unwrap();
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
            set_service_mode(&client, true).await.unwrap();
            calls.lock().unwrap().clear();
            set_service_mode(&client, false).await.unwrap();
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
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let ports = Arc::new(SessionPortResolver::default());
        let file_service = Arc::new(ProfileFileService::new(
            paths.clone(),
            ports.clone() as Arc<dyn SelfProxyPortSource>,
        ));
        let profiles = profiles::ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_config_path(dir, "profiles.yaml"),
            file_service.clone() as Arc<dyn ProfileFsPort>,
            fetcher,
            file_service.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("profiles client should be created");
        let (core_v2, service) = test_v2_clients();
        let (backup_paths, storage) = test_backup_deps(&dir);
        let client = NyanpasuClient::with_parts(
            None,
            crate::bundle::BundleMetadata {
                is_portable: false,
                is_fixed_webview: false,
                release_channel: crate::bundle::Channel::Stable,
            },
            logs::test_setup(
                PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"))
                    .app_logs_dir(),
            ),
            profiles.jobs(),
            application,
            session_state,
            clash_config,
            profiles,
            file_service.clone() as Arc<dyn ProfileFsPort>,
            ports,
            paths.app_profiles_dir(),
            PathBuf::new(),
            backup_paths,
            storage,
            RuntimePaths::from_resolver(&paths).unwrap(),
            crate::enhance::ScriptDirs::from_resolver(&paths),
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            None,
            None,
            core_v2,
            service,
            Arc::new(NoopSystemDnsCache),
            Arc::new(direct_egress::MockDirectEgressProbe::new()),
            Arc::new(crate::core::geo::NoopCountryIndexSource),
            Arc::new(MockOsProxyPort::new()),
            Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
            Arc::new(effects::ports::NoopApplicationEffects),
            Arc::new(hotkey::ports::MockWindowControl::new()),
            Arc::new(hotkey::adapters::PlatformAcceleratorValidator),
            None,
            Arc::new(|| anyhow::bail!("HTTP routes are unavailable")),
            None,
            tokio_util::sync::CancellationToken::new(),
            tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
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
    fn manual_backups_keep_the_newest_three_and_leave_migration_backups() {
        let dir = tempdir().expect("tempdir should be created");
        let backups = dir.path().join("data").join("backups");
        let migration = backups.join("migration-20250101T000000Z-1.0.0-to-2.0.0");
        std::fs::create_dir_all(&migration).unwrap();

        let names = tauri::async_runtime::block_on(async {
            let client = test_client(&dir).await;
            let mut names = Vec::new();
            for _ in 0..4 {
                names.push(client.create_config_backup().await.unwrap().name);
            }
            assert_eq!(client.backups_dir().unwrap(), backups);
            names
        });

        let mut kept: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("manual-"))
            .collect();
        kept.sort();
        let mut expected = names[1..].to_vec();
        expected.sort();
        assert_eq!(kept, expected);
        assert!(migration.exists());
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

            let window_state = WindowState {
                width: 1024,
                height: 768,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            };
            client
                .save_main_window_geometry(window_state.clone())
                .await
                .expect("session save should succeed");
            assert_eq!(client.main_window_geometry(), Some(window_state));

            let mut clash_patch = ClashConfig::new_empty_patch();
            clash_patch.enable_tun_mode = Some(true);
            client
                .patch_clash_config(clash_patch)
                .await
                .expect("clash patch should succeed");
            assert!(client.get_clash_config().await.unwrap().enable_tun_mode);
        });
    }

    #[test]
    fn typed_setup_loads_persisted_state() {
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

            let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
            let (loaded, _session_state, _clash_config) = new_typed_config_clients(
                crate::state::mutation::MutationCoordinator::isolated(),
                crate::bundle::Channel::Stable,
                paths,
                &tokio_util::sync::CancellationToken::new(),
                &tokio_util::task::TaskTracker::new(),
            )
            .await
            .expect("typed clients should load persisted state");

            assert_eq!(loaded.snapshot().state.theme_color.to_string(), "#123456");
        });
    }

    #[test]
    fn try_new_with_args_constructs_typed_config_facade() {
        let dir = tempdir().expect("tempdir should be created");
        let (paths, storage) = test_backup_deps(&dir);
        let runtime_paths = RuntimePaths::from_resolver(&paths).unwrap();
        let (shutdown, tasks) = (
            tokio_util::sync::CancellationToken::new(),
            tokio_util::task::TaskTracker::new(),
        );
        let (core_v2, service) = test_v2_clients();
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
            jobs: std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        tauri::async_runtime::block_on(jobs::test_client_with_owner(
                            shutdown.clone(),
                            &tasks,
                        ))
                    })
                    .join()
                    .unwrap()
            }),
            bundle_metadata: crate::bundle::BundleMetadata {
                is_portable: true,
                is_fixed_webview: false,
                release_channel: crate::bundle::Channel::Stable,
            },
            logging: logs::test_setup(paths.app_logs_dir()),
            http_frontend: None,
            http_routes: Arc::new(|| anyhow::bail!("HTTP routes are unavailable")),
            paths,
            storage,
            runtime_paths,
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            app_update_backend_factory: None,
            app_update_event_sink: None,
            core_v2,
            service,
            system_dns: Arc::new(NoopSystemDnsCache),
            direct_egress: Arc::new(direct_egress::MockDirectEgressProbe::new()),
            geo_index: Arc::new(crate::core::geo::NoopCountryIndexSource),
            os_proxy: Arc::new(MockOsProxyPort::new()),
            binary_installer: Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
            effects: Arc::new(effects::ports::NoopApplicationEffects),
            // A mock rather than a no-op: a test that dispatches a window
            // action without saying so should fail, not pass silently.
            window: Arc::new(hotkey::ports::MockWindowControl::new()),
            // The real rule: a test that writes a hotkey the platform cannot
            // parse should fail here, exactly as the app would.
            accelerators: Arc::new(hotkey::adapters::PlatformAcceleratorValidator),
            traffic_store: None,
            shutdown,
            tasks,
        })
        .expect("client should construct with typed config actors");

        assert!(client.is_portable());

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

    /// The runtime paths every test client is built with.
    pub(crate) fn test_runtime_paths(dir: &TempDir) -> RuntimePaths {
        RuntimePaths::from_resolver(&PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        ))
        .unwrap()
    }

    #[test]
    fn runtime_lifecycle_is_empty_before_first_rebuild() {
        let dir = tempdir().unwrap();
        let client = tauri::async_runtime::block_on(test_client(&dir));
        let promoted = tauri::async_runtime::block_on(client.promoted_runtime());
        let lifecycle = client.inner.application_workflow.runtime();

        assert!(promoted.is_none());
        assert!(lifecycle.promoted.is_none());
    }

    #[tokio::test]
    async fn reconcile_publishes_the_runtime_product_read_model() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        client.reconcile_core().await.unwrap();
        assert!(client.promoted_runtime().await.is_some());
        assert!(test_runtime_paths(&dir).product().exists());
    }

    #[tokio::test]
    async fn runtime_inspection_tracks_promoted_builds() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        assert!(client.inspect_runtime().await.is_none());
        assert!(matches!(
            client.runtime_yaml().await,
            Err(RuntimeError::NoRuntimeConfig)
        ));
        assert!(client.runtime_config().await.unwrap().is_none());
        assert!(matches!(
            client.inspect_runtime_node("missing", 0).await,
            Err(RuntimeError::RuntimeSnapshotChanged)
        ));
        client.reconcile_core().await.unwrap();
        let first = client.inspect_runtime().await.unwrap();
        assert!(!first.nodes.is_empty());
        let final_node = first
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.tag,
                    nyanpasu_config::runtime::snapshot::OperatorTag::BuiltinStep {
                        step: nyanpasu_config::runtime::snapshot::BuiltinStepKind::Finalizing,
                        ..
                    }
                )
            })
            .unwrap();
        let content = client
            .inspect_runtime_node(&first.snapshot_id, final_node.id)
            .await
            .unwrap();
        let config: serde_yaml::Mapping = serde_yaml::from_str(&content.yaml).unwrap();
        let mut expected = client.promoted_runtime().await.unwrap().config.clone();
        assert_ne!(
            expected.get("secret").and_then(serde_yaml::Value::as_str),
            Some("<redacted>")
        );
        expected.insert("secret".into(), "<redacted>".into());
        assert_eq!(config, expected);
        client.reconcile_core().await.unwrap();
        let second = client.inspect_runtime().await.unwrap();
        assert_ne!(first.snapshot_id, second.snapshot_id);
        assert!(matches!(
            client
                .inspect_runtime_node(&first.snapshot_id, first.root_id)
                .await,
            Err(RuntimeError::RuntimeSnapshotChanged)
        ));
        assert!(matches!(
            client
                .inspect_runtime_node(&second.snapshot_id, u32::MAX)
                .await,
            Err(RuntimeError::RuntimeNodeNotFound { node_id: u32::MAX })
        ));
        assert!(
            client
                .inspect_runtime_node(&second.snapshot_id, second.root_id)
                .await
                .is_ok()
        );
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

    #[test]
    fn repeated_core_updates_advance_the_runtime_revision() {
        use nyanpasu_config::application::ClashCore;
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            endpoint.prime(&client).await;
            client.update_core(ClashCore::ClashRs).await.unwrap();
            let first = client.promoted_runtime().await.unwrap();
            client.update_core(ClashCore::Mihomo).await.unwrap();
            let second = client.promoted_runtime().await.unwrap();
            assert_eq!(first.target_core, ClashCore::ClashRs);
            assert_eq!(second.target_core, ClashCore::Mihomo);
            assert!(second.revision > first.revision);
        });
        assert_eq!(endpoint.submissions(), 2);
    }

    /// Each composition root builds its own graph: what one client commits
    /// and submits never reaches another.
    #[test]
    fn two_client_graphs_are_independent() {
        use nyanpasu_config::application::ClashCore;
        let (dir_a, dir_b) = (tempdir().unwrap(), tempdir().unwrap());
        let (endpoint_a, endpoint_b) = (
            TestControlEndpoint::succeeding(),
            TestControlEndpoint::succeeding(),
        );
        let client_a = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir_a,
            endpoint_a.clone(),
        ))
        .unwrap();
        let client_b = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir_b,
            endpoint_b.clone(),
        ))
        .unwrap();
        let default_core = NyanpasuAppConfig::default().core;
        tauri::async_runtime::block_on(async {
            endpoint_a.prime(&client_a).await;
            endpoint_b.prime(&client_b).await;
            client_b.update_core(ClashCore::ClashRs).await.unwrap();
            assert_eq!(client_a.get_app_config().await.unwrap().core, default_core);
            assert_eq!(
                client_a.promoted_runtime().await.unwrap().target_core,
                default_core
            );
            assert_eq!(
                client_b.get_app_config().await.unwrap().core,
                ClashCore::ClashRs
            );
        });
        assert_eq!(endpoint_a.submissions(), 0);
        assert_eq!(endpoint_b.submissions(), 1);
    }

    #[tokio::test]
    async fn reconcile_product_matches_the_published_snapshot() {
        let dir = tempdir().unwrap();
        let client = test_client(&dir).await;
        client.reconcile_core().await.unwrap();
        let snapshot = client.promoted_runtime().await.unwrap();
        assert_eq!(
            tokio::fs::read(test_runtime_paths(&dir).product())
                .await
                .unwrap(),
            snapshot.product_bytes()
        );
    }

    #[test]
    fn facade_add_activate_rebuilds_via_control_endpoint() {
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
            client.reconcile_core().await.expect("reconcile");
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

            let lifecycle = client.inner.application_workflow.runtime();
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
    fn activation_requires_runtime_success_before_committing_selection() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::failing();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            endpoint.prime(&client).await;
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .unwrap()
                .into_value();
            let version = client.get_profiles().await.unwrap().revision();
            assert!(client.activate_profile(Some(uid)).await.is_err());
            let after = client.get_profiles().await.unwrap();
            assert!(after.current.is_none());
            assert_eq!(after.revision(), version);
        });
    }

    /// S5 (the adopting half; `startup.rs` covers a residual core): the core
    /// actor always spawns on Local, so startup hands the runtime to a
    /// persisted Service host, and applies the configuration there.
    #[test]
    fn startup_adopts_a_ready_service_host_and_starts_the_core_there() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let client = host_transition_client_seeded(&dir, calls.clone(), true);

        tauri::async_runtime::block_on(async {
            let report = client.startup_reconcile().await;

            assert_eq!(
                report.outcome,
                application_workflow::startup::StartupOutcome::Ready,
                "{report:?}"
            );
            assert_eq!(client.core_status().host, ExecutionHost::Service);
        });

        let calls = calls.lock().unwrap();
        assert!(
            !calls
                .iter()
                .any(|call| *call == "install" || *call == "start_daemon"),
            "adopting a ready daemon must not converge it: {calls:?}"
        );
        assert!(
            calls.contains(&"reconcile_service") && !calls.contains(&"reconcile_local"),
            "{calls:?}"
        );
    }

    /// S8 (#5443, supersedes leader ruling R8): ...and only by adopting a
    /// daemon that is already up. Converging one would install and start the
    /// service, raising a UAC prompt at every launch for a user who merely
    /// left the setting on. Service mode is a preference, so the core runs
    /// locally instead.
    #[test]
    fn startup_never_converges_an_absent_daemon_and_runs_the_core_locally() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            None,
        );
        let args = test_client_args_with_endpoint(&dir, endpoint.clone());
        let seed = NyanpasuAppConfig {
            enable_service_mode: true,
            ..Default::default()
        };
        std::fs::write(
            args.paths.application_config_path(),
            serde_yaml::to_string(&seed).unwrap(),
        )
        .unwrap();
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            let report = client.startup_reconcile().await;

            assert_eq!(
                report.outcome,
                application_workflow::startup::StartupOutcome::Ready,
                "{report:?}"
            );
            assert_eq!(
                client.core_status().host,
                ExecutionHost::Local,
                "an absent daemon must not be installed and started by startup"
            );
            assert_eq!(endpoint.submissions(), 1, "the core starts locally instead");
            assert_eq!(
                client.configuration_status().runtime.health,
                crate::client::convergence::ConvergenceHealth::Healthy
            );
        });
    }

    #[test]
    fn inspection_recovers_a_successful_apply_without_reconciling_again() {
        use std::sync::atomic::Ordering;
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            endpoint.prime(&client).await;
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .unwrap()
                .into_value();
            client.activate_profile(Some(uid)).await.unwrap();
            endpoint.effective_enabled.store(true, Ordering::SeqCst);
            client.reconcile_core().await.unwrap();
            let state = client.inner.application_workflow.runtime();
            assert!(state.pending.is_some());
            assert!(state.promoted.as_ref().unwrap().effective.is_none());
            let submitted = endpoint.submissions();
            let inspection = client.inspect_runtime().await.unwrap();
            assert!(inspection.applied);
            assert!(!inspection.effective_pending);
            assert!(inspection.nodes.iter().any(|node| matches!(
                node.tag,
                nyanpasu_config::runtime::snapshot::OperatorTag::BuiltinStep {
                    step: nyanpasu_config::runtime::snapshot::BuiltinStepKind::CoreController,
                    ..
                }
            )));
            assert_eq!(endpoint.submissions(), submitted);
            assert!(
                client
                    .inner
                    .application_workflow
                    .runtime()
                    .pending
                    .is_none()
            );
            assert!(client.inspect_applied_runtime().await.unwrap().applied);
        });
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
            endpoint.prime(&client).await;
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            client.activate_profile(Some(uid)).await.expect("activate");
            client.reconcile_core().await.expect("reconcile");
            assert!(endpoint.submissions() >= 1);
        });
    }

    /// F2 regression: `reconcile_core`'s CAS token must come from an
    /// admission-time read of the endpoint, not the router's projection,
    /// which the pump refreshes only every 2s. Two reconciles run back to
    /// back with no sleep in between, so that pump cannot have ticked; the
    /// fake enforces CAS the way the real runtime does, so a reconcile that
    /// reads the stale cached revision submits a token one generation behind
    /// and gets rejected.
    #[test]
    fn two_reconciles_in_one_pump_interval_both_get_a_fresh_cas_token() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            // Wait for the pump's very first read so the router's cached
            // projection is seeded with the endpoint's revision before
            // either reconcile below runs. Without this, both reconciles
            // would submit `expected_applied: None` (no cached revision
            // yet), which never conflicts, and the test could not tell a
            // stale cached read from a fresh one.
            //
            // The pump publishes the refreshed projection as an event
            // rather than on any fixed schedule this test can bound, so
            // wait on that notification instead of a fixed number of
            // yields: a bounded busy-wait can spuriously fail a correct
            // implementation if the pump simply has not run yet. Subscribe
            // before the first check -- a broadcast receiver only observes
            // events sent after it subscribes, so checking first would risk
            // missing the one update that seeds the projection.
            let mut status_events = client.subscribe_core_events();
            while client.core_status().snapshot.is_none() {
                match status_events.recv().await {
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            assert!(
                client.core_status().snapshot.is_some(),
                "the pump never seeded the router's cached projection; without a baseline \
                 revision both reconciles below submit expected_applied: None, which never \
                 conflicts, and this test would pass without exercising the CAS check at all"
            );
            endpoint.prime(&client).await;

            client
                .reconcile_core()
                .await
                .expect("the first reconcile establishes a new revision");
            // No sleep here: this relies on the second reconcile starting
            // within one pump interval of the first, not on proof that the
            // pump cannot have run -- the two calls run back to back with
            // no await point that yields to the pump's 2s timer in between.
            client.reconcile_core().await.expect(
                "the second reconcile must re-read the endpoint's current revision rather than \
                 the router's pump-refreshed cache; using the cache submits the revision from \
                 before the first reconcile, and the fake endpoint rejects it exactly as the \
                 real runtime would",
            );
        });
        assert_eq!(endpoint.submissions(), 2);
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

    /// D5+P0-1 invariant: a failed reconcile must leave the promoted product
    /// visible while the control endpoint reports the failure.
    #[test]
    fn failed_reconcile_keeps_the_committed_product_visible() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::failing();
        let args = test_client_args_with_endpoint(&dir, endpoint.clone());
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            endpoint.prime(&client).await;
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add")
                .into_value();
            // A rejected activation keeps the last accepted product.
            assert!(client.activate_profile(Some(uid)).await.is_err());
            let lifecycle = client.inner.application_workflow.runtime();
            assert!(client.promoted_runtime().await.is_some());
            assert_eq!(lifecycle.promoted.unwrap().revision.get(), 1);
        });
    }

    #[test]
    fn failed_reconcile_still_advances_the_promoted_read_model() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::failing();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();

        tauri::async_runtime::block_on(async {
            endpoint.prime(&client).await;
            let first = client.reconcile_core().await.expect_err("reconcile fails");
            assert!(first.to_string().contains("reconcile boom"));
            let before = client.promoted_runtime().await.unwrap();
            let _ = client.reconcile_core().await.expect_err("reconcile fails");
            let after = client.promoted_runtime().await.unwrap();
            assert!(after.revision > before.revision);
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
                .import_profile(url, None, Some(patch), None)
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

    /// R6/R7: a source receipt reaches the configuration status and its
    /// change stream, the status sequence only grows, and deleting the
    /// profile drops its row.
    #[test]
    fn configuration_status_projects_source_receipts_and_prunes_them() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(move |_, _| {
            if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2 {
                Ok(crate::state::profiles::ports::FetchedSubscription {
                    content: "proxies: []\n".into(),
                    subscription: SubscriptionInfo::default(),
                    filename: None,
                    suggested_update_interval_minutes: None,
                })
            } else {
                Err(SubscriptionFetchError::mock())
            }
        });
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;
            let (_, _, sources) = client.subscribe_configuration_changes();
            // The first import becomes current; the second stays deletable.
            client
                .import_profile(
                    url::Url::parse("https://example.com/subs/current.yaml").unwrap(),
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();
            let uid = client
                .import_profile(
                    url::Url::parse("https://example.com/subs/other.yaml").unwrap(),
                    None,
                    None,
                    None,
                )
                .await
                .unwrap()
                .into_value();
            let before = client.configuration_status();
            assert!(before.sources.is_empty());

            assert!(client.refresh_profile(uid.clone(), None).await.is_err());
            assert!(sources.has_changed().unwrap());
            let failed = client.configuration_status();
            assert!(failed.event_seq > before.event_seq);
            // No source moved, so only the receipt advanced the sequence.
            let (was, now) = (&before.source_versions, &failed.source_versions);
            assert_eq!(
                (was.application, was.clash, was.session, was.profiles),
                (now.application, now.clash, now.session, now.profiles)
            );
            assert_eq!(failed.sources.len(), 1);
            assert_eq!(failed.sources[0].profile, uid);
            assert_eq!(
                failed.sources[0].health,
                convergence::ConvergenceHealth::Blocked
            );

            client.delete_profile(uid).await.unwrap();
            let pruned = client.configuration_status();
            assert!(pruned.sources.is_empty());
            assert!(pruned.event_seq > failed.event_seq);
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
                .import_profile(url, None, Some(patch), None)
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
            assert!(
                client
                    .import_profile(url, None, Some(patch), None)
                    .await
                    .is_err()
            );
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
            .returning(|_, _| Err(SubscriptionFetchError::mock()));
        // A failed import never reaches core apply, so the bridge expects nothing.
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher)).await;
            let url = url::Url::parse("https://example.com/subs/x.yaml").unwrap();
            let result = client.import_profile(url, None, None, None).await;
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
            assert!(
                matches!(
                    rejected,
                    Err(ClientError::Profiles(
                        ProfilesError::RemoteProfileNeedsImport
                    ))
                ),
                "remote profiles must be rejected before any write, got {rejected:?}"
            );
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
                matches!(
                    rejected,
                    Err(ClientError::Profiles(
                        ProfilesError::RemoteProfileNeedsImport
                    ))
                ),
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
                crate::state::mutation::MutationCoordinator::isolated(),
                temp_config_path(&dir, "profiles.yaml"),
                Arc::new(MockProfileFsPort::new()),
                Arc::new(MockSubscriptionFetcher::new()),
                Arc::new(materialization),
                tokio_util::sync::CancellationToken::new(),
                &tokio_util::task::TaskTracker::new(),
            )
            .await
            .expect("profiles client");
            let ports = Arc::new(SessionPortResolver::default());
            let (core_v2, service) = test_v2_clients();
            let (backup_paths, storage) = test_backup_deps(&dir);
            let client = NyanpasuClient::with_parts(
                None,
                crate::bundle::BundleMetadata {
                    is_portable: false,
                    is_fixed_webview: false,
                    release_channel: crate::bundle::Channel::Stable,
                },
                logs::test_setup(
                    PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"))
                        .app_logs_dir(),
                ),
                profiles.jobs(),
                application,
                session_state,
                clash_config,
                profiles,
                Arc::new(MockProfileFsPort::new()),
                ports,
                dir.path().join("profiles"),
                PathBuf::new(),
                backup_paths,
                storage,
                RuntimePaths::from_resolver(&PathResolver::with_base_dirs(
                    dir.path().into(),
                    dir.path().join("data"),
                ))
                .unwrap(),
                crate::enhance::ScriptDirs::under(dir.path()),
                Arc::new(crate::client::event_sink::NoopUiEventSink),
                None,
                None,
                core_v2,
                service,
                Arc::new(NoopSystemDnsCache),
                Arc::new(direct_egress::MockDirectEgressProbe::new()),
                Arc::new(crate::core::geo::NoopCountryIndexSource),
                Arc::new(MockOsProxyPort::new()),
                Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
                Arc::new(effects::ports::NoopApplicationEffects),
                Arc::new(hotkey::ports::MockWindowControl::new()),
                Arc::new(hotkey::adapters::PlatformAcceleratorValidator),
                None,
                Arc::new(|| anyhow::bail!("HTTP routes are unavailable")),
                None,
                tokio_util::sync::CancellationToken::new(),
                tokio_util::task::TaskTracker::new(),
            )
            .await
            .unwrap();

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
            assert!(
                outcome.degradations().iter().any(|item| matches!(
                    item.reason,
                    crate::client::runtime::DegradationReason::ProfileAutoActivationFailed { .. }
                )),
                "expected ProfileAutoActivationFailed, got {:?}",
                outcome.degradations()
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
                .import_profile(url, None, None, None)
                .await
                .expect("import");
            assert!(
                matches!(
                    outcome,
                    crate::client::runtime::MutationOutcome::Committed { .. }
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
                    crate::client::runtime::MutationOutcome::Committed { .. }
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
            ProfilesError::ProfilesReplyDropped,
            ProfilesError::VersionConflict {
                expected: 1,
                actual: 2,
                cleanup_failures: Vec::new(),
            },
            ProfilesError::ProfilesActorStopped,
        ] {
            let degradation =
                NyanpasuClient::auto_activation_failure_degradation(uid.clone(), error);
            assert!(matches!(
                degradation.reason,
                crate::client::runtime::DegradationReason::ProfileAutoActivationFailed { .. }
            ));
            assert_eq!(
                degradation.phase,
                crate::client::runtime::DegradationPhase::SystemEffect
            );
            assert!(degradation.retryable);
            assert!(!degradation.message.is_empty());

            // Protocol both create and import use after a successful durable commit.
            let prior = vec![crate::client::runtime::Degradation {
                phase: crate::client::runtime::DegradationPhase::ProfileMaterialization,
                reason: crate::client::runtime::DegradationReason::CleanupDeferred,
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
            assert!(
                matches!(
                    outcome.degradations(),
                    [
                        crate::client::runtime::Degradation {
                            reason: crate::client::runtime::DegradationReason::CleanupDeferred,
                            ..
                        },
                        crate::client::runtime::Degradation {
                            reason:
                                crate::client::runtime::DegradationReason::ProfileAutoActivationFailed { .. },
                            ..
                        },
                    ]
                ),
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
                .import_profile(url, None, None, None)
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
                .import_profile(url, Some("My VPN".into()), None, None)
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

    #[test]
    fn facade_import_transform_creates_remote_transform_without_activating_it() {
        let dir = tempdir().unwrap();
        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(ok_fetch_without_name())).await;
            let url = url::Url::parse("https://example.com/overlay.yaml").unwrap();
            let uid = client
                .import_profile(url, None, None, Some(TransformKind::Overlay))
                .await
                .expect("import")
                .into_value();
            let profiles = client.get_profiles().await.unwrap();
            let item = &profiles.items[&uid];
            assert!(matches!(
                &item.definition,
                ProfileDefinition::Transform { transform }
                    if transform.kind() == TransformKind::Overlay
                        && matches!(transform.source(), nyanpasu_config::profile::ProfileSource::Remote { .. })
            ));
            assert_eq!(profiles.current, None, "a transform is never activatable");
        });
    }

    #[test]
    fn managed_edit_uses_candidate_bytes_and_rejects_invalid_runtime_without_overwriting_source() {
        let dir = tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
            Some(nyanpasu_core_manager::CoreKind::Mihomo),
        );
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            client.reconcile_core().await.unwrap();
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .unwrap()
                .into_value();
            client.activate_profile(Some(uid.clone())).await.unwrap();
            let path = client
                .get_profile_materialized_path(uid.clone())
                .await
                .unwrap();
            let before = std::fs::read(&path).unwrap();
            let revision = client.get_profiles().await.unwrap().revision();
            endpoint.set_check_answer(TestCheckAnswer::Reject(
                nyanpasu_core_manager::CoreError::new(
                    nyanpasu_core_manager::CoreErrorKind::InvalidConfig,
                    "candidate rejected",
                    false,
                ),
            ));
            assert!(
                client
                    .save_profile_file(
                        uid.clone(),
                        "proxies: [{name: t6-fresh, type: direct}]\n".into()
                    )
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(client.get_profiles().await.unwrap().revision(), revision);
            endpoint.set_check_answer(TestCheckAnswer::Pass);
            client
                .save_profile_file(
                    uid.clone(),
                    "proxies: [{name: t6-fresh, type: direct}]\n".into(),
                )
                .await
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "proxies: [{name: t6-fresh, type: direct}]\n"
            );
            let runtime = client.promoted_runtime().await.unwrap();
            assert_eq!(
                runtime
                    .config
                    .get("proxies")
                    .and_then(serde_yaml::Value::as_sequence)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("name"))
                    .and_then(serde_yaml::Value::as_str),
                Some("t6-fresh")
            );
            client.request_shutdown();
            client.wait_shutdown().await;
        });
    }
}
