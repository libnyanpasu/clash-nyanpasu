use nyanpasu_core::{control::CoreStatusInfo, service::actor::ServiceHostStatus};

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct CoreStatusChangedEvent(pub CoreStatusInfo);

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ServiceStatusChangedEvent(pub ServiceHostStatus);
