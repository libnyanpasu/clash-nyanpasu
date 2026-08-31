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

impl ServiceHostAdapter for OsServiceHostAdapter {
    fn probe(
        &self,
    ) -> super::endpoint::BoxFuture<'_, Result<nyanpasu_ipc::types::StatusInfo<'static>, String>>
    {
        Box::pin(async {
            crate::core::service::control::status()
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn install(&self) -> super::endpoint::BoxFuture<'_, Result<(), String>> {
        Box::pin(async {
            crate::core::service::control::install_service()
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn uninstall(&self) -> super::endpoint::BoxFuture<'_, Result<(), String>> {
        Box::pin(async {
            crate::core::service::control::uninstall_service()
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn start_daemon(&self) -> super::endpoint::BoxFuture<'_, Result<(), String>> {
        Box::pin(async {
            crate::core::service::control::start_service()
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn stop_daemon(&self) -> super::endpoint::BoxFuture<'_, Result<(), String>> {
        Box::pin(async {
            crate::core::service::control::stop_service()
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn update(&self) -> super::endpoint::BoxFuture<'_, Result<(), String>> {
        Box::pin(async {
            crate::core::service::control::update_service()
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn endpoint(&self) -> EndpointHandle {
        Arc::new(ServiceEndpoint::new(
            nyanpasu_ipc::client::shortcuts::Client::service_default().clone(),
        ))
    }
}
