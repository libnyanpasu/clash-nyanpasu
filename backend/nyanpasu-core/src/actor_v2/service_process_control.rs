//! Production OS boundary for `ServiceActor`.
//!
//! This module deliberately has no unit tests: it only forwards to the
//! platform service commands. Compatibility classification and actor phase
//! transitions are covered with fake adapters in `service_actor` tests.

use std::{path::PathBuf, sync::Arc};

use snafu::ResultExt;

use super::{
    endpoint::{EndpointHandle, ServiceEndpoint},
    service_actor::ServiceEndpointFactory,
};
use nyanpasu_contracts::service::{
    ResolveServiceDirsSnafu, ServiceCommandError, ServiceProcessControl,
};

pub struct OsServiceProcessControl {
    service_binary: PathBuf,
    paths: nyanpasu_paths::PathResolver,
}

impl OsServiceProcessControl {
    pub fn new(service_binary: PathBuf, paths: nyanpasu_paths::PathResolver) -> Self {
        Self {
            service_binary,
            paths,
        }
    }
}

#[async_trait::async_trait]
impl ServiceProcessControl for OsServiceProcessControl {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        crate::service::control::status(&self.service_binary).await
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        let app_dir = self
            .paths
            .app_install_dir()
            .context(ResolveServiceDirsSnafu)?;
        crate::service::control::install_service(
            &self.service_binary,
            self.paths.app_data_dir().as_std_path(),
            self.paths.app_config_dir().as_std_path(),
            app_dir.as_std_path(),
        )
        .await
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        crate::service::control::uninstall_service(&self.service_binary).await
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        crate::service::control::start_service(&self.service_binary).await
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        crate::service::control::stop_service(&self.service_binary).await
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        crate::service::control::update_service(
            &self.service_binary,
            self.paths.app_data_dir().as_std_path(),
        )
        .await
    }
}

/// Constructs the application endpoint only after the service version gate passes.
pub struct IpcServiceEndpointFactory {
    client: nyanpasu_ipc::client::Client,
}

impl IpcServiceEndpointFactory {
    pub fn new(client: nyanpasu_ipc::client::Client) -> Self {
        Self { client }
    }
}

impl ServiceEndpointFactory for IpcServiceEndpointFactory {
    fn endpoint(&self) -> EndpointHandle {
        Arc::new(ServiceEndpoint::new(self.client.clone()))
    }
}
