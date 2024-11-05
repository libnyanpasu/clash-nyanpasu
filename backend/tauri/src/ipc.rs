use crate::{
    config::*,
    core::{tasks::jobs::ProfilesJobGuard, updater::ManifestVersionLatest, *},
    enhance::PostProcessingOutput,
    feat,
    utils::{
        candy,
        collect::EnvInfo,
        dirs, help,
        resolve::{self, save_window_state},
    },
};
use anyhow::{anyhow, Context};
use chrono::Local;
use log::debug;
use nyanpasu_ipc::api::status::CoreState;
use profile::item_type::ProfileItemType;
use serde_yaml::Mapping;
use std::{borrow::Cow, collections::VecDeque, path::PathBuf, result::Result as StdResult};
use storage::{StorageOperationError, WebStorage};
use sysproxy::Sysproxy;
use tauri::AppHandle;
use tray::icon::TrayIcon;

use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
    #[error(transparent)]
    Storage(#[from] StorageOperationError),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error("{0}")]
    Custom(String),
}

impl From<String> for IpcError {
    fn from(s: String) -> Self {
        IpcError::Custom(s)
    }
}

impl serde::Serialize for IpcError {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(format!("{:#?}", self).as_str())
    }
}

type Result<T = ()> = StdResult<T, IpcError>;

#[tauri::command]
pub fn get_profiles() -> Result<Profiles> {
    Ok(Config::profiles().data().clone())
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn is_portable() -> Result<bool> {
    Ok(crate::utils::dirs::get_portable_flag())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn is_portable() -> Result<bool> {
    Ok(false)
}

#[tauri::command]
pub async fn enhance_profiles() -> Result {
    CoreManager::global().update_config().await?;
    handle::Handle::refresh_clash();
    Ok(())
}

#[tauri::command]
pub async fn import_profile(url: String, option: Option<RemoteProfileOptionsBuilder>) -> Result {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    let mut builder = crate::config::profile::item::RemoteProfileBuilder::default();
    builder.url(url);
    if let Some(option) = option {
        builder.option(option.clone());
    }
    let profile = builder
        .build_no_blocking()
        .await
        .context("failed to build a remote profile")?;
    // 根据是否为 Some(uid) 来判断是否要激活配置
    let profile_id = {
        if Config::profiles().draft().current.is_empty() {
            Some(profile.uid().to_string())
        } else {
            None
        }
    };
    {
        let committer = Config::profiles().auto_commit();
        (committer.draft().append_item(profile.into()))?;
    }
    // TODO: 使用 activate_profile 来激活配置
    if let Some(profile_id) = profile_id {
        let mut builder = ProfilesBuilder::default();
        builder.current(vec![profile_id]);
        patch_profiles_config(builder).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn create_profile(item: Mapping, file_data: Option<String>) -> Result {
    let kind = item
        .get("type")
        .and_then(|kind| serde_yaml::from_value::<ProfileItemType>(kind.clone()).ok())
        .ok_or(anyhow!("the type field is null"))?;
    let item = serde_yaml::Value::Mapping(item);
    tracing::trace!("create profile: {kind:?} with {item:?}");
    let profile: Profile = match kind {
        ProfileItemType::Local => {
            let item: LocalProfileBuilder = (serde_yaml::from_value(item))?;
            (item.build())
                .context("failed to build local profile")?
                .into()
        }
        ProfileItemType::Remote => {
            let mut item: RemoteProfileBuilder = (serde_yaml::from_value(item))?;
            (item.build_no_blocking().await)
                .context("failed to build remote profile")?
                .into()
        }
        ProfileItemType::Merge => {
            let item: MergeProfileBuilder = (serde_yaml::from_value(item))?;
            (item.build())
                .context("failed to build merge profile")?
                .into()
        }
        ProfileItemType::Script(_) => {
            let item: ScriptProfileBuilder = (serde_yaml::from_value(item))?;
            (item.build())
                .context("failed to build script profile")?
                .into()
        }
    };
    tracing::info!("created new profile: {:#?}", profile);
    if let Some(file_data) = file_data
        && !file_data.is_empty()
        && kind != ProfileItemType::Remote
    {
        (profile.save_file(file_data))?;
    }

    // 根据是否为 Some(uid) 来判断是否要激活配置
    let profile_id = {
        if (profile.is_local() || profile.is_remote())
            && Config::profiles().draft().current.is_empty()
        {
            Some(profile.uid().to_string())
        } else {
            None
        }
    };
    {
        let committer = Config::profiles().auto_commit();
        committer.draft().append_item(profile)?;
    };
    // TODO: 使用 activate_profile 来激活配置
    if let Some(profile_id) = profile_id {
        let mut builder = ProfilesBuilder::default();
        builder.current(vec![profile_id]);
        patch_profiles_config(builder).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn reorder_profile(active_id: String, over_id: String) -> Result {
    let committer = Config::profiles().auto_commit();
    (committer.draft().reorder(active_id, over_id))?;
    Ok(())
}

#[tauri::command]
pub fn reorder_profiles_by_list(list: Vec<String>) -> Result {
    let committer = Config::profiles().auto_commit();
    (committer.draft().reorder_by_list(&list))?;
    Ok(())
}

#[tauri::command]
pub async fn update_profile(uid: String, option: Option<RemoteProfileOptionsBuilder>) -> Result {
    (feat::update_profile(uid, option).await)?;
    Ok(())
}

#[tauri::command]
pub async fn delete_profile(uid: String) -> Result {
    let should_update = tokio::task::spawn_blocking(move || {
        #[allow(clippy::let_and_return)] // a bug in clippy
        nyanpasu_utils::runtime::block_on_current_thread(async move {
            let committer = Config::profiles().auto_commit();
            let x = committer.draft().delete_item(&uid).await;
            x
        })
    })
    .await
    .context("failed to join the task")?
    .context("failed to delete the profile")?;

    if should_update {
        (CoreManager::global().update_config().await)?;
        handle::Handle::refresh_clash();
    }
    Ok(())
}

/// 修改profiles的
#[tauri::command]
pub async fn patch_profiles_config(profiles: ProfilesBuilder) -> Result {
    Config::profiles().draft().apply(profiles);

    match CoreManager::global().update_config().await {
        Ok(_) => {
            handle::Handle::refresh_clash();
            Config::profiles().apply();
            (Config::profiles().data().save_file())?;
            Ok(())
        }
        Err(err) => {
            Config::profiles().discard();
            log::error!(target: "app", "{err:?}");
            Err(IpcError::from(err))
        }
    }
}

/// 修改某个profile item的
#[tauri::command]
pub async fn patch_profile(uid: String, profile: Mapping) -> Result {
    tracing::debug!("patch profile: {uid} with {profile:?}");
    {
        let committer = Config::profiles().auto_commit();
        (committer.draft().patch_item(uid.clone(), profile))?;
    }
    ProfilesJobGuard::global().lock().refresh();
    let need_update = {
        let profiles = Config::profiles();
        let profiles = profiles.latest();
        match (&profiles.chain, &profiles.current) {
            (chains, _) if chains.contains(&uid) => true,
            (_, current_chain) if current_chain.contains(&uid) => true,
            (_, current_chain) => {
                current_chain
                    .iter()
                    .any(|chain_uid| match profiles.get_item(chain_uid) {
                        Ok(item) if item.is_local() => {
                            item.as_local().unwrap().chain.contains(&uid)
                        }
                        Ok(item) if item.is_remote() => {
                            item.as_remote().unwrap().chain.contains(&uid)
                        }
                        _ => false,
                    })
            }
        }
    };
    if need_update {
        match CoreManager::global().update_config().await {
            Ok(_) => {
                handle::Handle::refresh_clash();
            }
            Err(err) => {
                log::error!(target: "app", "{err:?}");
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn view_profile(app_handle: tauri::AppHandle, uid: String) -> Result {
    let file = {
        Config::profiles()
            .latest()
            .get_item(&uid)?
            .file()
            .to_string()
    };

    let path = (dirs::app_profiles_dir())?.join(file);
    if !path.exists() {
        return Err(anyhow!("file not exists: {:#?}", path).into());
    }

    help::open_file(app_handle, path)?;
    Ok(())
}

#[tauri::command]
pub fn read_profile_file(uid: String) -> Result<String> {
    let profiles = Config::profiles();
    let profiles = profiles.latest();
    let item = (profiles.get_item(&uid))?;
    let data = match item.kind() {
        ProfileItemType::Local | ProfileItemType::Remote => {
            let raw = (item.read_file())?;
            let data = (serde_yaml::from_str::<Mapping>(&raw))?;
            (serde_yaml::to_string(&data).context("failed to convert yaml to string"))?
        }
        _ => (item.read_file())?,
    };
    Ok(data)
}

#[tauri::command]
pub fn save_profile_file(uid: String, file_data: Option<String>) -> Result {
    if file_data.is_none() {
        return Ok(());
    }

    let profiles = Config::profiles();
    let profiles = profiles.latest();
    let item = (profiles.get_item(&uid))?;
    (item.save_file(file_data.unwrap()))?;
    Ok(())
}

#[tauri::command]
pub fn get_clash_info() -> Result<ClashInfo> {
    Ok(Config::clash().latest().get_client_info())
}

#[tauri::command]
pub fn get_runtime_config() -> Result<Option<Mapping>> {
    Ok(Config::runtime().latest().config.clone())
}

#[tauri::command]
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
pub fn get_runtime_exists() -> Result<Vec<String>> {
    Ok(Config::runtime().latest().exists_keys.clone())
}

#[tauri::command]
pub fn get_postprocessing_output() -> Result<PostProcessingOutput> {
    Ok(Config::runtime().latest().postprocessing_output.clone())
}

#[tauri::command]
pub async fn get_core_status<'n>() -> Result<(Cow<'n, CoreState>, i64, RunType)> {
    Ok(CoreManager::global().status().await)
}

#[tauri::command]
pub async fn url_delay_test(url: &str, expected_status: u16) -> Result<Option<u64>> {
    Ok(crate::utils::net::url_delay_test(url, expected_status).await)
}

#[tauri::command]
pub async fn get_ipsb_asn() -> Result<Mapping> {
    Ok(crate::utils::net::get_ipsb_asn().await?)
}

#[tauri::command]
#[tracing_attributes::instrument]
pub async fn patch_clash_config(payload: Mapping) -> Result {
    tracing::debug!("patch_clash_config: {payload:?}");
    if RUNTIME_PATCHABLE_KEYS
        .iter()
        .any(|key| payload.contains_key(key))
    {
        (crate::core::clash::api::patch_configs(&payload).await)?;
    }
    if let Err(e) = feat::patch_clash(payload).await {
        tracing::error!("{e}");
        return Err(IpcError::from(e));
    }
    feat::update_proxies_buff(None);
    Ok(())
}

#[tauri::command]
pub fn get_verge_config() -> Result<IVerge> {
    Ok(Config::verge().data().clone())
}

#[tauri::command]
pub async fn patch_verge_config(payload: IVerge) -> Result {
    (feat::patch_verge(payload).await)?;
    Ok(())
}

#[tauri::command]
pub async fn change_clash_core(clash_core: Option<nyanpasu::ClashCore>) -> Result {
    (CoreManager::global().change_core(clash_core).await)?;
    Ok(())
}

/// restart the sidecar
#[tauri::command]
pub async fn restart_sidecar() -> Result {
    (CoreManager::global().run_core().await)?;
    Ok(())
}

/// get the system proxy
#[tauri::command]
pub fn get_sys_proxy() -> Result<Mapping> {
    let current = (Sysproxy::get_system_proxy()).context("failed to get system proxy")?;

    let mut map = Mapping::new();
    map.insert("enable".into(), current.enable.into());
    map.insert(
        "server".into(),
        format!("{}:{}", current.host, current.port).into(),
    );
    map.insert("bypass".into(), current.bypass.into());

    Ok(map)
}

#[tauri::command]
pub fn get_clash_logs() -> Result<VecDeque<String>> {
    Ok(logger::Logger::global().get_log())
}

#[tauri::command]
pub fn open_app_config_dir() -> Result<()> {
    let config_dir = (dirs::app_config_dir())?;
    (crate::utils::open::that(config_dir))?;
    Ok(())
}

#[tauri::command]
pub fn open_app_data_dir() -> Result<()> {
    let data_dir = (dirs::app_data_dir())?;
    (crate::utils::open::that(data_dir))?;
    Ok(())
}

#[tauri::command]
pub fn open_core_dir() -> Result<()> {
    let core_dir = (tauri::utils::platform::current_exe())?;
    let core_dir = core_dir
        .parent()
        .ok_or("failed to get core dir".to_string())?;
    (crate::utils::open::that(core_dir))?;
    Ok(())
}

#[tauri::command]
pub fn get_core_dir() -> Result<String> {
    let core_dir = (tauri::utils::platform::current_exe())?;
    let core_dir = core_dir
        .parent()
        .ok_or("failed to get core dir".to_string())?;
    Ok(core_dir.to_string_lossy().to_string())
}

#[tauri::command]
pub fn open_logs_dir() -> Result<()> {
    let log_dir = (dirs::app_logs_dir())?;
    (crate::utils::open::that(log_dir))?;
    Ok(())
}

#[tauri::command]
pub fn open_web_url(url: String) -> Result<()> {
    (crate::utils::open::that(url))?;
    Ok(())
}

#[tauri::command]
pub fn save_window_size_state() -> Result<()> {
    let handle = handle::Handle::global().app_handle.lock().clone().unwrap();
    (save_window_state(&handle, true))?;
    Ok(())
}

#[tauri::command]
pub async fn fetch_latest_core_versions() -> Result<ManifestVersionLatest> {
    let mut updater = updater::UpdaterManager::global().write().await; // It is intended to block here
    (updater.fetch_latest().await)?;
    Ok(updater.get_latest_versions())
}

#[tauri::command]
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
pub async fn collect_logs(app_handle: AppHandle) -> Result {
    let now = Local::now().format("%Y-%m-%d");
    let fname = format!("{}-log", now);
    let builder = FileDialogBuilder::new(app_handle.dialog().clone());
    builder
        .add_filter("archive files", &["zip"])
        .set_file_name(&fname)
        .set_title("Save log archive")
        .save_file(|file_path| match file_path {
            Some(path) if path.as_path().is_some() => {
                debug!("{:#?}", path);
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
pub async fn update_core(core_type: nyanpasu::ClashCore) -> Result<usize> {
    let event_id = (updater::UpdaterManager::global()
        .write()
        .await
        .update_core(&core_type)
        .await)?;
    Ok(event_id)
}

#[tauri::command]
pub async fn inspect_updater(updater_id: usize) -> Result<updater::UpdaterSummary> {
    let updater = (updater::UpdaterManager::global()
        .read()
        .await
        .inspect_updater(updater_id)
        .ok_or(anyhow::anyhow!("updater is not exist")))?;
    Ok(updater)
}

#[tauri::command]
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
pub async fn mutate_proxies() -> Result<crate::core::clash::proxies::Proxies> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    (ProxiesGuard::global().update().await)?;
    Ok(ProxiesGuard::global().read().inner().clone())
}

#[tauri::command]
pub async fn select_proxy(group: String, name: String) -> Result<()> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    (ProxiesGuard::global().select_proxy(&group, &name).await)?;
    Ok(())
}

#[tauri::command]
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
pub fn collect_envs<'a>() -> Result<EnvInfo<'a>> {
    Ok((crate::utils::collect::collect_envs())?)
}

#[tauri::command]
pub fn open_that(path: String) -> Result {
    (crate::utils::open::that(path))?;
    Ok(())
}

#[tauri::command]
pub fn is_appimage() -> Result<bool> {
    Ok(*crate::consts::IS_APPIMAGE)
}

#[cfg(windows)]
#[tauri::command]
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
pub fn get_custom_app_dir() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(windows)]
#[tauri::command]
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
pub fn restart_application(app_handle: tauri::AppHandle) -> Result {
    crate::utils::help::restart_application(&app_handle);
    Ok(())
}

#[tauri::command]
pub fn get_server_port() -> Result<u16> {
    Ok(*crate::server::SERVER_PORT)
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn set_custom_app_dir(_path: String) -> Result {
    Ok(())
}

#[cfg(windows)]
pub mod uwp {
    use super::Result;
    use crate::core::win_uwp;

    #[tauri::command]
    pub async fn invoke_uwp_tool() -> Result {
        (win_uwp::invoke_uwptools().await)?;
        Ok(())
    }
}

#[tauri::command]
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
pub async fn is_tray_icon_set(mode: TrayIcon) -> Result<bool> {
    let icon_path = (crate::utils::dirs::tray_icons_path(mode.as_str()))?;
    Ok(tokio::fs::metadata(icon_path).await.is_ok())
}

pub mod service {
    use super::Result;
    use crate::core::service;

    #[tauri::command]
    pub async fn status_service<'a>() -> Result<nyanpasu_ipc::types::StatusInfo<'a>> {
        let res = (service::control::status().await)?;
        Ok(res)
    }

    #[tauri::command]
    pub async fn install_service() -> Result {
        (service::control::install_service().await)?;
        Ok(())
    }

    #[tauri::command]
    pub async fn uninstall_service() -> Result {
        (service::control::uninstall_service().await)?;
        Ok(())
    }

    #[tauri::command]
    pub async fn start_service() -> Result {
        let res = service::control::start_service().await;
        let enabled_service = {
            *crate::config::Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enabled_service {
            if let Err(e) = crate::core::CoreManager::global().run_core().await {
                log::error!(target: "app", "{e}");
            }
        }
        Ok(res?)
    }

    #[tauri::command]
    pub async fn stop_service() -> Result {
        let res = service::control::stop_service().await;
        let enabled_service = {
            *crate::config::Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enabled_service {
            if let Err(e) = crate::core::CoreManager::global().run_core().await {
                log::error!(target: "app", "{e}");
            }
        }
        Ok(res?)
    }

    #[tauri::command]
    pub async fn restart_service() -> Result {
        let res = service::control::restart_service().await;
        let enabled_service = {
            *crate::config::Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enabled_service {
            if let Err(e) = crate::core::CoreManager::global().run_core().await {
                log::error!(target: "app", "{e}");
            }
        }
        Ok(res?)
    }
}

#[cfg(not(windows))]
pub mod uwp {
    use super::*;

    #[tauri::command]
    pub async fn invoke_uwp_tool() -> Result {
        Ok(())
    }
}

#[tauri::command]
pub async fn get_service_install_prompt() -> Result<String> {
    let args = (crate::core::service::control::get_service_install_args().await)?
        .into_iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let mut prompt = format!("./nyanpasu-service {}", args);
    if cfg!(not(windows)) {
        prompt = format!("sudo {}", prompt);
    }
    Ok(prompt)
}

#[tauri::command]
pub fn cleanup_processes(app_handle: AppHandle) -> Result {
    crate::utils::help::cleanup_processes(&app_handle);
    Ok(())
}

#[tauri::command]
pub fn get_storage_item(key: String) -> Result<Option<String>> {
    let value = (crate::core::storage::Storage::global().get_item(&key))?;
    Ok(value)
}

#[tauri::command]
pub fn set_storage_item(key: String, value: String) -> Result {
    (crate::core::storage::Storage::global().set_item(&key, &value))?;
    Ok(())
}

#[tauri::command]
pub fn remove_storage_item(key: String) -> Result {
    (crate::core::storage::Storage::global().remove_item(&key))?;
    Ok(())
}
