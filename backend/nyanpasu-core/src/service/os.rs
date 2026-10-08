//! Production OS boundary for `ServiceActor`.
//!
//! This module deliberately has no unit tests: it only forwards to the
//! platform service commands. Compatibility classification and actor phase
//! transitions are covered with fake adapters in `actor` tests.

use std::{path::PathBuf, sync::Arc};

use snafu::ResultExt;

use crate::{
    control::endpoint::{EndpointHandle, ServiceEndpoint},
    service::{
        actor::ServiceHostAdapter,
        control::{ResolveServiceDirsSnafu, ServiceCommandError},
    },
};

pub struct OsServiceHostAdapter {
    client: nyanpasu_ipc::client::Client,
    service_binary: PathBuf,
    paths: nyanpasu_paths::PathResolver,
}

impl OsServiceHostAdapter {
    pub fn new(
        client: nyanpasu_ipc::client::Client,
        service_binary: PathBuf,
        paths: nyanpasu_paths::PathResolver,
    ) -> Self {
        Self {
            client,
            service_binary,
            paths,
        }
    }
}

#[async_trait::async_trait]
impl ServiceHostAdapter for OsServiceHostAdapter {
    async fn probe(&self) -> Result<nyanpasu_ipc::types::StatusInfo<'static>, ServiceCommandError> {
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

    fn endpoint(&self) -> EndpointHandle {
        Arc::new(ServiceEndpoint::new(self.client.clone()))
    }
}

#[cfg(target_os = "macos")]
pub(super) mod sudo;
