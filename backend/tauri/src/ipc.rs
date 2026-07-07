use crate::{
    bridge::verge::LegacyVergeBridge,
    client::{ClientError, NyanpasuClient},
    config::*,
    core::{logger::Logger, storage::Storage, updater::ManifestVersionLatest, *},
    enhance::PostProcessingOutput,
    feat::{self, CopyEnvOption},
    utils::{candy, collect::EnvInfo, dirs, help, resolve},
};
use anyhow::Context;
use chrono::Local;
use log::debug;
use nyanpasu_ipc::api::status::CoreState;
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    path::PathBuf,
    result::Result as StdResult,
};
use storage::{StorageOperationError, WebStorage};
use sysproxy::Sysproxy;
use tauri::{AppHandle, Manager, State};
use tray::icon::TrayIcon;

use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
    #[error(transparent)]
    Storage(#[from] StorageOperationError),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    Profiles(#[from] crate::state::profiles::actor::ProfilesError),
    #[error("{0}")]
    Custom(String),
}

impl From<String> for IpcError {
    fn from(s: String) -> Self {
        IpcError::Custom(s)
    }
}

impl From<ClientError> for IpcError {
    fn from(err: ClientError) -> Self {
        match err {
            ClientError::Io(err) => IpcError::Io(err),
            ClientError::SerdeYaml(err) => IpcError::SerdeYaml(err),
            ClientError::SerdeJson(err) => IpcError::SerdeJson(err),
            ClientError::Storage(err) => IpcError::Storage(err),
            ClientError::Anyhow(err) => IpcError::Anyhow(err),
            ClientError::Profiles(err) => IpcError::Profiles(err),
            ClientError::Custom(err) => IpcError::Custom(err),
        }
    }
}

impl serde::Serialize for IpcError {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(format!("{self:#?}").as_str())
    }
}

impl specta::Type for IpcError {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        let _ = types;
        specta::datatype::DataType::Primitive(specta::datatype::Primitive::str)
    }
}

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
    RemoteProfileOptionsPatch,
};

#[tauri::command]
#[specta::specta]
pub async fn get_profiles(client: State<'_, NyanpasuClient>) -> Result<DomainProfiles> {
    Ok((*client.get_profiles().await?).clone())
}

#[cfg(target_os = "windows")]
#[tauri::command]
#[specta::specta]
pub fn is_portable() -> Result<bool> {
    Ok(crate::utils::dirs::get_portable_flag())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
#[specta::specta]
pub fn is_portable() -> Result<bool> {
    Ok(false)
}

// #[tauri::command]
// #[specta::specta]
// pub fn get_device_info() -> Result<crate::utils::hwid::DeviceInfo> {
//     Ok(crate::utils::hwid::get_device_info())
// }

#[tauri::command]
#[specta::specta]
pub async fn enhance_profiles(client: State<'_, NyanpasuClient>) -> Result {
    client.rebuild_running_config().await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<ProfileId> {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    // Return the created uid so the caller can apply user-provided metadata
    // (import derives the name from the url server-side).
    Ok(client.import_profile(url, option).await?)
}

/// create a new profile
#[tauri::command]
#[specta::specta]
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result {
    client.create_profile(request, file_data).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profile(
    client: State<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result {
    client.reorder_profile(active_id, over_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: State<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result {
    client.reorder_profiles_by_list(list).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result {
    client.refresh_profile(uid, option).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_profile(client: State<'_, NyanpasuClient>, uid: ProfileId) -> Result {
    client.delete_profile(uid).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn activate_profile(client: State<'_, NyanpasuClient>, uid: Option<ProfileId>) -> Result {
    client.activate_profile(uid).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_global_transforms(
    client: State<'_, NyanpasuClient>,
    ids: Vec<ProfileId>,
) -> Result {
    client.set_global_transforms(ids).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: State<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result {
    client.set_profile_valid_fields(fields).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result {
    client.patch_profile_metadata(uid, patch).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result {
    client.patch_remote_profile_options(uid, patch).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn replace_profile_definition(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result {
    client.replace_profile_definition(uid, definition).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn view_profile(
    app_handle: tauri::AppHandle,
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result {
    let path = client.get_profile_materialized_path(uid).await?;
    if !path.exists() {
        return Err(IpcError::Custom("profile file not found".into()));
    }
    help::open_file(app_handle, path)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn read_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn save_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result {
    client.save_profile_file(uid, file_data).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_clash_info() -> Result<ClashInfo> {
    Ok(Config::clash().latest().get_client_info())
}

/// get the runtime config
#[tauri::command]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion. Replace with a typed ClashConfig struct if desired.
pub fn get_runtime_config() -> Result<Option<specta_typescript::Any<serde_json::Value>>> {
    let config = Config::runtime().latest().config.clone();
    match config {
        Some(cfg) => {
            let yaml_value = serde_yaml::to_value(cfg)?;
            let json_value = serde_json::to_value(&yaml_value)?;
            let wrapped: specta_typescript::Any<serde_json::Value> =
                serde_json::from_value(json_value)?;
            Ok(Some(wrapped))
        }
        None => Ok(None),
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_runtime_yaml() -> Result<String> {
    let runtime = Config::runtime();
    let runtime = runtime.latest();
    let config = runtime.config.as_ref();
    let mapping = (config
        .ok_or(anyhow::anyhow!("failed to parse config to yaml file"))
        .and_then(|config| {
            serde_yaml::to_string(config).context("failed to convert config to yaml")
        }))?;
    Ok(mapping)
}

#[tauri::command]
#[specta::specta]
pub fn get_runtime_exists() -> Result<Vec<String>> {
    Ok(Config::runtime().latest().exists_keys.clone())
}

#[tauri::command]
#[specta::specta]
pub fn get_postprocessing_output() -> Result<PostProcessingOutput> {
    Ok(Config::runtime().latest().postprocessing_output.clone())
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_status() -> Result<(Cow<'static, CoreState>, i64, RunType)> {
    // TODO(actor-migration): compatibility bridge for legacy core manager status.
    // Reason: core lifecycle/status is not yet owned by an injected typed client here.
    // Remove when: CoreClient exposes typed status through NyanpasuClient or command adapters.
    Ok(CoreManager::global().status().await)
}

#[tauri::command]
#[specta::specta]
pub async fn url_delay_test(url: &str, expected_status: u16) -> Result<Option<u64>> {
    Ok(crate::utils::net::url_delay_test(url, expected_status).await)
}

#[tauri::command]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion.
pub async fn get_ipsb_asn() -> Result<specta_typescript::Any<serde_json::Value>> {
    let value = crate::utils::net::get_ipsb_asn().await?;
    let wrapped: specta_typescript::Any<serde_json::Value> = serde_json::from_value(value)?;
    Ok(wrapped)
}

/// patch clash runtime config
#[tauri::command]
#[specta::specta]
#[tracing_attributes::instrument]
pub async fn patch_clash_config(payload: PatchRuntimeConfig) -> Result {
    tracing::debug!("patch_clash_config: {payload:?}");

    let mapping = match serde_yaml::to_value(&payload)? {
        serde_yaml::Value::Mapping(m) => m,
        _ => return Err(IpcError::Custom("Expected a mapping".to_string())),
    };

    (crate::core::clash::api::patch_configs(&mapping).await)?;

    if let Err(e) = feat::patch_clash(mapping).await {
        tracing::error!("{e}");
        return Err(IpcError::from(e));
    }

    feat::update_proxies_buff(None);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_verge_config(legacy: State<'_, LegacyVergeBridge>) -> Result<IVerge> {
    Ok(legacy.get_verge_config().await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    crate::core::hotkey::Hotkey::get_supported_hotkey_functions()
}

#[tauri::command]
#[specta::specta]
pub async fn patch_verge_config(legacy: State<'_, LegacyVergeBridge>, payload: IVerge) -> Result {
    legacy.patch_verge_config(payload).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn change_clash_core(
    legacy: State<'_, LegacyVergeBridge>,
    clash_core: Option<nyanpasu::ClashCore>,
) -> Result {
    // `change_core` writes `Config::verge().clash_core` directly; reseed typed actors so a
    // later pure patch does not persist stale typed state and revert the core change.
    legacy
        .run_legacy_verge_mutation(
            || async move { CoreManager::global().change_core(clash_core).await },
        )
        .await?;
    Ok(())
}

/// restart the sidecar
#[tauri::command]
#[specta::specta]
pub async fn restart_sidecar() -> Result {
    (CoreManager::global().run_core().await)?;
    Ok(())
}

/// get the system proxy
/// server field is the combination of host and port
#[tauri::command]
#[specta::specta]
pub fn get_sys_proxy() -> Result<GetSysProxyResponse> {
    let current = (Sysproxy::get_system_proxy()).context("failed to get system proxy")?;

    let server = format!("{}:{}", current.host, current.port);

    Ok(GetSysProxyResponse {
        enable: current.enable,
        host: current.host,
        port: current.port,
        bypass: current.bypass,
        server,
    })
}

#[tauri::command]
#[specta::specta]
pub fn get_clash_logs() -> Result<VecDeque<String>> {
    Ok(Logger::global().get_log())
}

#[tauri::command]
#[specta::specta]
pub fn open_app_config_dir() -> Result<()> {
    let config_dir = (dirs::app_config_dir())?;
    (crate::utils::open::that(config_dir))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn open_app_data_dir() -> Result<()> {
    let data_dir = (dirs::app_data_dir())?;
    (crate::utils::open::that(data_dir))?;
    Ok(())
}

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

#[tauri::command]
#[specta::specta]
pub fn open_logs_dir() -> Result<()> {
    let log_dir = (dirs::app_logs_dir())?;
    (crate::utils::open::that(log_dir))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn open_web_url(url: String) -> Result<()> {
    (crate::utils::open::that(url))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_latest_core_versions() -> Result<ManifestVersionLatest> {
    let mut updater = updater::UpdaterManager::global().write().await; // It is intended to block here
    (updater.fetch_latest().await)?;
    // TODO: result key should be kebab-case
    Ok(updater.get_latest_versions())
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_version(
    app_handle: AppHandle,
    core_type: nyanpasu::ClashCore,
) -> Result<String> {
    match resolve::resolve_core_version(&app_handle, &core_type).await {
        Ok(version) => Ok(version),
        Err(err) => Err(IpcError::from(err)),
    }
}

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

#[tauri::command]
#[specta::specta]
pub async fn update_core(core_type: nyanpasu::ClashCore) -> Result<usize> {
    let event_id = (updater::UpdaterManager::global()
        .write()
        .await
        .update_core(&core_type)
        .await)?;
    Ok(event_id)
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_updater(updater_id: usize) -> Result<updater::UpdaterSummary> {
    let updater = (updater::UpdaterManager::global()
        .read()
        .await
        .inspect_updater(updater_id)
        .ok_or(anyhow::anyhow!("updater is not exist")))?;
    Ok(updater)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_proxy_delay(
    name: String,
    url: Option<String>,
) -> Result<clash::api::DelayRes> {
    match clash::api::get_proxy_delay(name, url).await {
        Ok(res) => Ok(res),
        Err(err) => Err(err.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_configs() -> Result<clash::api::ClashConfig> {
    Ok(clash::api::get_configs().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_delete_connections(id: Option<String>) -> Result<()> {
    Ok(clash::api::delete_connections(id.as_deref()).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_version() -> Result<clash::api::ClashVersion> {
    Ok(clash::api::get_version().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_rules() -> Result<clash::api::RulesRes> {
    Ok(clash::api::get_rules().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_providers_rules() -> Result<clash::api::ProvidersRulesRes> {
    Ok(clash::api::get_providers_rules().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_update_providers_rules(name: String) -> Result<()> {
    Ok(clash::api::update_providers_rules_group(&name).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    group: String,
    url: Option<String>,
) -> Result<HashMap<String, u32>> {
    Ok(clash::api::get_group_delay(group, url).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_providers_proxies() -> Result<clash::api::ProvidersProxiesRes> {
    Ok(clash::api::get_providers_proxies().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_proxies() -> Result<crate::core::clash::proxies::Proxies> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    {
        let guard = ProxiesGuard::global().read();
        if guard.is_updated() {
            return Ok(guard.inner().clone());
        }
    }
    match ProxiesGuard::global().update().await {
        Ok(_) => {
            let proxies = ProxiesGuard::global().read().inner().clone();
            Ok(proxies)
        }
        Err(err) => Err(err.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn mutate_proxies() -> Result<crate::core::clash::proxies::Proxies> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    (ProxiesGuard::global().update().await)?;
    Ok(ProxiesGuard::global().read().inner().clone())
}

#[tauri::command]
#[specta::specta]
pub async fn select_proxy(group: String, name: String) -> Result<()> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    (ProxiesGuard::global().select_proxy(&group, &name).await)?;

    // Interrupt connections based on configuration
    let _ = crate::core::connection_interruption::ConnectionInterruptionService::on_proxy_change()
        .await;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_proxy_provider(name: String) -> Result<()> {
    use crate::core::clash::{
        api,
        proxies::{ProxiesGuard, ProxiesGuardExt},
    };
    (api::update_providers_proxies_group(&name).await)?;
    (ProxiesGuard::global().update().await)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn collect_envs<'a>() -> Result<EnvInfo<'a>> {
    Ok((crate::utils::collect::collect_envs())?)
}

#[tauri::command]
#[specta::specta]
pub fn open_that(path: String) -> Result {
    (crate::utils::open::that(path))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn is_appimage() -> Result<bool> {
    Ok(*crate::consts::IS_APPIMAGE)
}

#[cfg(windows)]
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
#[tauri::command]
#[specta::specta]
pub fn get_custom_app_dir() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(windows)]
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

#[tauri::command]
#[specta::specta]
pub fn restart_application(app_handle: tauri::AppHandle) -> Result {
    crate::utils::help::restart_application(&app_handle);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_server_port() -> Result<u16> {
    Ok(*crate::server::SERVER_PORT)
}

#[cfg(not(windows))]
#[tauri::command]
#[specta::specta]
pub async fn set_custom_app_dir(_path: String) -> Result {
    Ok(())
}

#[cfg(windows)]
pub mod uwp {
    use super::Result;
    use crate::core::win_uwp;

    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool() -> Result {
        (win_uwp::invoke_uwptools().await)?;
        Ok(())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn set_tray_icon(
    app_handle: tauri::AppHandle,
    mode: TrayIcon,
    path: Option<PathBuf>,
) -> Result {
    (crate::core::tray::icon::set_icon(mode, path))?;
    (crate::core::tray::Tray::update_part(&app_handle))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn is_tray_icon_set(mode: TrayIcon) -> Result<bool> {
    let icon_path = (crate::utils::dirs::tray_icons_path(mode.as_str()))?;
    Ok(tokio::fs::metadata(icon_path).await.is_ok())
}

pub mod service {
    use super::Result;
    use crate::core::service;

    #[tauri::command]
    #[specta::specta]
    pub async fn status_service<'a>() -> Result<nyanpasu_ipc::types::StatusInfo<'a>> {
        let res = (service::control::status().await)?;
        Ok(res)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn install_service() -> Result {
        (service::control::install_service().await)?;
        Ok(())
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn uninstall_service() -> Result {
        (service::control::uninstall_service().await)?;
        Ok(())
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn start_service() -> Result {
        let res = service::control::start_service().await;
        let enabled_service = {
            *crate::config::Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enabled_service && let Err(e) = crate::core::CoreManager::global().run_core().await {
            log::error!(target: "app", "{e}");
        }
        Ok(res?)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn stop_service() -> Result {
        let res = service::control::stop_service().await;
        let enabled_service = {
            *crate::config::Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enabled_service && let Err(e) = crate::core::CoreManager::global().run_core().await {
            log::error!(target: "app", "{e}");
        }
        Ok(res?)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn restart_service() -> Result {
        let res = service::control::restart_service().await;
        let enabled_service = {
            *crate::config::Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enabled_service && let Err(e) = crate::core::CoreManager::global().run_core().await {
            log::error!(target: "app", "{e}");
        }
        Ok(res?)
    }
}

#[cfg(not(windows))]
pub mod uwp {
    use super::*;

    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool() -> Result {
        Ok(())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_service_install_prompt() -> Result<String> {
    let args = (crate::core::service::control::get_service_install_args().await)?
        .into_iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let mut prompt = format!("./nyanpasu-service {args}");
    if cfg!(not(windows)) {
        prompt = format!("sudo {prompt}");
    }
    Ok(prompt)
}

#[tauri::command]
#[specta::specta]
pub fn cleanup_processes(app_handle: AppHandle) -> Result {
    crate::utils::help::cleanup_processes(&app_handle);
    Ok(())
}

/// Namespace prefix for all frontend-visible KV entries.
/// Internal subsystems (e.g. task storage) use un-prefixed keys and are
/// never exposed to the frontend through these IPC commands.
const WEB_STORAGE_KEY_PREFIX: &str = "web:";

fn web_key(key: &str) -> String {
    format!("{WEB_STORAGE_KEY_PREFIX}{key}")
}

#[tauri::command]
#[specta::specta]
pub fn get_storage_item(app_handle: AppHandle, key: String) -> Result<Option<String>> {
    let storage = app_handle.state::<Storage>();
    let value = (storage.get_item(&web_key(&key)))?;
    Ok(value)
}

#[tauri::command]
#[specta::specta]
pub fn set_storage_item(app_handle: AppHandle, key: String, value: String) -> Result {
    let storage = app_handle.state::<Storage>();
    (storage.set_item(&web_key(&key), &value))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn remove_storage_item(app_handle: AppHandle, key: String) -> Result {
    let storage = app_handle.state::<Storage>();
    (storage.remove_item(&web_key(&key)))?;
    Ok(())
}

const HOTKEYS_KEY: &str = "hotkeys";

#[tauri::command]
#[specta::specta]
pub fn get_hotkeys(app_handle: AppHandle) -> Result<Option<Vec<String>>> {
    let storage = app_handle.state::<Storage>();
    let value = storage.get_item::<Vec<String>>(HOTKEYS_KEY)?;
    Ok(value)
}

#[tauri::command]
#[specta::specta]
pub fn set_hotkeys(app_handle: AppHandle, hotkeys: Vec<String>) -> Result {
    // Validate and register hotkeys first (may fail with error)
    (hotkey::Hotkey::global().update(hotkeys.clone()))?;
    // Only save to storage after validation succeeds
    let storage = app_handle.state::<Storage>();
    storage.set_item(HOTKEYS_KEY, &hotkeys)?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StorageEntry {
    pub key: String,
    /// Raw JSON-encoded value string.
    pub value: String,
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[tauri::command]
#[specta::specta]
pub fn get_all_storage_items(app_handle: AppHandle) -> Result<Vec<StorageEntry>> {
    let storage = app_handle.state::<Storage>();
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
#[tauri::command]
#[specta::specta]
pub fn clear_storage(app_handle: AppHandle) -> Result {
    let storage = app_handle.state::<Storage>();
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

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_connections_state(
    app_handle: AppHandle,
) -> Result<crate::core::clash::ws::ClashConnectionsConnectorState> {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    Ok(ws_connector.state())
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    app_handle: AppHandle,
) -> Result<crate::core::clash::ws::ClashWsSnapshot> {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    Ok(ws_connector.snapshot())
}

#[tauri::command]
#[specta::specta]
pub async fn set_clash_ws_recording(
    app_handle: AppHandle,
    kind: crate::core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<crate::core::clash::ws::ClashWsRecording> {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    Ok(ws_connector.set_recording(kind, enabled))
}

#[tauri::command]
#[specta::specta]
pub async fn clear_clash_ws_history(
    app_handle: AppHandle,
    kind: crate::core::clash::ws::ClashWsKind,
) -> Result {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    ws_connector.clear_history(kind);
    Ok(())
}

// Updater block

#[derive(Default, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
// TODO: a copied from updater metadata, and should be moved a separate updater module
pub struct UpdateWrapper {
    rid: tauri::ResourceId,
    available: bool,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    // TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value).
    #[specta(type = specta_typescript::Any)]
    raw_json: serde_json::Value,
}

#[tauri::command]
#[specta::specta]
pub async fn check_update(webview: tauri::Webview) -> Result<Option<UpdateWrapper>> {
    use crate::utils::config::{get_self_proxy, get_system_proxy};
    use std::cmp::Ordering;
    use tauri_plugin_updater::UpdaterExt;

    let build_time = time::OffsetDateTime::parse(
        crate::consts::BUILD_INFO.build_date,
        &time::format_description::well_known::Rfc3339,
    )
    .context("failed to parse build time")?;
    let mut builder = webview
        .updater_builder()
        .version_comparator(move |_, remote| {
            use semver::Version;
            let local = Version::parse(crate::consts::BUILD_INFO.pkg_version).ok();
            log::trace!("[check] local: {:?}, remote: {:?}", local, remote.version);
            match local {
                Some(local) => {
                    if !local.build.is_empty() && !remote.version.build.is_empty() {
                        // ignore build info to compare the version directly
                        match local.cmp_precedence(&remote.version) {
                            Ordering::Less => true,
                            Ordering::Equal => match remote.pub_date {
                                // prefer newer build if pub_date is available
                                Some(pub_date) => {
                                    local.build != remote.version.build && pub_date > build_time
                                }
                                None => local.build != remote.version.build,
                            },
                            Ordering::Greater => false,
                        }
                    } else {
                        local < remote.version
                    }
                }
                None => false,
            }
        });
    // apply proxy
    if let Ok(proxy) = get_self_proxy() {
        builder = builder.proxy(proxy.parse().context("failed to parse proxy")?);
    }
    if let Ok(Some(proxy)) = get_system_proxy() {
        builder = builder.proxy(proxy.parse().context("failed to parse system proxy")?);
    }
    let updater = builder.build().context("failed to build updater")?;
    let update = updater.check().await.context("failed to check update")?;
    Ok(update.map(|u| {
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
        wrapper.rid = webview.resources_table().add(u);
        wrapper
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn save_window_size_state(
    legacy: State<'_, LegacyVergeBridge>,
    app_handle: AppHandle,
    label: String,
) -> Result<()> {
    // Window-state save writes `Config::verge().window_size_state` directly; reseed typed
    // actors so a later pure patch does not revert the saved geometry.
    legacy
        .run_legacy_verge_mutation(|| async move {
            match label.as_str() {
                crate::consts::MAIN_WINDOW_LABEL => {
                    resolve::save_main_window_state(&app_handle, true)?;
                }
                _ => log::warn!("Unknown window label: {}", label),
            }
            Ok(())
        })
        .await?;
    Ok(())
}

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

#[tauri::command]
#[specta::specta]
pub fn copy_clash_env(app_handle: AppHandle, env_type: CopyEnvOption) {
    feat::copy_clash_env(&app_handle, &env_type);
}

#[tauri::command]
#[specta::specta]
pub fn quit_application(app_handle: AppHandle) {
    crate::utils::help::quit_application(&app_handle);
}

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

#[tauri::command]
#[specta::specta]
pub fn get_system_accent_color() -> Result<Option<String>> {
    Ok(crate::utils::color::get_system_accent_color())
}
