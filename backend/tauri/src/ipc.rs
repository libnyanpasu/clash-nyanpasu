use crate::{
    client::{
        ClientError, NyanpasuClient, RuntimeError, SystemDnsError, effects::error::EffectsError,
        system_proxy::ports::OsProxyError,
    },
    core::{storage::Storage, updater::ManifestVersionLatest, *},
    enhance::PostProcessingOutput,
    state::{
        config_error::ConfigError,
        profiles::{InvalidSubscriptionUrlSnafu, ProfileFileMissingSnafu, ProfilesError},
    },
    utils::{
        candy,
        collect::EnvInfo,
        dirs, help,
        proxy_env::{self, CopyEnvOption},
        resolve,
    },
};
use anyhow::Context;
use chrono::Local;
use indexmap::IndexMap;
use log::debug;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, result::Result as StdResult};
use storage::{StorageOperationError, WebStorage};
use tauri::{AppHandle, Manager, State};
use tray::icon::TrayIcon;

use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for IpcError {}

/// A failed command as the frontend receives it.
#[derive(Debug, Serialize, specta::Type)]
pub struct IpcError {
    /// The domain failure; the frontend localizes it.
    kind: IpcErrorKind,
    /// The error's own message, shown when `kind` cannot be localized.
    message: String,
    /// The original error, copied by the user for diagnosis.
    detail: String,
}

/// The domain a command failed in. A domain joins once its errors are typed.
#[derive(Debug, Serialize, specta::Type)]
#[serde(tag = "domain", content = "error", rename_all = "snake_case")]
pub enum IpcErrorKind {
    /// Not classified into a domain; only `message` describes it.
    Unknown,
    Profiles(Box<ProfilesError>),
    Runtime(Box<RuntimeError>),
    Config(Box<ConfigError>),
    Storage(Box<StorageOperationError>),
    SystemDns(Box<SystemDnsError>),
    SystemProxy(Box<OsProxyError>),
    Effects(Box<EffectsError>),
}

impl From<EffectsError> for IpcErrorKind {
    fn from(error: EffectsError) -> Self {
        Self::Effects(Box::new(error))
    }
}

impl From<OsProxyError> for IpcErrorKind {
    fn from(error: OsProxyError) -> Self {
        Self::SystemProxy(Box::new(error))
    }
}

impl From<SystemDnsError> for IpcErrorKind {
    fn from(error: SystemDnsError) -> Self {
        Self::SystemDns(Box::new(error))
    }
}

impl From<StorageOperationError> for IpcErrorKind {
    fn from(error: StorageOperationError) -> Self {
        Self::Storage(Box::new(error))
    }
}

impl From<ConfigError> for IpcErrorKind {
    fn from(error: ConfigError) -> Self {
        Self::Config(Box::new(error))
    }
}

impl From<RuntimeError> for IpcErrorKind {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(Box::new(error))
    }
}

impl From<ProfilesError> for IpcErrorKind {
    fn from(error: ProfilesError) -> Self {
        Self::Profiles(Box::new(error))
    }
}

impl From<ClientError> for IpcErrorKind {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Profiles(error) => Self::Profiles(Box::new(error)),
            ClientError::Runtime(error) => Self::Runtime(Box::new(error)),
            ClientError::Config(error) => Self::Config(Box::new(error)),
            ClientError::Storage(error) => Self::Storage(Box::new(error)),
            _ => Self::Unknown,
        }
    }
}

impl<E> From<E> for IpcError
where
    E: std::fmt::Display + std::fmt::Debug,
    IpcErrorKind: From<E>,
{
    fn from(error: E) -> Self {
        Self {
            message: error.to_string(),
            detail: format!("{error:?}"),
            kind: error.into(),
        }
    }
}

macro_rules! unknown_domain {
    ($($error:ty),* $(,)?) => {$(
        impl From<$error> for IpcErrorKind {
            fn from(_: $error) -> Self {
                Self::Unknown
            }
        }
    )*};
}

unknown_domain!(
    String,
    std::io::Error,
    serde_yaml::Error,
    serde_json::Error,
    tauri::Error,
    anyhow::Error,
);

type Result<T = ()> = StdResult<T, IpcError>;

// TODO: remove this struct use Sysproxy
#[derive(specta::Type, serde::Serialize)]
pub struct GetSysProxyResponse {
    // Sysproxy fields (manually defined),
    // because specta not support serde(flatten)
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub bypass: String,

    // old version compatible
    pub server: String,
}

// ---- profiles domain commands (PR-3 T08, thin adapters over NyanpasuClient) ----

use crate::state::profiles::actor::NewProfileRequest;
use nyanpasu_config::profile::{
    ProfileDefinition, ProfileId, ProfileMetadataPatch, Profiles as DomainProfiles,
    RemoteProfileOptionsPatch, TransformKind,
};

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_profiles(client: State<'_, NyanpasuClient>) -> Result<DomainProfiles> {
    Ok((*client.get_profiles().await?).clone())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn is_portable(client: State<'_, NyanpasuClient>) -> Result<bool> {
    Ok(client.is_portable())
}

// #[tauri::command]
// #[specta::specta]
// pub fn get_device_info() -> Result<crate::utils::hwid::DeviceInfo> {
//     Ok(crate::utils::hwid::get_device_info())
// }

/// Rebuild-only command: there is no prior state commit, so a failure is a
/// plain error — the committed/degraded model (spec §6.2) does not apply.
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn enhance_profiles(client: State<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    name: Option<String>,
    option: Option<RemoteProfileOptionsPatch>,
    transform: Option<TransformKind>,
) -> Result<crate::client::runtime::MutationOutcome<ProfileId>> {
    let url = snafu::ResultExt::context(
        url::Url::parse(&url),
        InvalidSubscriptionUrlSnafu { url: &url },
    )?;
    // `name` carries deep-link intent (e.g. an install-config `name=` param);
    // when absent the facade derives the name from the url server-side. Return
    // MutationOutcome so a degraded post-import rebuild still carries the uid.
    Ok(client.import_profile(url, name, option, transform).await?)
}

/// Emitted to the frontend after a `clash-nyanpasu`/`clash` custom-scheme deep
/// link joins [`PendingDeepLinks`]. It carries no URL: it only asks a listening
/// frontend to take the queue through [`take_pending_deep_links`].
///
/// Event name: `scheme-request-received-event` (derived by `tauri_specta`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct SchemeRequestReceivedEvent;

/// Deep links no frontend has taken yet, oldest first: the cold-start link from
/// argv and every link a later instance forwards. A link leaves the queue only
/// when a frontend takes it, so one that arrives while no frontend listens,
/// before the window loads or while it reloads, waits for the next frontend
/// to mount and drain it. Managed Tauri state, not a global singleton.
#[derive(Debug, Default)]
pub struct PendingDeepLinks(std::sync::Mutex<Vec<String>>);

impl PendingDeepLinks {
    pub fn push(&self, url: String) {
        self.0.lock().unwrap().push(url);
    }

    /// Takes every queued link, oldest first, and leaves the queue empty.
    pub fn take_all(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Take and clear the queued deep links, oldest first. The frontend calls it
/// once its [`SchemeRequestReceivedEvent`] listener is registered, and again on
/// every such event.
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn take_pending_deep_links(pending: State<'_, PendingDeepLinks>) -> Result<Vec<String>> {
    Ok(pending.take_all())
}

/// create a new profile
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result<crate::client::runtime::MutationOutcome<ProfileId>> {
    // Must return the created ProfileId; never drop it on the wire.
    Ok(client.create_profile(request, file_data).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn reorder_profile(
    client: State<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.reorder_profile(active_id, over_id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: State<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.reorder_profiles_by_list(list).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_status(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<crate::client::jobs::ProfileSyncStatus> {
    Ok(client.profile_sync_status(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_runs(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    after: Option<nyanpasu_jobs::dto::RunCursorDto>,
) -> Result<nyanpasu_jobs::dto::RunPageDto> {
    Ok(client.profile_sync_runs(uid, after).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_logs(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    run: String,
    after: Option<String>,
) -> Result<nyanpasu_jobs::dto::LogPageDto> {
    Ok(client.profile_sync_logs(uid, run, after).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn update_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.refresh_profile(uid, option).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn delete_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.delete_profile(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn activate_profile(
    client: State<'_, NyanpasuClient>,
    uid: Option<ProfileId>,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.activate_profile(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_global_transforms(
    client: State<'_, NyanpasuClient>,
    ids: Vec<ProfileId>,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.set_global_transforms(ids).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: State<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.set_profile_valid_fields(fields).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_profile_metadata(uid, patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_remote_profile_options(uid, patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn replace_profile_definition(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.replace_profile_definition(uid, definition).await?)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn view_profile(
    app_handle: tauri::AppHandle,
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result {
    let path = client.get_profile_materialized_path(uid.clone()).await?;
    if !path.exists() {
        return Err(ProfileFileMissingSnafu { uid, path: &path }.build().into());
    }
    help::open_file(app_handle, path)?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn read_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn save_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.save_profile_file(uid, file_data).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_clash_info(client: State<'_, NyanpasuClient>) -> Result<crate::client::ClashInfo> {
    Ok(client.clash_info())
}

/// get the runtime config
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion. Replace with a typed ClashConfig struct if desired.
pub async fn get_runtime_config(
    client: State<'_, NyanpasuClient>,
) -> Result<Option<specta_typescript::Any<serde_json::Value>>> {
    Ok(client
        .runtime_config()
        .await?
        .map(|config| serde_json::from_value(config).expect("a JSON value deserializes as itself")))
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_runtime_yaml(client: State<'_, NyanpasuClient>) -> Result<String> {
    Ok(client.runtime_yaml().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn inspect_runtime(
    client: State<'_, NyanpasuClient>,
) -> Result<Option<crate::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_runtime().await)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn inspect_applied_runtime(
    client: State<'_, NyanpasuClient>,
) -> Result<Option<crate::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_applied_runtime().await)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn inspect_runtime_node(
    client: State<'_, NyanpasuClient>,
    snapshot_id: String,
    node_id: u32,
) -> Result<crate::client::runtime_inspection::RuntimeInspectionContent> {
    Ok(client.inspect_runtime_node(&snapshot_id, node_id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_runtime_exists(client: State<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.exists_keys.clone())
        .unwrap_or_default())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_postprocessing_output(
    client: State<'_, NyanpasuClient>,
) -> Result<PostProcessingOutput> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.postprocessing_output.clone())
        .unwrap_or_default())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_core_status(
    client: State<'_, NyanpasuClient>,
) -> Result<crate::core::actor_v2::CoreStatusInfo> {
    Ok(client.core_status().into())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn url_delay_test(
    client: State<'_, NyanpasuClient>,
    url: String,
    expected_status: u16,
) -> Result<Option<u64>> {
    Ok(crate::utils::net::url_delay_test(&url, expected_status, client.clash_info().port).await)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion.
pub async fn get_ipsb_asn(
    client: State<'_, NyanpasuClient>,
) -> Result<specta_typescript::Any<serde_json::Value>> {
    let value = crate::utils::net::get_ipsb_asn(client.clash_info().port).await?;
    let wrapped: specta_typescript::Any<serde_json::Value> = serde_json::from_value(value)?;
    Ok(wrapped)
}

// ---- typed configuration commands (thin adapters over NyanpasuClient) ----

use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch},
};

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_app_config(client: State<'_, NyanpasuClient>) -> Result<NyanpasuAppConfig> {
    Ok(client.get_app_config().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn patch_app_config(
    client: State<'_, NyanpasuClient>,
    patch: NyanpasuAppConfigPatch,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_app_config(patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_clash_config(client: State<'_, NyanpasuClient>) -> Result<ClashConfig> {
    Ok(client.get_clash_config().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn patch_clash_config(
    client: State<'_, NyanpasuClient>,
    patch: ClashConfigPatch,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_clash_config(patch).await?)
}

/// patch the clash guard overrides (mode, log level, LAN, IPv6, secret...)
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
#[tracing_attributes::instrument(skip_all)]
pub async fn patch_runtime_overrides(
    client: State<'_, NyanpasuClient>,
    patch: ClashGuardOverridesPatch,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    // Explicit-field whitelist so future patch fields never auto-leak into
    // logs; the secret is reported by presence only.
    tracing::debug!(
        log_level = ?patch.log_level,
        allow_lan = ?patch.allow_lan,
        mode = ?patch.mode,
        secret = patch.secret.is_some(),
        unified_delay = ?patch.unified_delay,
        tcp_concurrent = ?patch.tcp_concurrent,
        ipv6 = ?patch.ipv6,
        "patch_runtime_overrides"
    );
    Ok(client.patch_runtime_overrides(patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    crate::client::hotkey::ports::HotkeyAction::all()
        .iter()
        .map(|action| action.as_str())
        .collect()
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn change_clash_core(client: State<'_, NyanpasuClient>, clash_core: ClashCore) -> Result {
    client.update_core(clash_core).await?;
    Ok(())
}

/// restart the sidecar
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn restart_sidecar(client: State<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

/// get the system proxy
/// server field is the combination of host and port
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_sys_proxy(client: State<'_, NyanpasuClient>) -> Result<GetSysProxyResponse> {
    let current = client.get_os_proxy().await?;

    let server = format!("{}:{}", current.host, current.port);

    Ok(GetSysProxyResponse {
        enable: current.enable,
        host: current.host,
        port: current.port,
        bypass: current.bypass,
        server,
    })
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn flush_system_dns_cache(client: State<'_, NyanpasuClient>) -> Result {
    client.flush_system_dns_cache().await?;
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn open_app_config_dir() -> Result<()> {
    let config_dir = (dirs::app_config_dir())?;
    (crate::utils::open::that(config_dir))?;
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn open_app_data_dir() -> Result<()> {
    let data_dir = (dirs::app_data_dir())?;
    (crate::utils::open::that(data_dir))?;
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn open_core_dir() -> Result<()> {
    let core_dir = (tauri::utils::platform::current_exe())?;
    let core_dir = core_dir
        .parent()
        .ok_or("failed to get core dir".to_string())?;
    (crate::utils::open::that(core_dir))?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_core_dir() -> Result<String> {
    let core_dir = (tauri::utils::platform::current_exe())?;
    let core_dir = core_dir
        .parent()
        .ok_or("failed to get core dir".to_string())?;
    let core_dir = dunce::canonicalize(core_dir)?;
    Ok(core_dir.to_string_lossy().to_string())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn open_logs_dir() -> Result<()> {
    let log_dir = (dirs::app_logs_dir())?;
    (crate::utils::open::that(log_dir))?;
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn open_web_url(url: String) -> Result<()> {
    (crate::utils::open::that(url))?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn fetch_latest_core_versions(
    client: State<'_, NyanpasuClient>,
) -> Result<ManifestVersionLatest> {
    Ok(client.fetch_latest_core_versions().await?)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn get_core_version(app_handle: AppHandle, core_type: ClashCore) -> Result<String> {
    Ok(snafu::ResultExt::context(
        resolve::resolve_core_version(&app_handle, &core_type).await,
        crate::client::runtime_error::ReadCoreVersionSnafu,
    )?)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn collect_logs(app_handle: AppHandle) -> Result {
    let now = Local::now().format("%Y-%m-%d");
    let fname = format!("{now}-log");
    let builder = FileDialogBuilder::new(app_handle.dialog().clone());
    builder
        .add_filter("archive files", &["zip"])
        .set_file_name(&fname)
        .set_title("Save log archive")
        .save_file(|file_path| match file_path {
            Some(path) if path.as_path().is_some() => {
                debug!("{path:#?}");
                match candy::collect_logs(path.as_path().unwrap()) {
                    Ok(_) => (),
                    Err(err) => {
                        log::error!(target: "app", "{err:?}");
                    }
                }
            }
            _ => (),
        });
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn update_core(client: State<'_, NyanpasuClient>, core_type: ClashCore) -> Result<usize> {
    Ok(client.download_core_update(core_type).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn inspect_updater(
    client: State<'_, NyanpasuClient>,
    updater_id: usize,
) -> Result<updater::UpdaterSummary> {
    Ok(client.inspect_updater(updater_id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_proxy_delay(
    client: State<'_, NyanpasuClient>,
    name: String,
    provider: Option<String>,
    url: Option<String>,
) -> Result<clash::api::DelayRes> {
    Ok(client.proxy_delay(name, provider, url).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_configs(
    client: State<'_, NyanpasuClient>,
) -> Result<clash::api::ClashConfig> {
    Ok(client.clash_configs().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_delete_connections(
    client: State<'_, NyanpasuClient>,
    id: Option<String>,
) -> Result<()> {
    Ok(client.close_clash_connections(id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_version(
    client: State<'_, NyanpasuClient>,
) -> Result<clash::api::ClashVersion> {
    Ok(client.clash_version().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_rules(
    client: State<'_, NyanpasuClient>,
) -> Result<clash::api::RulesRes> {
    Ok(client.clash_rules().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_providers_rules(
    client: State<'_, NyanpasuClient>,
) -> Result<clash::api::ProvidersRulesRes> {
    Ok(client.clash_rule_providers().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_update_providers_rules(
    client: State<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_clash_rule_provider(name).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    client: State<'_, NyanpasuClient>,
    group: String,
    url: Option<String>,
) -> Result<IndexMap<String, u32>> {
    Ok(client.group_delay(group, url).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_providers_proxies(
    client: State<'_, NyanpasuClient>,
) -> Result<clash::api::ProvidersProxiesRes> {
    Ok(client.proxy_providers().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_proxies(
    client: State<'_, NyanpasuClient>,
) -> Result<crate::core::clash::proxies::Proxies> {
    Ok(client.get_proxies().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn mutate_proxies(
    client: State<'_, NyanpasuClient>,
) -> Result<crate::core::clash::proxies::Proxies> {
    Ok(client.refresh_proxies().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn select_proxy(
    client: State<'_, NyanpasuClient>,
    group: String,
    name: String,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.select_proxy(group, name).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn update_proxy_provider(client: State<'_, NyanpasuClient>, name: String) -> Result<()> {
    Ok(client.update_proxy_provider(name).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn collect_envs<'a>() -> Result<EnvInfo<'a>> {
    Ok((crate::utils::collect::collect_envs())?)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn open_that(path: String) -> Result {
    (crate::utils::open::that(path))?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn is_appimage() -> Result<bool> {
    Ok(*crate::consts::IS_APPIMAGE)
}

#[cfg(windows)]
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn get_custom_app_dir() -> Result<Option<String>> {
    use crate::utils::winreg::get_app_dir;
    match get_app_dir() {
        Ok(Some(path)) => Ok(Some(path.to_string_lossy().to_string())),
        Ok(None) => Ok(None),
        Err(err) => Err(IpcError::from(err)),
    }
}

#[cfg(not(windows))]
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn get_custom_app_dir() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(windows)]
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn set_custom_app_dir(app_handle: tauri::AppHandle, path: String) -> Result {
    use crate::utils::{self, dialog::migrate_dialog, winreg::set_app_dir};
    use rust_i18n::t;
    use std::path::PathBuf;

    let path_str = path.clone();
    let path = PathBuf::from(path);

    // show a dialog to ask whether to migrate the data
    let res =
        tauri::async_runtime::spawn_blocking(move || {
            let msg = t!("dialog.custom_app_dir_migrate", path = path_str).to_string();

            if migrate_dialog(&msg) {
                let app_exe = tauri::utils::platform::current_exe()?;
                let app_exe = dunce::canonicalize(app_exe)?.to_string_lossy().to_string();
                std::process::Command::new("powershell")
                    .arg("-Command")
                    .arg(
                    format!(
                        r#"Start-Process '{}' -ArgumentList 'migrate-home-dir','"{}"' -Verb runAs"#,
                        app_exe.as_str(),
                        path_str.as_str()
                    )
                    .as_str(),
                ).spawn().unwrap().wait()?;
                utils::help::quit_application(&app_handle);
            } else {
                set_app_dir(&path)?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await;
    ((res)?)?;
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn restart_application(app_handle: tauri::AppHandle) -> Result {
    crate::utils::help::restart_application(&app_handle);
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn get_server_port(port: State<'_, crate::server::ServerPort>) -> Result<u16> {
    Ok(port.0)
}

#[cfg(not(windows))]
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn set_custom_app_dir(_path: String) -> Result {
    Ok(())
}

#[cfg(windows)]
pub mod uwp {
    use super::Result;
    use crate::{core::win_uwp, utils::path::PathResolver};
    use tauri::State;

    #[nyanpasu_macro::rpc]
    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool(paths: State<'_, PathResolver>) -> Result {
        (win_uwp::invoke_uwptools(paths.app_resources_dir()?).await)?;
        Ok(())
    }
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn set_tray_icon(
    app_handle: tauri::AppHandle,
    mode: TrayIcon,
    path: Option<PathBuf>,
) -> Result {
    (crate::core::tray::icon::set_icon(mode, path))?;
    // Checked here, so a bad icon reaches the caller; only applying it to the
    // tray is queued.
    (crate::core::tray::icon::check_icon(&crate::core::tray::icon::get_icon(&mode)))?;
    (crate::core::tray::Tray::request(&app_handle, crate::core::tray::TrayWork::PART))?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn is_tray_icon_set(mode: TrayIcon) -> Result<bool> {
    let icon_path = (crate::utils::dirs::tray_icons_path(mode.as_str()))?;
    Ok(tokio::fs::metadata(icon_path).await.is_ok())
}

pub mod service {
    use super::{NyanpasuClient, Result};
    use tauri::State;

    /// `StatusInfo` 的 additive 镜像：字段逐一复制（specta 不支持 `serde(flatten)`，
    /// 见本文件 `GetSysProxyResponse` 的同款处理），追加 actor 投影字段。
    /// wire 是原结构的严格超集，前端既有消费点不受影响。
    #[derive(serde::Serialize, specta::Type)]
    pub struct ServiceStatusInfo {
        pub name: std::borrow::Cow<'static, str>,
        pub version: std::borrow::Cow<'static, str>,
        pub status: nyanpasu_ipc::types::ServiceStatus,
        pub server: Option<nyanpasu_ipc::api::status::StatusResBody<'static>>,
        pub compat: crate::core::service::compat::ServiceCompat,
        pub phase: crate::core::actor_v2::service_actor::ServicePhase,
        pub restart_attempts: u8,
    }

    #[nyanpasu_macro::rpc(http)]
    #[tauri::command]
    #[specta::specta]
    pub async fn status_service(client: State<'_, NyanpasuClient>) -> Result<ServiceStatusInfo> {
        let info = client.service_status();
        Ok(ServiceStatusInfo {
            name: info.name,
            version: info.version,
            status: info.status,
            server: info.server,
            compat: info.compat,
            phase: info.phase,
            restart_attempts: info.restart_attempts,
        })
    }

    #[nyanpasu_macro::rpc]
    #[tauri::command]
    #[specta::specta]
    pub async fn install_service(client: State<'_, NyanpasuClient>) -> Result {
        client.install_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc]
    #[tauri::command]
    #[specta::specta]
    pub async fn uninstall_service(client: State<'_, NyanpasuClient>) -> Result {
        client.uninstall_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc]
    #[tauri::command]
    #[specta::specta]
    pub async fn start_service(client: State<'_, NyanpasuClient>) -> Result {
        client.start_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc]
    #[tauri::command]
    #[specta::specta]
    pub async fn stop_service(client: State<'_, NyanpasuClient>) -> Result {
        client.stop_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc]
    #[tauri::command]
    #[specta::specta]
    pub async fn restart_service(client: State<'_, NyanpasuClient>) -> Result {
        client.restart_service().await?;
        Ok(())
    }
}

#[cfg(not(windows))]
pub mod uwp {
    use super::*;

    #[nyanpasu_macro::rpc]
    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool() -> Result {
        Ok(())
    }
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_service_install_prompt() -> Result<String> {
    let args = snafu::ResultExt::context(
        crate::core::service::control::get_service_install_args().await,
        crate::client::runtime_error::PrepareServiceInstallPromptSnafu,
    )?
    .into_iter()
    .map(|arg| {
        #[cfg(unix)]
        {
            format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''"))
        }
        #[cfg(windows)]
        {
            arg.to_string_lossy().to_string()
        }
    })
    .collect::<Vec<_>>()
    .join(" ");
    let mut prompt = format!("./nyanpasu-service {args}");
    if cfg!(not(windows)) {
        prompt = format!("sudo {prompt}");
    }
    Ok(prompt)
}

/// Shuts every owner down and returns with the app still running; the caller
/// then installs an update or relaunches.
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn cleanup_processes(app_handle: AppHandle) -> Result {
    crate::utils::exit::clean_up(&app_handle).await;
    Ok(())
}

/// Namespace prefix for all frontend-visible KV entries.
/// Internal subsystems (e.g. task storage) use un-prefixed keys and are
/// never exposed to the frontend through these IPC commands.
const WEB_STORAGE_KEY_PREFIX: &str = "web:";

fn web_key(key: &str) -> String {
    format!("{WEB_STORAGE_KEY_PREFIX}{key}")
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_storage_item(storage: State<'_, Storage>, key: String) -> Result<Option<String>> {
    let value = (storage.get_item(web_key(&key)))?;
    Ok(value)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn set_storage_item(storage: State<'_, Storage>, key: String, value: String) -> Result {
    (storage.set_item(web_key(&key), &value))?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn remove_storage_item(storage: State<'_, Storage>, key: String) -> Result {
    (storage.remove_item(web_key(&key)))?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_hotkeys(client: State<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client.get_app_config().await?.hotkeys)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_hotkeys(
    client: State<'_, NyanpasuClient>,
    hotkeys: Vec<String>,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    // An unparsable list is rejected before anything is written; a shortcut the
    // OS refuses lands as a degradation on a committed config.
    Ok(client
        .patch_app_config(nyanpasu_config::application::NyanpasuAppConfigPatch {
            hotkeys: Some(hotkeys),
            ..Default::default()
        })
        .await?)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StorageEntry {
    pub key: String,
    /// Raw JSON-encoded value string.
    pub value: String,
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_all_storage_items(storage: State<'_, Storage>) -> Result<Vec<StorageEntry>> {
    let items = storage.get_all()?;
    Ok(items
        .into_iter()
        .filter_map(|(raw_key, value)| {
            raw_key
                .strip_prefix(WEB_STORAGE_KEY_PREFIX)
                .map(|key| StorageEntry {
                    key: key.to_string(),
                    value,
                })
        })
        .collect())
}

/// Debug: clears all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are left intact.
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn clear_storage(storage: State<'_, Storage>) -> Result {
    let web_keys: Vec<String> = storage
        .get_all()?
        .into_iter()
        .filter(|(k, _)| k.starts_with(WEB_STORAGE_KEY_PREFIX))
        .map(|(k, _)| k)
        .collect();
    for key in web_keys {
        storage.remove_item(&key)?;
    }
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    client: tauri::State<'_, NyanpasuClient>,
) -> Result<crate::core::clash::ws::ClashWsSnapshot> {
    Ok(client.clash_ws_snapshot().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_traffic_summary(
    client: tauri::State<'_, NyanpasuClient>,
) -> Result<nyanpasu_traffic::TrafficSummary> {
    Ok(client.traffic_summary().await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn query_traffic_report(
    client: tauri::State<'_, NyanpasuClient>,
    request: nyanpasu_traffic::ReportRequest,
) -> Result<nyanpasu_traffic::TrafficReport> {
    Ok(client.query_traffic_report(request).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn query_traffic_usage(
    client: tauri::State<'_, NyanpasuClient>,
    query: nyanpasu_traffic::TrafficQuery,
    group_by: nyanpasu_traffic::Dimension,
    after: Option<nyanpasu_traffic::UsageCursor>,
    limit: usize,
) -> Result<nyanpasu_traffic::UsagePage> {
    Ok(client
        .query_traffic_usage(query, group_by, after, limit)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn query_traffic_usage_by_keys(
    client: tauri::State<'_, NyanpasuClient>,
    query: nyanpasu_traffic::TrafficQuery,
    group_by: nyanpasu_traffic::Dimension,
    keys: Vec<String>,
) -> Result<Vec<nyanpasu_traffic::UsageGroup>> {
    Ok(client
        .query_traffic_usage_by_keys(query, group_by, keys)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn query_traffic_closed_connections(
    client: tauri::State<'_, NyanpasuClient>,
    before: Option<nyanpasu_traffic::ClosedCursor>,
    limit: usize,
) -> Result<nyanpasu_traffic::ClosedPage> {
    Ok(client
        .query_traffic_closed_connections(before, limit)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_clash_ws_recording(
    client: tauri::State<'_, NyanpasuClient>,
    kind: crate::core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<crate::core::clash::ws::ClashWsRecording> {
    Ok(client.set_clash_ws_recording(kind, enabled).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clear_clash_ws_history(
    client: tauri::State<'_, NyanpasuClient>,
    kind: crate::core::clash::ws::ClashWsKind,
) -> Result {
    client.clear_clash_ws_history(kind).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn subscribe_clash_connection_details(
    webview: tauri::Webview,
    client: tauri::State<'_, NyanpasuClient>,
    subscriptions: tauri::State<
        '_,
        crate::core::clash::connection_details::ConnectionDetailSubscriptions,
    >,
    on_frame: tauri::ipc::Channel<crate::core::clash::ws::ClashConnectionDetails>,
) -> Result<crate::core::clash::connection_details::SubscriptionId> {
    let receiver = client.subscribe_clash_connection_details();
    let parent = client.shutdown_child_token();
    let (id, cancel) = subscriptions.register(&parent, webview.label().to_string());
    client.spawn_tracked(
        &cancel,
        crate::core::clash::connection_details::forward_details(receiver, on_frame),
    );
    Ok(id)
}

#[tauri::command]
#[specta::specta]
pub fn unsubscribe_clash_connection_details(
    subscriptions: tauri::State<
        '_,
        crate::core::clash::connection_details::ConnectionDetailSubscriptions,
    >,
    id: crate::core::clash::connection_details::SubscriptionId,
) -> Result {
    subscriptions.unsubscribe(id);
    Ok(())
}

// Updater block

#[nyanpasu_macro::rpc(http, result)]
#[tauri::command]
#[specta::specta]
pub async fn list_log_files(
    client: tauri::State<'_, NyanpasuClient>,
    source: crate::client::logs::LogSource,
) -> nyanpasu_logging::LogResult<Vec<nyanpasu_logging::LogFileInfo>> {
    client.list_log_files(source).await
}
#[nyanpasu_macro::rpc(http, result, owner)]
#[tauri::command]
#[specta::specta]
pub async fn open_log_session(
    window: tauri::Window,
    client: tauri::State<'_, NyanpasuClient>,
    source: crate::client::logs::LogSource,
    request: nyanpasu_logging::OpenLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogSession> {
    client
        .open_log_session(source, window.label().to_string(), request)
        .await
}
#[nyanpasu_macro::rpc(http, result, owner)]
#[tauri::command]
#[specta::specta]
pub async fn query_logs(
    window: tauri::Window,
    client: tauri::State<'_, NyanpasuClient>,
    source: crate::client::logs::LogSource,
    request: nyanpasu_logging::QueryLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogPage> {
    client
        .query_logs(source, window.label().to_string(), request)
        .await
}
#[nyanpasu_macro::rpc(http, result, owner)]
#[tauri::command]
#[specta::specta]
pub async fn close_log_session(
    window: tauri::Window,
    client: tauri::State<'_, NyanpasuClient>,
    source: crate::client::logs::LogSource,
    session: String,
) -> nyanpasu_logging::LogResult<()> {
    client
        .close_log_session(source, window.label().to_string(), session)
        .await
}

#[derive(Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UpdateDownload {
    source: nyanpasu_config::application::UpdateSource,
    rid: tauri::ResourceId,
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
// TODO: a copied from updater metadata, and should be moved a separate updater module
pub struct UpdateWrapper {
    downloads: Vec<UpdateDownload>,
    available: bool,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    // TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value).
    #[specta(type = specta_typescript::Any)]
    raw_json: serde_json::Value,
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn get_release_channel(
    client: State<'_, NyanpasuClient>,
) -> Result<crate::bundle::Channel> {
    Ok(client.release_channel().await?)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn set_release_channel(
    client: State<'_, NyanpasuClient>,
    channel: crate::bundle::Channel,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.set_release_channel(channel).await?)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn check_update(
    webview: tauri::Webview,
    client: State<'_, NyanpasuClient>,
) -> Result<Option<UpdateWrapper>> {
    use crate::utils::config::{get_self_proxy, get_system_proxy};
    use tauri_plugin_updater::UpdaterExt;

    let build_time = time::OffsetDateTime::parse(
        crate::consts::BUILD_INFO.build_date,
        &time::format_description::well_known::Rfc3339,
    )
    .context("failed to parse build time")?;
    let channel = client.release_channel().await?;
    let local = semver::Version::parse(crate::consts::BUILD_INFO.pkg_version)
        .context("invalid application version")?;
    let mut builder = webview
        .updater_builder()
        .endpoints(
            crate::bundle::update_endpoints(channel)
                .into_iter()
                .map(|endpoint| endpoint.parse())
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("invalid update endpoint")?,
        )
        .context("failed to configure update endpoints")?
        .version_comparator(move |_, remote| {
            crate::bundle::is_newer_release(channel, &local, &remote, build_time)
        });
    // apply proxy
    builder = builder.proxy(
        get_self_proxy(client.clash_info().port)
            .parse()
            .context("failed to parse proxy")?,
    );
    if let Ok(Some(proxy)) = get_system_proxy() {
        builder = builder.proxy(proxy.parse().context("failed to parse system proxy")?);
    }
    let updater = builder.build().context("failed to build updater")?;
    let update = updater.check().await.context("failed to check update")?;
    update
        .map(|u| {
            let downloads = client.update_download_urls(&u.download_url)?;
            let mut wrapper = UpdateWrapper {
                available: true,
                current_version: u.current_version.clone(),
                version: u.version.clone(),
                date: u.date.and_then(|d| {
                    d.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                }),
                body: u.body.clone(),
                raw_json: u.raw_json.clone(),
                ..Default::default()
            };
            wrapper.downloads = downloads
                .into_iter()
                .map(|(source, url)| {
                    let mut download = u.clone();
                    // A source changes only the download route; every attempt must verify
                    // against the same release version, signature and public key.
                    download.download_url = url;
                    UpdateDownload {
                        source,
                        rid: webview.resources_table().add(download),
                    }
                })
                .collect();
            Ok(wrapper)
        })
        .transpose()
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn save_window_size_state(app_handle: AppHandle, label: String) -> Result<()> {
    if label == crate::consts::MAIN_WINDOW_LABEL {
        resolve::save_main_window_state_async(&app_handle, true).await?;
    }
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn create_main_window(app_handle: AppHandle) -> Result<()> {
    // Spawn window creation to avoid blocking
    std::thread::spawn(move || {
        // Small delay to let the IPC return first
        std::thread::sleep(std::time::Duration::from_millis(10));
        let handle_inner = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            resolve::create_main_window(&handle_inner);
        });
    });
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn create_debug_tray_menu_window(app_handle: AppHandle) -> Result<()> {
    // Spawn window creation to avoid blocking
    std::thread::spawn(move || {
        // Small delay to let the IPC return first
        std::thread::sleep(std::time::Duration::from_millis(10));
        let handle_inner = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            let _ = resolve::create_debug_tray_menu_window(&handle_inner);
        });
    });
    Ok(())
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn copy_clash_env(
    app_handle: AppHandle,
    client: State<'_, NyanpasuClient>,
    env_type: CopyEnvOption,
) {
    proxy_env::copy_clash_env(&app_handle, client.clash_info().port, &env_type);
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn quit_application(app_handle: AppHandle) {
    crate::utils::help::quit_application(&app_handle);
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub fn create_editor_window(
    app_handle: AppHandle,
    window_type: resolve::EditorWindowType,
    uid: Option<String>,
) -> Result<()> {
    // Spawn window creation to avoid blocking
    std::thread::spawn(move || {
        // Small delay to let the IPC return first
        std::thread::sleep(std::time::Duration::from_millis(10));
        let handle_inner = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            let _ = resolve::create_editor_window(&handle_inner, window_type, uid.as_deref());
        });
    });
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_system_accent_color() -> Result<Option<String>> {
    Ok(crate::utils::color::get_system_accent_color())
}

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ConfigurationStatusChanged(pub crate::client::configuration_status::ConfigurationStatus);

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_configuration_status(
    client: State<'_, NyanpasuClient>,
) -> crate::client::configuration_status::ConfigurationStatus {
    client.configuration_status()
}
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn retry_configuration_runtime(client: State<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.retry_runtime_now().await?)
}
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn retry_configuration_effect(
    client: State<'_, NyanpasuClient>,
    kind: crate::client::effects::plan::EffectKind,
) -> Result<()> {
    Ok(client.retry_effect_now(kind)?)
}

#[cfg(test)]
mod tests {
    use super::{ClientError, IpcError, PendingDeepLinks, ProfilesError, SystemDnsError};
    use nyanpasu_config::profile::ProfileId;
    use nyanpasu_core::state::ReplaceIfVersionError;
    use serde_json::json;
    use snafu::IntoError;

    use crate::{
        client::{
            effects::error::EffectsError, runtime_error::RuntimeError,
            system_proxy::ports::OsProxyError,
        },
        state::{
            mutation::{CommitAborted, RuntimeAftermath, WriteConfigSnafu},
            profiles::{ProfileFileError, SubscriptionFetchError},
        },
    };

    fn wire(error: impl Into<ClientError>) -> serde_json::Value {
        serde_json::to_value(IpcError::from(error.into())).unwrap()
    }

    #[test]
    fn a_profiles_error_reaches_the_frontend_as_its_own_domain() {
        let wire = wire(ProfilesError::ProfileNotFound {
            uid: ProfileId("p1".into()),
        });

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "profiles",
                "error": { "kind": "profile_not_found", "uid": "p1" },
            })
        );
        assert_eq!(wire["message"], "profile not found: p1");
    }

    #[test]
    fn a_runtime_error_reaches_the_frontend_with_the_cores_own_kind() {
        let failure = crate::client::runtime_error::ApplyRuntimeSnafu.into_error(
            nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::ApplyFailed,
                "the core kept the previous configuration",
                false,
            ),
        );
        let wire = wire(failure);

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "runtime",
                "error": {
                    "kind": "apply_runtime",
                    "failure": {
                        "kind": "apply_failed",
                        "message": "the core kept the previous configuration",
                        "retryable": false,
                        "operation_id": null,
                    },
                },
            })
        );
        assert_eq!(
            serde_json::to_value(IpcError::from(super::RuntimeError::Isolated)).unwrap()["kind"],
            json!({ "domain": "runtime", "error": { "kind": "isolated" } })
        );
    }

    #[test]
    fn a_dns_flush_failure_names_the_command_and_its_exit_code() {
        let wire = serde_json::to_value(IpcError::from(SystemDnsError::FlushRejected {
            command: "ipconfig.exe",
            code: Some(5),
        }))
        .unwrap();

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "system_dns",
                "error": { "kind": "flush_rejected", "command": "ipconfig.exe", "code": 5 },
            })
        );
    }

    #[test]
    fn a_stopped_effects_owner_reaches_the_frontend_as_its_own_domain() {
        let wire = serde_json::to_value(IpcError::from(EffectsError::EffectsStopped)).unwrap();

        assert_eq!(
            wire["kind"],
            json!({ "domain": "effects", "error": { "kind": "effects_stopped" } })
        );
    }

    #[test]
    fn a_failed_os_proxy_write_names_where_it_was_going() {
        let wire = serde_json::to_value(IpcError::from(OsProxyError::WriteOsProxy {
            enable: true,
            host: "127.0.0.1".into(),
            port: 7890,
            source: "access denied".into(),
        }))
        .unwrap();

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "system_proxy",
                "error": {
                    "kind": "write_os_proxy",
                    "enable": true,
                    "host": "127.0.0.1",
                    "port": 7890,
                },
            })
        );
        assert!(wire["detail"].as_str().unwrap().contains("access denied"));
    }

    #[test]
    fn unit_variants_and_context_free_kinds_serialize_their_tag_only() {
        assert_eq!(
            wire(ProfilesError::ShuttingDown)["kind"],
            json!({ "domain": "profiles", "error": { "kind": "shutting_down" } })
        );
        assert_eq!(
            wire(ClientError::Custom("no domain".into()))["kind"],
            json!({ "domain": "unknown" })
        );
    }

    #[test]
    fn nested_domain_errors_are_serialized_and_library_sources_are_not() {
        let fetch = wire(ProfilesError::FetchSubscription {
            url: "https://sub.example/x".parse().unwrap(),
            source: SubscriptionFetchError::SubscriptionHttpStatus { status: 404 },
        });
        assert_eq!(
            fetch["kind"]["error"],
            json!({
                "kind": "fetch_subscription",
                "url": "https://sub.example/x",
                "source": { "kind": "subscription_http_status", "status": 404 },
            })
        );

        let read = wire(ProfilesError::ReadProfileFile {
            uid: ProfileId("p1".into()),
            source: ProfileFileError::mock("disk full"),
        });
        assert_eq!(
            read["kind"]["error"],
            json!({
                "kind": "read_profile_file",
                "uid": "p1",
                "source": { "kind": "write_file", "path": "mock" },
            }),
            "the io error stays out of the wire form"
        );
        assert!(
            read["detail"].as_str().unwrap().contains("disk full"),
            "and reaches the user through the copied detail: {}",
            read["detail"]
        );
    }

    #[test]
    fn an_aborted_commit_names_what_became_of_the_runtime() {
        let cause = ReplaceIfVersionError::WriteConfig(anyhow::anyhow!("disk full"));
        let aborted = WriteConfigSnafu {
            runtime: RuntimeAftermath::RollbackFailed {
                detail: std::sync::Arc::new(RuntimeError::OwnerUnresponsive {
                    operation_id: "op1".into(),
                }),
            },
        }
        .into_error(cause);
        assert!(matches!(aborted, CommitAborted::WriteConfig { .. }));

        let wire = wire(ProfilesError::Commit {
            source: aborted,
            cleanup_failures: Vec::new(),
        });
        assert_eq!(
            wire["kind"]["error"],
            json!({
                "kind": "commit",
                "source": {
                    "kind": "write_config",
                    "runtime": {
                        "kind": "rollback_failed",
                        "detail": { "kind": "owner_unresponsive", "operation_id": "op1" },
                    },
                },
            })
        );
    }

    #[test]
    fn a_version_conflict_does_not_serialize_its_cleanup_failures() {
        let wire = wire(ProfilesError::VersionConflict {
            expected: 3,
            actual: 4,
            cleanup_failures: vec![ProfilesError::ShuttingDown],
        });
        assert_eq!(
            wire["kind"]["error"],
            json!({ "kind": "version_conflict", "expected": 3, "actual": 4 })
        );
    }

    #[test]
    fn unclassified_error_serializes_message_and_original_error() {
        let error = anyhow::anyhow!("disk full").context("failed to save the profile");
        let wire = serde_json::to_value(IpcError::from(error)).unwrap();

        assert_eq!(wire["kind"], serde_json::json!({ "domain": "unknown" }));
        assert_eq!(wire["message"], "failed to save the profile");
        let detail = wire["detail"].as_str().unwrap();
        assert!(
            detail.contains("failed to save the profile") && detail.contains("disk full"),
            "detail keeps the whole source chain: {detail}"
        );
    }

    #[test]
    fn deep_links_queued_while_no_frontend_listens_are_taken_oldest_first() {
        let pending = PendingDeepLinks::default();
        assert!(pending.take_all().is_empty());

        pending.push("clash://install-config?url=https%3A%2F%2Fa".into());
        pending.push("clash://install-config?url=https%3A%2F%2Fb".into());
        pending.push("clash://install-config?url=https%3A%2F%2Fa".into());
        assert_eq!(
            pending.take_all(),
            [
                "clash://install-config?url=https%3A%2F%2Fa",
                "clash://install-config?url=https%3A%2F%2Fb",
                "clash://install-config?url=https%3A%2F%2Fa",
            ]
        );
        assert!(pending.take_all().is_empty(), "taking empties the queue");

        pending.push("clash://install-config?url=https%3A%2F%2Fc".into());
        assert_eq!(
            pending.take_all(),
            ["clash://install-config?url=https%3A%2F%2Fc"]
        );
    }
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_debug_http_status(
    client: State<'_, NyanpasuClient>,
) -> Result<crate::server::debug_http::DebugHttpStatus> {
    Ok(client.debug_http_status().await?)
}
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn set_debug_http_enabled(
    client: State<'_, NyanpasuClient>,
    enabled: bool,
) -> Result<crate::server::debug_http::DebugHttpStatus> {
    Ok(client.set_debug_http_enabled(enabled).await?)
}
