use nyanpasu_core::{
    control::CoreStatusInfo, logs::CoreLogStatus, service::actor::ServiceHostStatus,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct CoreStatusChangedEvent(pub CoreStatusInfo);

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ServiceStatusChangedEvent(pub ServiceHostStatus);

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CoreLogsChanged {
    pub status: CoreLogStatus,
}
