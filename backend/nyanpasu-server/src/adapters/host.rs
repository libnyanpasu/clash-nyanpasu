//! Adapters for desktop-only ports, each answering "unsupported" truthfully.

use std::{borrow::Cow, io, sync::Arc};

use nyanpasu_config::application::ClashCore;
use nyanpasu_contracts::net::SystemProxySource;
use nyanpasu_core::{
    actor_v2::{
        endpoint::{EndpointHandle, ServiceEndpoint},
        service_actor::ServiceHostAdapter,
    },
    client::{
        StateChanged, UiEventSink,
        core_version::{
            CoreVersionError, CoreVersionExitSnafu, CoreVersionReader, RunCoreVersionSnafu,
            parse_version,
        },
        debug_http::{DebugHttpControl, DebugHttpStatus},
        hotkey::ports::{AcceleratorValidator, HotkeyParseError, WindowControl, WindowError},
        system_proxy::ports::{OsProxyConfig, OsProxyError, OsProxyPort},
    },
    service::control::{ServiceCommand, ServiceCommandError},
};
use nyanpasu_ipc::types::{ServiceStatus, StatusInfo};
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc::EventBus;
use nyanpasu_rpc_protocol::EventName;
use snafu::ResultExt as _;

const UNSUPPORTED: &str = "not supported by the headless host";

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, UNSUPPORTED)
}

/// Frontend state-change hints go to the event bus, as in the desktop shell.
/// The local socket has no subscribers yet, so they reach only stream
/// subscribers that attach later.
pub struct BusUiEvents(EventBus);

impl BusUiEvents {
    pub fn new(events: EventBus) -> Self {
        Self(events)
    }
}

impl UiEventSink for BusUiEvents {
    fn state_changed(&self, state: StateChanged) {
        match serde_json::to_value(&state) {
            Ok(payload) => self.0.publish(EventName::StateChanged, payload),
            Err(error) => tracing::warn!(%error, "failed to serialize a state change"),
        }
    }
}

/// A headless host has no dashboard window to toggle.
pub struct NoWindows;

#[async_trait::async_trait]
impl WindowControl for NoWindows {
    async fn toggle_dashboard(&self) -> Result<(), WindowError> {
        Err(WindowError::ToggleDashboard {
            source: Box::new(unsupported()),
        })
    }
}

/// A headless host has no global shortcuts, so every accelerator is rejected
/// before it can be committed.
pub struct NoAccelerators;

impl NoAccelerators {
    fn reject(accelerator: &str) -> HotkeyParseError {
        HotkeyParseError::UnsupportedAccelerator {
            accelerator: accelerator.to_owned(),
            source: Box::new(unsupported()),
        }
    }
}

impl AcceleratorValidator for NoAccelerators {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        Err(Self::reject(accelerator))
    }

    fn canonical(&self, accelerator: &str) -> Result<String, HotkeyParseError> {
        Err(Self::reject(accelerator))
    }
}

/// A headless host has no OS proxy settings to read or write.
pub struct NoOsProxy;

impl OsProxyPort for NoOsProxy {
    fn get(&self) -> Result<OsProxyConfig, OsProxyError> {
        Err(OsProxyError::ReadOsProxy {
            source: Box::new(unsupported()),
        })
    }

    fn set(&self, config: &OsProxyConfig) -> Result<(), OsProxyError> {
        Err(OsProxyError::WriteOsProxy {
            enable: config.enable,
            host: config.host.clone(),
            port: config.port,
            source: Box::new(unsupported()),
        })
    }

    // SPIKE: finding F2. The port can only return a string here, so "no
    // platform bypass list" is reported as the empty list.
    fn default_bypass(&self) -> &'static str {
        ""
    }
}

/// HTTP clients ask for the OS proxy per request; a headless host has none, so
/// they connect directly.
pub struct NoProxyEnvironment;

impl SystemProxySource for NoProxyEnvironment {
    fn system_proxy(&self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

/// Runs the core binary the process manager would run and asks it for its version.
pub struct CoreBinaryVersions {
    paths: PathResolver,
}

impl CoreBinaryVersions {
    pub fn new(paths: PathResolver) -> Self {
        Self { paths }
    }
}

#[async_trait::async_trait]
impl CoreVersionReader for CoreBinaryVersions {
    async fn read(&self, core: ClashCore) -> Result<String, CoreVersionError> {
        let argument = match core {
            ClashCore::ClashRs | ClashCore::ClashRsAlpha => "-V",
            _ => "-v",
        };
        let spec = nyanpasu_core::actor_v2::local_host::core_spec(&core, &self.paths)
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })?;
        let output = tokio::process::Command::new(spec.binary_path.as_std_path())
            .arg(argument)
            .output()
            .await
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })?;
        snafu::ensure!(output.status.success(), CoreVersionExitSnafu { core });
        parse_version(core, &String::from_utf8_lossy(&output.stdout))
    }
}

/// The debug HTTP server serves the desktop frontend; there is none to serve.
pub struct DisabledDebugHttp;

#[async_trait::async_trait]
impl DebugHttpControl for DisabledDebugHttp {
    async fn status(&self) -> anyhow::Result<DebugHttpStatus> {
        Ok(DebugHttpStatus::default())
    }

    async fn set_enabled(&self, enabled: bool) -> anyhow::Result<DebugHttpStatus> {
        if enabled {
            anyhow::bail!("the debug HTTP server is {UNSUPPORTED}");
        }
        Ok(DebugHttpStatus::default())
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A headless host has no privileged system service: the probe reports it as not
/// installed and every command that would change that is refused.
pub struct NoServiceHost {
    client: nyanpasu_ipc::client::Client,
}

impl NoServiceHost {
    pub fn new(client: nyanpasu_ipc::client::Client) -> Self {
        Self { client }
    }

    fn refused(command: ServiceCommand) -> ServiceCommandError {
        ServiceCommandError::RunElevated {
            command,
            source: unsupported(),
        }
    }
}

#[async_trait::async_trait]
impl ServiceHostAdapter for NoServiceHost {
    // SPIKE: finding F3. The only honest answer is "not installed". An `Err` is
    // read as an unknown daemon that may hold a core, and then startup refuses to
    // start any core (`RecoveryRequired`); the port has no "this host has no
    // service" answer.
    async fn probe(&self) -> Result<StatusInfo<'static>, ServiceCommandError> {
        Ok(StatusInfo {
            name: Cow::Borrowed("nyanpasu-service"),
            version: Cow::Borrowed(""),
            status: ServiceStatus::NotInstalled,
            server: None,
        })
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        Err(Self::refused(ServiceCommand::Install))
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        Err(Self::refused(ServiceCommand::Uninstall))
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        Err(Self::refused(ServiceCommand::Start))
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        Err(Self::refused(ServiceCommand::Stop))
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        Err(Self::refused(ServiceCommand::Update))
    }

    // SPIKE: finding F6. The port requires an endpoint even for a host with no
    // service. The IPC endpoint is returned; it is only reachable after a probe
    // succeeds, which this adapter never allows.
    fn endpoint(&self) -> EndpointHandle {
        Arc::new(ServiceEndpoint::new(self.client.clone()))
    }
}
