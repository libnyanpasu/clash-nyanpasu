//! IPC endpoint construction after the application service version gate.

use std::sync::Arc;

use super::{
    endpoint::{EndpointHandle, ServiceEndpoint},
    service_actor::ServiceEndpointFactory,
};

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
