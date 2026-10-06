//! Desktop adapters around the application-owned core lifecycle.

pub mod facade;
pub mod intent;
pub mod local_host;
pub mod service_actor;
pub mod service_host_adapter;

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct CoreStatusChangedEvent(pub nyanpasu_application::core::CoreStatusInfo);

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ServiceStatusChangedEvent(pub service_actor::ServiceHostStatus);
