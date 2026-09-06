//! Production OS boundary for `ServiceActor`.
//!
//! This module deliberately has no unit tests: it only forwards to the
//! platform service commands. Compatibility classification and actor phase
//! transitions are covered with fake adapters in `service_actor` tests.

use std::sync::Arc;

use super::{
    endpoint::{EndpointHandle, ServiceEndpoint},
    service_actor::ServiceHostAdapter,
};

pub struct OsServiceHostAdapter;

#[async_trait::async_trait]
impl ServiceHostAdapter for OsServiceHostAdapter {
    async fn probe(&self) -> Result<nyanpasu_ipc::types::StatusInfo<'static>, String> {
        crate::core::service::control::status()
            .await
            .map_err(|e| e.to_string())
    }

    async fn install(&self) -> Result<(), String> {
        crate::core::service::control::install_service()
            .await
            .map_err(|e| e.to_string())
    }

    async fn uninstall(&self) -> Result<(), String> {
        crate::core::service::control::uninstall_service()
            .await
            .map_err(|e| e.to_string())
    }

    async fn start_daemon(&self) -> Result<(), String> {
        crate::core::service::control::start_service()
            .await
            .map_err(|e| e.to_string())
    }

    async fn stop_daemon(&self) -> Result<(), String> {
        crate::core::service::control::stop_service()
            .await
            .map_err(|e| e.to_string())
    }

    async fn update(&self) -> Result<(), String> {
        crate::core::service::control::update_service()
            .await
            .map_err(|e| e.to_string())
    }

    // TODO(actor-migration): temporary bridge to the process-wide
    // `nyanpasu_ipc::client::shortcuts::Client` shortcut.
    // Reason: the IPC client is not yet injected from the composition root.
    // Remove when: `ServiceHostAdapter`'s constructor accepts the client as a
    // dependency and this call is replaced with the injected instance.
    fn endpoint(&self) -> EndpointHandle {
        Arc::new(ServiceEndpoint::new(
            nyanpasu_ipc::client::shortcuts::Client::service_default().clone(),
        ))
    }
}
