//! The headless composition root.
//!
//! Builds the same `NyanpasuClient` graph the desktop shell builds in
//! `backend/tauri/src/setup.rs`, from explicit directories and without any GUI
//! runtime. Ports a headless host cannot provide get the honest adapters in
//! [`crate::adapters`].

use std::sync::Arc;

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::application::ReleaseChannel;
use nyanpasu_contracts::bundle_metadata::BundleMetadata;
use nyanpasu_core::{
    actor_v2::{
        CoreClient,
        endpoint::{EndpointHandle, LocalEndpoint},
        local_host::CoreLifetime,
        service_actor::ServiceClient,
    },
    client::{
        ClientSetupArgs, NyanpasuClient, OsSystemDnsCache, RuntimePaths, UiEventSink,
        host_adapters::{ClientHostAdapters, ProfileFilePorts},
    },
    storage::{RedbHotkeyStoreOpener, Storage},
};
use nyanpasu_net::{
    device_headers::{self, DeviceHeaders},
    direct_egress::HttpDirectEgressProbe,
    net_adapters::ReqwestNetworkAdapter,
    subscription_http::SubscriptionHttpFetcher,
};
use nyanpasu_paths::PathResolver;
use nyanpasu_profile_io::{content_source::FsProfileContentSource, fs_service::ProfileFileService};
use nyanpasu_rpc::{EventBus, RpcDependencies, UnifiedRpc};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::adapters::{
    BusUiEvents, CoreBinaryVersions, DisabledDebugHttp, FileCoreIntent, NoAccelerators, NoOsProxy,
    NoProxyEnvironment, NoServiceHost, NoWindows, UnsupportedEffects,
};

const RESTART_BUDGET: u8 = 3;
const APP_NAME: &str = "nyanpasu-server";

/// Where the server keeps its state. The path resolver finds the core binary in
/// the data directory, so it must be `<data_dir>/mihomo` for the default core.
#[derive(Debug, Clone)]
pub struct ServerDirs {
    pub config_dir: Utf8PathBuf,
    pub data_dir: Utf8PathBuf,
}

/// The owners the shutdown waits for, retained even when construction stops early.
struct Owners {
    shutdown: CancellationToken,
    tasks: TaskTracker,
    core: Option<CoreClient>,
}

impl Owners {
    async fn abort(&self) {
        self.shutdown.cancel();
        self.tasks.close();
        self.tasks.wait().await;
        if let Some(core) = &self.core {
            core.finish_shutdown().await;
        }
    }
}

/// The file the user's last run or stop request is kept in, under the config directory.
const CORE_INTENT_FILE: &str = "core-intent";

/// A running application graph, its facade and what the RPC layer needs of it.
pub struct Server {
    client: NyanpasuClient,
    storage: Storage,
    paths: PathResolver,
    events: EventBus,
}

/// What assembling the graph hands back besides the facade.
struct Assembled {
    client: NyanpasuClient,
    storage: Storage,
    paths: PathResolver,
    events: EventBus,
}

impl Server {
    /// Builds every owner and returns the ready facade.
    ///
    /// # Errors
    /// Returns the first construction failure, after stopping the owners already started.
    pub async fn start(dirs: ServerDirs) -> anyhow::Result<Self> {
        let mut owners = Owners {
            shutdown: CancellationToken::new(),
            tasks: TaskTracker::new(),
            core: None,
        };
        match assemble(dirs, &mut owners).await {
            Ok(Assembled {
                client,
                storage,
                paths,
                events,
            }) => Ok(Self {
                client,
                storage,
                paths,
                events,
            }),
            Err(error) => {
                owners.abort().await;
                Err(error)
            }
        }
    }

    pub fn client(&self) -> &NyanpasuClient {
        &self.client
    }

    /// The registry the local socket serves. A headless shell provides no shell
    /// capability, so a command that needs one is not registered.
    ///
    /// # Errors
    /// Returns an error when two linked commands share a name.
    pub fn rpc(&self) -> anyhow::Result<UnifiedRpc> {
        UnifiedRpc::new(
            RpcDependencies {
                client: self.client.clone(),
                storage: self.storage.clone(),
                paths: self.paths.clone(),
                events: self.events.clone(),
            },
            &[],
        )
        .context("build the RPC registry")
    }

    /// Orderly shutdown: every owner stops admitting work, then the core stops.
    pub async fn shutdown(self) {
        self.client.request_shutdown();
        self.client.wait_shutdown().await;
    }
}

// SPIKE: finding F4. `BundleMetadata` is shaped by the desktop bundle (window
// label, WebView, portable layout, updater feeds), so a server has to invent
// values for fields it does not have.
fn bundle_metadata() -> BundleMetadata {
    BundleMetadata {
        app_version: env!("CARGO_PKG_VERSION"),
        package_version: env!("CARGO_PKG_VERSION"),
        build_date: "1970-01-01T00:00:00Z",
        is_appimage: false,
        main_window_label: "",
        is_portable: false,
        is_fixed_webview: false,
        release_channel: ReleaseChannel::Stable,
        sourceforge_project: None,
    }
}

async fn assemble(dirs: ServerDirs, owners: &mut Owners) -> anyhow::Result<Assembled> {
    let bundle_metadata = bundle_metadata();
    let paths = PathResolver::with_base_dirs(dirs.config_dir, dirs.data_dir);
    paths
        .create_base_dirs()
        .context("create base directories")?;
    nyanpasu_migration::Runner::with_paths(
        paths.clone(),
        bundle_metadata,
        false,
        Arc::new(RedbHotkeyStoreOpener),
    )
    .context("prepare migrations")?
    .run_pending()
    .context("run migrations")?;
    let runtime_paths = RuntimePaths::from_resolver(&paths);

    // The system service is unavailable, but its IPC client type is part of the
    // service port and of the service log port.
    let service_ipc = nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER)
        .context("build service IPC client")?;
    // SPIKE: finding F5. `local_host::build` requires the Windows shutdown helper
    // path on every platform; this path is never run.
    let shutdown_helper = paths
        .app_data_dir()
        .join("nyanpasu-service")
        .into_std_path_buf();
    let stop_strategy = Arc::new(nyanpasu_utils::process::AtomicStopStrategy::new(
        nyanpasu_utils::process::StopStrategy::default(),
    ));
    // The core dies with the server, so a crash cannot leave a core holding the
    // listeners while the supervisor restarts the server.
    let control = nyanpasu_core::actor_v2::local_host::build(
        &paths,
        &shutdown_helper,
        stop_strategy,
        CoreLifetime::BoundToHost,
    )
    .await
    .context("build core host")?;
    let local: EndpointHandle = Arc::new(LocalEndpoint::new(control));
    let core_v2 = CoreClient::spawn(local).await.context("spawn core actor")?;
    owners.core = Some(core_v2.clone());
    let service = ServiceClient::spawn(
        Arc::new(NoServiceHost::new(service_ipc.clone())),
        RESTART_BUDGET,
    )
    .await
    .context("spawn service actor")?;
    service.drain_on_shutdown(&owners.tasks, owners.shutdown.child_token());
    let jobs = nyanpasu_core::client::jobs::start(
        paths.jobs_path().into_std_path_buf(),
        nyanpasu_core::client::jobs::capture(),
        owners.shutdown.child_token(),
        &owners.tasks,
    )
    .await
    .context("start jobs")?;

    let storage = Storage::try_new(paths.storage_path().as_std_path()).context("open storage")?;
    let geo_index = Arc::new(nyanpasu_core::geo::FsCountryIndexSource::new(
        paths.app_data_dir().as_std_path().to_owned(),
        paths.cache_dir().join("geodata").into_std_path_buf(),
    ));
    let network = Arc::new(ReqwestNetworkAdapter::new(
        bundle_metadata,
        Arc::new(NoProxyEnvironment),
    ));
    let events = EventBus::new(Vec::new());
    let ui_sink: Arc<dyn UiEventSink> = Arc::new(BusUiEvents::new(events.clone()));

    let args = ClientSetupArgs {
        bundle_metadata,
        host_adapters: host_adapters(&paths, &runtime_paths, bundle_metadata, network),
        jobs,
        logging: logging(&paths, service_ipc),
        paths: paths.clone(),
        storage: storage.clone(),
        runtime_paths,
        ui_sink,
        // Absent: the client then uses its unavailable updater backend.
        app_update_backend_factory: None,
        app_update_event_sink: None,
        core_v2,
        service,
        system_dns: Arc::new(OsSystemDnsCache),
        direct_egress: Arc::new(HttpDirectEgressProbe::dnspod()),
        geo_index,
        os_proxy: Arc::new(NoOsProxy),
        binary_installer: Arc::new(
            nyanpasu_core::client::core_lifecycle::adapters::FsBinaryInstaller,
        ),
        core_intent_store: Some(Arc::new(FileCoreIntent::new(
            paths
                .app_config_dir()
                .join(CORE_INTENT_FILE)
                .into_std_path_buf(),
        ))),
        core_versions: Arc::new(CoreBinaryVersions::new(paths.clone())),
        effects: Arc::new(UnsupportedEffects),
        window: Arc::new(NoWindows),
        accelerators: Arc::new(NoAccelerators),
        // Recording is optional; this spike does not open the traffic store.
        traffic_store: None,
        shutdown: owners.shutdown.clone(),
        tasks: owners.tasks.clone(),
    };
    let client = NyanpasuClient::try_new(args)
        .await
        .context("build client")?;
    Ok(Assembled {
        client,
        storage,
        paths,
        events,
    })
}

fn host_adapters(
    paths: &PathResolver,
    runtime_paths: &RuntimePaths,
    bundle_metadata: BundleMetadata,
    network: Arc<ReqwestNetworkAdapter>,
) -> ClientHostAdapters {
    ClientHostAdapters {
        icon_fetcher: network.clone(),
        network_diagnostics: network,
        debug_http: Arc::new(|| Box::pin(async { Ok(Arc::new(DisabledDebugHttp) as Arc<_>) })),
        changelog_http: Arc::new(move |ports| {
            Arc::new(nyanpasu_net::changelog_http::ReqwestChangelogHttp::new(
                ports,
                bundle_metadata,
                Arc::new(NoProxyEnvironment),
            ))
        }),
        runtime_build: Arc::new(
            nyanpasu_core::client::runtime_build::RuntimeBuildService::new(
                Arc::new(FsProfileContentSource::new(
                    paths.app_profiles_dir().into_std_path_buf(),
                )),
                {
                    let paths = paths.clone();
                    Arc::new(move |core| {
                        nyanpasu_core::actor_v2::local_host::core_spec(core, &paths)
                    })
                },
                runtime_paths.clone(),
                {
                    let dirs = nyanpasu_scripts::ScriptDirs::new(
                        paths.scripts_dir().into_std_path_buf(),
                        paths.cache_dir().into_std_path_buf(),
                    );
                    Arc::new(move || {
                        Ok(Box::new(nyanpasu_scripts::EnhanceScriptRunner::new(
                            dirs.clone(),
                        )?))
                    })
                },
            ),
        ),
        profile_files: Arc::new({
            let paths = paths.clone();
            let device_info = nyanpasu_core::utils::hwid::get_device_info();
            let device_headers = DeviceHeaders {
                hwid: device_info.hwid,
                device_os: device_headers::sanitize_for_header(&device_info.device_os),
                os_version: device_headers::sanitize_for_header(&device_info.os_version),
                device_model: device_headers::sanitize_for_header(&device_info.device_model),
            };
            move |ports| {
                let service = Arc::new(ProfileFileService::new(paths.clone()));
                ProfileFilePorts {
                    fs: service.clone(),
                    fetcher: Arc::new(SubscriptionHttpFetcher::new(
                        ports,
                        bundle_metadata,
                        Arc::new(NoProxyEnvironment),
                        device_headers.clone(),
                    )),
                    materialization: service,
                }
            }
        }),
    }
}

fn logging(
    paths: &PathResolver,
    service_ipc: nyanpasu_ipc::client::Client,
) -> nyanpasu_core::client::logs::LoggingSetup {
    nyanpasu_core::client::logs::LoggingSetup {
        core: match nyanpasu_core::logs::RedbCoreLogStore::open(
            paths.app_logs_dir().join("core").into_std_path_buf(),
        ) {
            Ok(store) => Box::new(store),
            Err(error) => {
                tracing::warn!(%error, "core log storage unavailable");
                Box::new(nyanpasu_core::logs::UnavailableCoreLogStore(format!(
                    "{error:#}"
                )))
            }
        },
        files: Arc::new(nyanpasu_logging::FsLogFiles::new(
            paths.app_logs_dir().into_std_path_buf(),
            APP_NAME.into(),
        )),
        clock: Arc::new(nyanpasu_logging::MonotonicClock::default()),
        service: Arc::new(nyanpasu_core::client::logs::IpcServiceLogs::new(
            service_ipc,
        )),
        frontend: Arc::new(nyanpasu_core::client::frontend_events::TracingFrontendLogSink),
    }
}
