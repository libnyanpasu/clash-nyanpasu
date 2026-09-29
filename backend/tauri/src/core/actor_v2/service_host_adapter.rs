//! Production OS boundary for `ServiceActor`.
//!
//! This module deliberately has no unit tests: it only forwards to the
//! platform service commands. Compatibility classification and actor phase
//! transitions are covered with fake adapters in `service_actor` tests.

use std::{path::PathBuf, sync::Arc};

use super::{
    endpoint::{EndpointHandle, ServiceEndpoint},
    service_actor::ServiceHostAdapter,
};
use crate::core::service::control::ServiceCommandError;

pub struct OsServiceHostAdapter {
    client: nyanpasu_ipc::client::Client,
    service_binary: PathBuf,
}

impl OsServiceHostAdapter {
    pub fn new(client: nyanpasu_ipc::client::Client, service_binary: PathBuf) -> Self {
        Self {
            client,
            service_binary,
        }
    }
}

#[async_trait::async_trait]
impl ServiceHostAdapter for OsServiceHostAdapter {
    async fn probe(&self) -> Result<nyanpasu_ipc::types::StatusInfo<'static>, ServiceCommandError> {
        crate::core::service::control::status(&self.service_binary).await
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        crate::core::service::control::install_service(&self.service_binary).await
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        crate::core::service::control::uninstall_service(&self.service_binary).await
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        crate::core::service::control::start_service(&self.service_binary).await
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        crate::core::service::control::stop_service(&self.service_binary).await
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        crate::core::service::control::update_service(&self.service_binary).await
    }

    fn endpoint(&self) -> EndpointHandle {
        Arc::new(ServiceEndpoint::new(self.client.clone()))
    }
}
