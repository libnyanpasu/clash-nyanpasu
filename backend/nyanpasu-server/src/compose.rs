//! Owns one application graph, assembled from explicit host inputs without presentation.

use std::{path::PathBuf, sync::Arc};

use anyhow::Context as _;
use nyanpasu_contracts::{bundle_metadata::BundleMetadata, elevation::ElevationConsent};
use nyanpasu_core::{
    actor_v2::{CoreClient, endpoint::LocalEndpoint, service_actor::ServiceClient},
    client::{
        ClientSetupArgs, NyanpasuClient, RuntimePaths,
        effects::{executor::ApplicationEffectExecutor, ports::LoggerRefresher},
        host_adapters::{ClientHostAdapters, ProfileFilePorts},
        system_proxy::{SystemProxyArgs, SystemProxyClient},
    },
    storage::{RedbHotkeyStoreOpener, Storage},
    traffic::TrafficFileStorage,
};
use nyanpasu_net::{
    device_headers::{self, DeviceHeaders},
    net_adapters::ReqwestNetworkAdapter,
    subscription_http::SubscriptionHttpFetcher,
};
use nyanpasu_os::{
    autostart::{AutoLaunchBackend, AutoLaunchConfig},
    core_install::FsBinaryInstaller,
    elevation::RefuseElevation,
    pac::HttpPacBackend,
    service::OsServiceProcessControl,
    system_dns::OsSystemDnsCache,
    system_proxy::SysproxyOsProxy,
};
use nyanpasu_paths::PathResolver;
use nyanpasu_profile_io::{content_source::FsProfileContentSource, fs_service::ProfileFileService};
use nyanpasu_rpc::{
    audit::RpcAudit,
    events::EventBus,
    registry::{RpcDependencies, UnifiedRpc},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[cfg(test)]
mod tests;

struct ElevationAdapters {
    service: Arc<OsServiceProcessControl>,
    dns: Arc<OsSystemDnsCache>,
    installer: Arc<FsBinaryInstaller>,
}

fn elevation_adapters(
    paths: &PathResolver,
    service_binary: PathBuf,
    #[cfg(test)] runner: Option<Arc<dyn nyanpasu_os::elevation::runner::CommandRunner>>,
) -> ElevationAdapters {
    let consent: Arc<dyn ElevationConsent> = Arc::new(RefuseElevation);
    #[cfg(test)]
    if let Some(runner) = runner {
        return ElevationAdapters {
            service: Arc::new(OsServiceProcessControl::new_with_runner(
                service_binary,
                paths.clone(),
                consent.clone(),
                runner.clone(),
            )),
            dns: Arc::new(OsSystemDnsCache::new_with_runner(
                consent.clone(),
                runner.clone(),
            )),
            installer: Arc::new(FsBinaryInstaller::new_with_runner(consent, runner)),
        };
    }
    ElevationAdapters {
        service: Arc::new(OsServiceProcessControl::new(
            service_binary,
            paths.clone(),
            consent.clone(),
        )),
        dns: Arc::new(OsSystemDnsCache::new(consent.clone())),
        installer: Arc::new(FsBinaryInstaller::new(consent)),
    }
}

/// Host facts supplied by the executable; metadata and executable paths are not guessed.
pub struct ServerSetupArgs {
    /// Explicit application roots and the host's install-directory resolution.
    pub paths: PathResolver,
    /// Real package/build identity collected by the shell.
    pub bundle_metadata: BundleMetadata,
    /// The server executable, used by updater, autostart and sidecar version probes.
    pub executable: PathBuf,
    /// The running logger's reload port, retained by shared configuration effects.
    pub logger: Arc<dyn LoggerRefresher>,
    /// Capture supplied by the shell's logging setup.
    pub jobs_capture: nyanpasu_jobs::LogCapture,
}

/// A composed application; listener activation belongs to the executable.
pub struct Server {
    client: NyanpasuClient,
    rpc: UnifiedRpc,
    storage: Storage,
    events: EventBus,
}

#[derive(Default)]
struct Owners {
    shutdown: CancellationToken,
    tasks: TaskTracker,
    core: Option<CoreClient>,
}

impl Owners {
    async fn shutdown(&self) {
        self.shutdown.cancel();
        self.tasks.close();
        self.tasks.wait().await;
        if let Some(core) = &self.core {
            core.finish_shutdown().await;
        }
    }
}

impl Server {
    #[cfg(test)]
    pub(crate) async fn from_test_args(mut args: ClientSetupArgs, storage: Storage) -> Self {
        let events = EventBus::new();
        args.ui_sink = Arc::new(nyanpasu_rpc::events::BusUiEventSink::new(events.clone()));
        let paths = args.paths.clone();
        let client = NyanpasuClient::try_new(args).await.unwrap();
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                icon_files: None,
                core_directory: None,
                service_prompt: None,
                accent_color: None,
                host_environment: nyanpasu_rpc::ports::HostEnvironment { is_appimage: false },
                client: client.clone(),
                storage: storage.clone(),
                paths,
                events: events.clone(),
                audit: RpcAudit::new(Arc::new(nyanpasu_rpc::audit::TracingRpcAuditSink)),
            },
            &[],
        )
        .unwrap();
        Self {
            client,
            rpc,
            storage,
            events,
        }
    }

    /// Constructs the owners and registry once, without activating transports or reconciliation.
    ///
    /// # Errors
    /// Returns a construction failure after joining owners already started.
    pub async fn try_new(args: ServerSetupArgs) -> anyhow::Result<Self> {
        let mut owners = Owners::default();
        match assemble(args, &mut owners).await {
            Ok(server) => Ok(server),
            Err(error) => {
                owners.shutdown().await;
                Err(error)
            }
        }
    }

    /// The single application facade shared by the shell's transports.
    pub fn client(&self) -> &NyanpasuClient {
        &self.client
    }

    /// The registry retains known unavailable commands as unsupported.
    pub fn rpc(&self) -> &UnifiedRpc {
        &self.rpc
    }

    /// Storage observations consumed by the shell's forwarding tasks.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Shared publication and transport subscriptions.
    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// Stops admission to application owners and waits for their admitted work and cleanup.
    pub async fn shutdown(self) {
        self.client.request_shutdown();
        self.client.wait_shutdown().await;
    }
}

async fn assemble(args: ServerSetupArgs, owners: &mut Owners) -> anyhow::Result<Server> {
    let ServerSetupArgs {
        paths,
        bundle_metadata,
        executable,
        logger,
        jobs_capture,
    } = args;
    let migration_paths = paths.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        migration_paths
            .create_base_dirs()
            .context("create application directories")?;
        let mut migrations = nyanpasu_migration::Runner::with_paths(
            migration_paths,
            bundle_metadata,
            false,
            Arc::new(RedbHotkeyStoreOpener),
        )
        .context("prepare migrations")?;
        migrations.run_pending().context("run migrations")
    })
    .await
    .map_err(|error| match error.try_into_panic() {
        Ok(payload) => std::panic::resume_unwind(payload),
        Err(error) => anyhow::Error::from(error),
    })??;
    let install_dir = executable.parent().context("locate executable parent")?;
    let service_binary = nyanpasu_os::service::service_binary(install_dir);
    let adapters = elevation_adapters(
        &paths,
        service_binary.clone(),
        #[cfg(test)]
        None,
    );
    let service_ipc = nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER)
        .context("build service IPC client")?;
    let control = nyanpasu_core::actor_v2::local_host::build(
        &paths,
        &service_binary,
        Arc::new(nyanpasu_utils::process::AtomicStopStrategy::new(
            Default::default(),
        )),
    )
    .await
    .context("build core host")?;
    let core = CoreClient::spawn(Arc::new(LocalEndpoint::new(control)))
        .await
        .context("spawn core actor")?;
    owners.core = Some(core.clone());
    let service = ServiceClient::spawn(
        adapters.service,
        Arc::new(
            nyanpasu_core::actor_v2::service_endpoint_factory::IpcServiceEndpointFactory::new(
                service_ipc.clone(),
            ),
        ),
        3,
    )
    .await
    .context("spawn service actor")?;
    service.drain_on_shutdown(&owners.tasks, owners.shutdown.child_token());
    let jobs = nyanpasu_core::client::jobs::start(
        paths.jobs_path().into_std_path_buf(),
        jobs_capture,
        owners.shutdown.child_token(),
        &owners.tasks,
    )
    .await
    .context("start jobs")?;
    let os_proxy = Arc::new(SysproxyOsProxy);
    let auto_launch = AutoLaunchBackend::new(AutoLaunchConfig::resolve(&executable, None)?)?;
    let pac = HttpPacBackend::new(paths.cache_dir().join("pac.js"))?;
    let system_proxy = SystemProxyClient::spawn(
        SystemProxyArgs {
            os: os_proxy.clone(),
            auto_launch: Arc::new(auto_launch),
            pac: Arc::new(pac),
            schedule_guard_ticks: true,
            shutdown: owners.shutdown.child_token(),
        },
        &owners.tasks,
    )
    .await
    .context("spawn system proxy owner")?;
    let effects = Arc::new(ApplicationEffectExecutor::new(system_proxy, logger, None));
    let events = EventBus::new();
    let runtime_paths = RuntimePaths::from_resolver(&paths);
    let storage_paths = paths.clone();
    let host_runtime_paths = runtime_paths.clone();
    let (storage, host_adapters, logging, traffic_store) = tokio::task::spawn_blocking(move || {
        let storage =
            Storage::try_new(storage_paths.storage_path().as_std_path()).context("open storage")?;
        let host = host_adapters(&storage_paths, &host_runtime_paths, bundle_metadata);
        let logging = logging(&storage_paths, service_ipc);
        let traffic = open_traffic_store(&storage_paths);
        Ok::<_, anyhow::Error>((storage, host, logging, traffic))
    })
    .await
    .map_err(|error| match error.try_into_panic() {
        Ok(payload) => std::panic::resume_unwind(payload),
        Err(error) => anyhow::Error::from(error),
    })??;
    let destination = install_dir.to_owned();
    let updater_paths = paths.clone();
    let client = NyanpasuClient::try_new(ClientSetupArgs {
        bundle_metadata,
        host_adapters,
        updater_backend_factory: Arc::new(move |ports| {
            Ok(Arc::new(nyanpasu_updater::HttpUpdaterBackend::new(
                destination.clone(),
                updater_paths.clone(),
                ports,
                bundle_metadata,
            )))
        }),
        jobs,
        logging,
        paths: paths.clone(),
        storage: Arc::new(storage.clone()),
        runtime_paths,
        ui_sink: Arc::new(nyanpasu_rpc::events::BusUiEventSink::new(events.clone())),
        app_update_backend_factory: None,
        app_update_event_sink: None,
        core_v2: core,
        service,
        system_dns: adapters.dns,
        direct_egress: Arc::new(nyanpasu_net::direct_egress::NetworkDirectEgressProbe::default()),
        geo_index: Arc::new(nyanpasu_core::geo::FsCountryIndexSource::new(
            paths.app_data_dir().as_std_path().to_owned(),
            paths.cache_dir().join("geodata").into_std_path_buf(),
        )),
        os_proxy,
        binary_installer: adapters.installer,
        core_versions: Arc::new(nyanpasu_os::core_version::SidecarCoreVersionReader::new(
            Arc::new(ExecutableSource(executable)),
        )),
        effects,
        window: None,
        accelerators: None,
        traffic_store,
        shutdown: owners.shutdown.clone(),
        tasks: owners.tasks.clone(),
    })
    .await
    .context("build application client")?;
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: nyanpasu_rpc::ports::HostEnvironment {
                is_appimage: bundle_metadata.is_appimage,
            },
            client: client.clone(),
            storage: storage.clone(),
            paths,
            events: events.clone(),
            audit: RpcAudit::new(Arc::new(nyanpasu_rpc::audit::TracingRpcAuditSink)),
        },
        &[nyanpasu_rpc_protocol::ShellCapability::OsIntegration],
    )
    .context("build RPC registry")?;
    Ok(Server {
        client,
        rpc,
        storage,
        events,
    })
}

struct ExecutableSource(PathBuf);

impl nyanpasu_contracts::core_version::CoreVersionExecutableSource for ExecutableSource {
    fn current_exe(
        &self,
        _: nyanpasu_config::application::ClashCore,
    ) -> Result<PathBuf, nyanpasu_contracts::core_version::CoreVersionError> {
        Ok(self.0.clone())
    }
}

fn host_adapters(
    paths: &PathResolver,
    runtime_paths: &RuntimePaths,
    metadata: BundleMetadata,
) -> ClientHostAdapters {
    let network = Arc::new(ReqwestNetworkAdapter::new(
        metadata,
        Arc::new(nyanpasu_os::system_proxy_source::OsSystemProxySource),
    ));
    let dirs = nyanpasu_scripts::ScriptDirs::new(
        paths.scripts_dir().into_std_path_buf(),
        paths.cache_dir().into_std_path_buf(),
    );
    let runtime = tokio::runtime::Handle::current();
    let profile_paths = paths.clone();
    let spec_paths = paths.clone();
    let device = nyanpasu_os::device_identity::get_device_info();
    let headers = DeviceHeaders {
        hwid: device.hwid,
        device_os: device_headers::sanitize_for_header(&device.device_os),
        os_version: device_headers::sanitize_for_header(&device.os_version),
        device_model: device_headers::sanitize_for_header(&device.device_model),
    };
    ClientHostAdapters {
        icon_fetcher: network.clone(),
        network_diagnostics: network,
        debug_http: None,
        changelog_http: Arc::new(move |ports| {
            Arc::new(nyanpasu_net::changelog_http::ReqwestChangelogHttp::new(
                ports,
                metadata,
                Arc::new(nyanpasu_os::system_proxy_source::OsSystemProxySource),
            ))
        }),
        runtime_build: Arc::new(
            nyanpasu_core::client::runtime_build::RuntimeBuildService::new(
                Arc::new(FsProfileContentSource::new(
                    paths.app_profiles_dir().into_std_path_buf(),
                )),
                Arc::new(move |core| {
                    nyanpasu_core::actor_v2::local_host::core_spec(core, &spec_paths)
                }),
                runtime_paths.clone(),
                Arc::new(move || {
                    let source = crate::script_extensions::HttpSource::new(
                        dirs.cache.clone(),
                        runtime.clone(),
                    )?;
                    Ok(Box::new(nyanpasu_scripts::EnhanceScriptRunner::new(
                        dirs.clone(),
                        Arc::new(crate::script_extensions::ServerScriptExtensions::new(
                            Arc::new(source),
                        )),
                    )?))
                }),
            ),
        ),
        profile_files: Arc::new(move |ports| {
            let files = Arc::new(ProfileFileService::new(profile_paths.clone()));
            ProfileFilePorts {
                fs: files.clone(),
                materialization: files,
                fetcher: Arc::new(SubscriptionHttpFetcher::new(
                    ports,
                    metadata,
                    Arc::new(nyanpasu_os::system_proxy_source::OsSystemProxySource),
                    headers.clone(),
                )),
            }
        }),
    }
}

fn logging(
    paths: &PathResolver,
    service: nyanpasu_ipc::client::Client,
) -> nyanpasu_core::client::logs::LoggingSetup {
    nyanpasu_core::client::logs::LoggingSetup {
        core: match nyanpasu_core::logs::RedbCoreLogStore::open(
            paths.app_logs_dir().join("core").into_std_path_buf(),
        ) {
            Ok(store) => Box::new(store),
            Err(error) => {
                tracing::warn!(%error, "core log storage unavailable");
                Box::new(nyanpasu_observers::logs::UnavailableCoreLogStore(format!(
                    "{error:#}"
                )))
            }
        },
        files: Arc::new(nyanpasu_logging::FsLogFiles::new(
            paths.app_logs_dir().into_std_path_buf(),
            "nyanpasu-server".into(),
        )),
        clock: Arc::new(nyanpasu_logging::MonotonicClock::default()),
        service: Arc::new(nyanpasu_core::client::logs::IpcServiceLogs::new(service)),
        frontend: Arc::new(nyanpasu_core::client::frontend_events::TracingFrontendLogSink),
    }
}

fn open_traffic_store(
    paths: &PathResolver,
) -> Option<Arc<dyn nyanpasu_observers::traffic::TrafficOwnerFactory>> {
    let dir = paths.app_data_dir().join("traffic").into_std_path_buf();
    let open = || -> anyhow::Result<TrafficFileStorage> {
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        nyanpasu_utils::io::atomic_fs::harden_windows_directory_acl(&dir)?;
        Ok(TrafficFileStorage::new(
            dir.join("traffic.tsdb"),
            nyanpasu_tsdb::Profile::desktop().backend_cache,
        ))
    };
    match open() {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::error!(%error, "traffic recording disabled");
            None
        }
    }
}
