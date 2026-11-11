//! The thirteen reviewed local commands; target and capability metadata stay in declarations.

use nyanpasu_config::profile::{NewProfileRequest, ProfileId, Profiles};
use nyanpasu_contracts::logs::LogSource;
use nyanpasu_logging::{
    LogError, LogFileInfo, LogPage, LogSession, OpenLogs, QueryLogs as LogQuery,
};
use nyanpasu_runtime_values::{
    configuration::ConfigurationStatus,
    core_status::CoreStatusInfo,
    runtime::MutationOutcome,
    storage::{StorageItem, StorageRevision},
};
use serde::{Deserialize, Serialize};

use crate::{
    operation::{NoDomainError, RpcOperation},
    types,
};

/// Creates a profile with the existing nested definition and optional file contents.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProfileRequest {
    pub request: NewProfileRequest,
    pub file_data: Option<String>,
}

/// Removes the identified profile.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProfileRequest {
    pub uid: ProfileId,
}

/// Reads the identified profile's file.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadProfileFileRequest {
    pub uid: ProfileId,
}

/// Reads a frontend storage key in the existing namespace.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetStorageItemRequest {
    pub key: String,
}

/// Writes a frontend storage value.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetStorageItemRequest {
    pub key: String,
    pub value: String,
}

/// Removes a frontend storage key.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveStorageItemRequest {
    pub key: String,
}

/// Lists files from an application or service log source.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLogFilesRequest {
    pub source: LogSource,
}

/// Opens a log session owned by the transport-supplied caller.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenLogSessionRequest {
    pub source: LogSource,
    pub request: OpenLogs,
}

/// Queries a log session; nested log fields retain their existing serialized names.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryLogsRequest {
    pub source: LogSource,
    pub request: LogQuery,
}

/// Closes a session owned by the transport-supplied caller.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseLogSessionRequest {
    pub source: LogSource,
    pub session: String,
}

macro_rules! operations {
    ($($operation:ident: $name:literal, $request:ty => $response:ty, $received:ty, $error:ty;)*) => {
        $(
            #[doc = concat!("Contract for `", $name, "`; does not grant transport admission.")]
            pub struct $operation;

            impl RpcOperation for $operation {
                const NAME: &'static str = $name;
                type Request = $request;
                type Response = $response;
                type ReceivedResponse = $received;
                type DomainError = $error;
            }
        )*
    };
}

operations! {
    GetProfiles: "get_profiles", () => Profiles, Profiles, types::IpcError;
    CreateProfile: "create_profile", CreateProfileRequest =>
        MutationOutcome<ProfileId>, types::MutationOutcome<ProfileId>, types::IpcError;
    DeleteProfile: "delete_profile", DeleteProfileRequest =>
        MutationOutcome<()>, types::MutationOutcome<()>, types::IpcError;
    ReadProfileFile: "read_profile_file", ReadProfileFileRequest => String, String, types::IpcError;
    GetConfigurationStatus: "get_configuration_status", () =>
        ConfigurationStatus, ConfigurationStatus, NoDomainError;
    GetCoreStatus: "get_core_status", () => CoreStatusInfo, CoreStatusInfo, types::IpcError;
    GetStorageItem: "get_storage_item", GetStorageItemRequest =>
        StorageItem<String>, StorageItem<String>, types::IpcError;
    SetStorageItem: "set_storage_item", SetStorageItemRequest =>
        StorageRevision, StorageRevision, types::IpcError;
    RemoveStorageItem: "remove_storage_item", RemoveStorageItemRequest =>
        StorageRevision, StorageRevision, types::IpcError;
    ListLogFiles: "list_log_files", ListLogFilesRequest =>
        Vec<LogFileInfo>, Vec<LogFileInfo>, LogError;
    OpenLogSession: "open_log_session", OpenLogSessionRequest => LogSession, LogSession, LogError;
    QueryLogs: "query_logs", QueryLogsRequest => LogPage, LogPage, LogError;
    CloseLogSession: "close_log_session", CloseLogSessionRequest => (), (), LogError;
}

#[cfg(test)]
mod tests;
