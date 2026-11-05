use crate::{
    core::*,
    utils::{help, proxy_env, proxy_env::CopyEnvOption},
    window::{
        WindowManager, kinds,
        kinds::{EditorWindow, EditorWindowType, MainWindow},
    },
};
use base64::{Engine, prelude::BASE64_STANDARD};
use chrono::Local;
use log::debug;
use nyanpasu_core::{
    client::NyanpasuClient,
    state::profiles::{InvalidSubscriptionUrlSnafu, ProfileFileMissingSnafu},
};
use nyanpasu_paths::PathResolver;
use nyanpasu_profile_transform::PostProcessingOutput;
use nyanpasu_rpc::{IpcError, commands::IconData};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, result::Result as StdResult};
use tauri::{AppHandle, State, Webview};
use tray::icon::TrayIcon;

use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{
    DialogExt, FileDialogBuilder, MessageDialogButtons as TauriDialogButtons,
    MessageDialogKind as TauriDialogKind, MessageDialogResult as TauriDialogResult,
};
use tauri_plugin_notification::NotificationExt;

type Result<T = ()> = StdResult<T, IpcError>;

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NativeDialogKind {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NativeDialogButtons {
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
    OkCustom {
        ok: String,
    },
    OkCancelCustom {
        ok: String,
        cancel: String,
    },
    YesNoCancelCustom {
        yes: String,
        no: String,
        cancel: String,
    },
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileDialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[nyanpasu_macro::rpc(capabilities(clipboard))]
#[tauri::command]
#[specta::specta]
pub async fn read_clipboard_text(app_handle: AppHandle) -> Result<String> {
    Ok(app_handle
        .clipboard()
        .read_text()
        .map_err(|error| error.to_string())?)
}

#[nyanpasu_macro::rpc(capabilities(clipboard))]
#[tauri::command]
#[specta::specta]
pub async fn write_clipboard_text(app_handle: AppHandle, text: String) -> Result {
    app_handle
        .clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(notification))]
#[tauri::command]
#[specta::specta]
pub async fn show_native_notification(
    app_handle: AppHandle,
    title: String,
    body: Option<String>,
) -> Result {
    let mut notification = app_handle.notification().builder().title(title);
    if let Some(body) = body {
        notification = notification.body(body);
    }
    notification.show().map_err(|error| error.to_string())?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(dialog))]
#[tauri::command]
#[specta::specta]
pub async fn show_native_message_dialog(
    app_handle: AppHandle,
    message: String,
    title: Option<String>,
    kind: NativeDialogKind,
    buttons: NativeDialogButtons,
) -> Result<String> {
    let kind = match kind {
        NativeDialogKind::Info => TauriDialogKind::Info,
        NativeDialogKind::Warning => TauriDialogKind::Warning,
        NativeDialogKind::Error => TauriDialogKind::Error,
    };
    let buttons = match buttons {
        NativeDialogButtons::Ok => TauriDialogButtons::Ok,
        NativeDialogButtons::OkCancel => TauriDialogButtons::OkCancel,
        NativeDialogButtons::YesNo => TauriDialogButtons::YesNo,
        NativeDialogButtons::YesNoCancel => TauriDialogButtons::YesNoCancel,
        NativeDialogButtons::OkCustom { ok } => TauriDialogButtons::OkCustom(ok),
        NativeDialogButtons::OkCancelCustom { ok, cancel } => {
            TauriDialogButtons::OkCancelCustom(ok, cancel)
        }
        NativeDialogButtons::YesNoCancelCustom { yes, no, cancel } => {
            TauriDialogButtons::YesNoCancelCustom(yes, no, cancel)
        }
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut dialog = app_handle
        .dialog()
        .message(message)
        .kind(kind)
        .buttons(buttons);
    if let Some(title) = title {
        dialog = dialog.title(title);
    }
    dialog.show_with_result(move |result| {
        let result = match result {
            TauriDialogResult::Yes => "Yes".to_owned(),
            TauriDialogResult::No => "No".to_owned(),
            TauriDialogResult::Ok => "Ok".to_owned(),
            TauriDialogResult::Cancel => "Cancel".to_owned(),
            TauriDialogResult::Custom(label) => label,
        };
        let _ = sender.send(result);
    });
    receiver
        .await
        .map_err(|error| error.to_string())
        .map_err(Into::into)
}

#[nyanpasu_macro::rpc(capabilities(dialog))]
#[tauri::command]
#[specta::specta]
pub async fn ask_native_dialog(
    app_handle: AppHandle,
    message: String,
    title: Option<String>,
    kind: NativeDialogKind,
) -> Result<bool> {
    let kind = match kind {
        NativeDialogKind::Info => TauriDialogKind::Info,
        NativeDialogKind::Warning => TauriDialogKind::Warning,
        NativeDialogKind::Error => TauriDialogKind::Error,
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut dialog = app_handle
        .dialog()
        .message(message)
        .kind(kind)
        .buttons(TauriDialogButtons::YesNo);
    if let Some(title) = title {
        dialog = dialog.title(title);
    }
    dialog.show(move |confirmed| {
        let _ = sender.send(confirmed);
    });
    receiver
        .await
        .map_err(|error| error.to_string())
        .map_err(Into::into)
}

#[nyanpasu_macro::rpc(capabilities(dialog))]
#[tauri::command]
#[specta::specta]
pub async fn open_native_file_dialog(
    app_handle: AppHandle,
    title: Option<String>,
    filters: Vec<FileDialogFilter>,
) -> Result<Option<String>> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut dialog = app_handle.dialog().file();
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    for filter in filters {
        let extensions = filter
            .extensions
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        dialog = dialog.add_filter(filter.name, &extensions);
    }
    dialog.pick_file(move |selected| {
        let path = selected
            .and_then(|selected| selected.into_path().ok())
            .map(|path| path.to_string_lossy().into_owned());
        let _ = sender.send(path);
    });
    receiver
        .await
        .map_err(|error| error.to_string())
        .map_err(Into::into)
}

// ---- profiles domain commands (PR-3 T08, thin adapters over NyanpasuClient) ----

use nyanpasu_config::profile::{
    FolderId, ProfileId, ProfileLayout, ProfileMetadataPatch, ProxyGroupLayout,
    RemoteProfileOptionsPatch, TransformKind,
};

// #[tauri::command]
// #[specta::specta]
// pub fn get_device_info() -> Result<nyanpasu_core::utils::hwid::DeviceInfo> {
//     Ok(nyanpasu_core::utils::hwid::get_device_info())
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

/// Downloads a profile URL and suggests how to import it, for the user to
/// confirm. Desktop-only: it sends a request to any URL the caller names.
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn probe_profile_url(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<nyanpasu_core::client::profiles::ProfileUrlProbe> {
    let url = snafu::ResultExt::context(
        url::Url::parse(&url),
        InvalidSubscriptionUrlSnafu { url: &url },
    )?;
    Ok(client.probe_profile_url(url, option).await?)
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
    folder: Option<FolderId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<ProfileId>> {
    let url = snafu::ResultExt::context(
        url::Url::parse(&url),
        InvalidSubscriptionUrlSnafu { url: &url },
    )?;
    // `name` carries deep-link intent (e.g. an install-config `name=` param);
    // when absent the facade derives the name from the url server-side. Return
    // MutationOutcome so a degraded post-import rebuild still carries the uid.
    Ok(client
        .import_profile(url, name, option, transform, folder)
        .await?)
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
#[nyanpasu_macro::rpc(capabilities(deep_link))]
#[tauri::command]
#[specta::specta]
pub async fn take_pending_deep_links(pending: State<'_, PendingDeepLinks>) -> Result<Vec<String>> {
    Ok(pending.take_all())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn reorder_profile(
    client: State<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.reorder_profile(active_id, over_id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: State<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.reorder_profiles_by_list(list).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_profile_layout(
    client: State<'_, NyanpasuClient>,
    layout: ProfileLayout,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_profile_layout(layout).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_proxy_group_layout(
    client: State<'_, NyanpasuClient>,
    profile: ProfileId,
    layout: ProxyGroupLayout,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_proxy_group_layout(profile, layout).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn create_proxy_group_folder(
    client: State<'_, NyanpasuClient>,
    profile: ProfileId,
    name: String,
    members: Vec<String>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<FolderId>> {
    Ok(client
        .create_proxy_group_folder(profile, name, members)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn create_profile_folder(
    client: State<'_, NyanpasuClient>,
    name: String,
    members: Vec<ProfileId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<FolderId>> {
    Ok(client.create_profile_folder(name, members).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_status(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::jobs::ProfileSyncStatus> {
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
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.refresh_profile(uid, option).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: State<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_profile_valid_fields(fields).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_profile_metadata(uid, patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_remote_profile_options(uid, patch).await?)
}

#[nyanpasu_macro::rpc(capabilities(opener))]
#[tauri::command]
#[specta::specta]
pub async fn view_profile(client: State<'_, NyanpasuClient>, uid: ProfileId) -> Result {
    let path = client.get_profile_materialized_path(uid.clone()).await?;
    if !path.exists() {
        return Err(ProfileFileMissingSnafu { uid, path: &path }.build().into());
    }
    help::open_file(&path)?;
    Ok(())
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
pub async fn inspect_applied_runtime(
    client: State<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_applied_runtime().await)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn inspect_runtime_node(
    client: State<'_, NyanpasuClient>,
    snapshot_id: String,
    node_id: u32,
) -> Result<nyanpasu_core::client::runtime_inspection::RuntimeInspectionContent> {
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

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn url_delay_test(
    client: State<'_, NyanpasuClient>,
    url: String,
    expected_status: u16,
) -> Result<Option<u64>> {
    Ok(client.url_delay_test(&url, expected_status).await)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion.
pub async fn get_ipsb_asn(
    client: State<'_, NyanpasuClient>,
) -> Result<specta_typescript::Any<serde_json::Value>> {
    let value = client.get_ipsb_asn().await?;
    let wrapped: specta_typescript::Any<serde_json::Value> = serde_json::from_value(value)?;
    Ok(wrapped)
}

// ---- typed configuration commands (thin adapters over NyanpasuClient) ----

use nyanpasu_config::application::{ClashCore, ReleaseChannel};

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    nyanpasu_core::client::hotkey::ports::HotkeyAction::all()
        .iter()
        .map(|action| action.as_str())
        .collect()
}

#[nyanpasu_macro::rpc(capabilities(elevation))]
#[tauri::command]
#[specta::specta]
pub async fn flush_system_dns_cache(client: State<'_, NyanpasuClient>) -> Result {
    client.flush_system_dns_cache().await?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(opener))]
#[tauri::command]
#[specta::specta]
pub fn open_app_config_dir(paths: State<'_, PathResolver>) -> Result<()> {
    (crate::utils::open::that(paths.app_config_dir()))?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(opener))]
#[tauri::command]
#[specta::specta]
pub fn open_app_data_dir(paths: State<'_, PathResolver>) -> Result<()> {
    (crate::utils::open::that(paths.app_data_dir()))?;
    Ok(())
}

#[derive(specta::Type, serde::Serialize)]
pub struct ConfigBackupInfo {
    pub name: String,
    pub path: String,
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn create_config_backup(client: State<'_, NyanpasuClient>) -> Result<ConfigBackupInfo> {
    let backup = client.create_config_backup().await?;
    Ok(ConfigBackupInfo {
        name: backup.name,
        path: backup.path.to_string_lossy().into_owned(),
    })
}

#[nyanpasu_macro::rpc(capabilities(opener))]
#[tauri::command]
#[specta::specta]
pub fn open_backups_dir(client: State<'_, NyanpasuClient>) -> Result<()> {
    (crate::utils::open::that(client.backups_dir()?))?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(opener))]
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

#[nyanpasu_macro::rpc(capabilities(opener))]
#[tauri::command]
#[specta::specta]
pub fn open_logs_dir(paths: State<'_, PathResolver>) -> Result<()> {
    (crate::utils::open::that(paths.app_logs_dir()))?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(opener))]
#[tauri::command]
#[specta::specta]
pub fn open_web_url(url: String) -> Result<()> {
    (crate::utils::open::that(url))?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(dialog))]
#[tauri::command]
#[specta::specta]
pub async fn collect_logs(
    app_handle: AppHandle,
    paths: State<'_, PathResolver>,
    client: State<'_, NyanpasuClient>,
) -> Result {
    let archive = crate::utils::collect::logs_archive((*paths).clone(), (*client).clone()).await?;
    let now = Local::now().format("%Y-%m-%d");
    let fname = format!("{now}-log");
    let builder = FileDialogBuilder::new(app_handle.dialog().clone());
    builder
        .add_filter("archive files", &["zip"])
        .set_file_name(&fname)
        .set_title("Save log archive")
        .save_file(move |file_path| match file_path {
            Some(path) if path.as_path().is_some() => {
                debug!("{path:#?}");
                match std::fs::copy(&archive, path.as_path().unwrap()) {
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

#[nyanpasu_macro::rpc(http, capabilities(elevation))]
#[tauri::command]
#[specta::specta]
pub async fn update_core(client: State<'_, NyanpasuClient>, core_type: ClashCore) -> Result<usize> {
    Ok(client.download_core_update(core_type).await?)
}

#[nyanpasu_macro::rpc(capabilities(opener))]
#[tauri::command]
#[specta::specta]
pub fn report_issue(
    paths: State<'_, PathResolver>,
    client: State<'_, NyanpasuClient>,
    page: nyanpasu_diagnose::report::PageCapabilities,
) -> Result {
    crate::utils::collect::report_issue((*paths).clone(), (*client).clone(), page)?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(opener))]
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
    match nyanpasu_paths::registry::custom_config_dir(crate::host_paths::APP_NAME) {
        Ok(Some(path)) => Ok(Some(path.into_string())),
        Ok(None) => Ok(None),
        Err(err) => Err(IpcError::from(anyhow::Error::from(err))),
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
#[nyanpasu_macro::rpc(capabilities(dialog, elevation))]
#[tauri::command]
#[specta::specta]
pub async fn set_custom_app_dir(app_handle: tauri::AppHandle, path: String) -> Result {
    use crate::utils::{self, dialog::migrate_dialog};
    use rust_i18n::t;

    let path_str = path.clone();

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
                nyanpasu_paths::registry::set_custom_config_dir(
                    crate::host_paths::APP_NAME,
                    camino::Utf8Path::new(&path_str),
                )?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await;
    res.map_err(IpcError::unknown)??;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(lifecycle))]
#[tauri::command]
#[specta::specta]
pub fn restart_application(app_handle: tauri::AppHandle) -> Result {
    crate::utils::help::restart_application(&app_handle);
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_tray_icon(paths: State<'_, PathResolver>, mode: TrayIcon) -> Result<IconData> {
    let bytes = crate::core::tray::icon::get_raw_icon(paths, mode);
    Ok(IconData {
        data_url: format!("data:image/png;base64,{}", BASE64_STANDARD.encode(bytes)),
    })
}

#[cfg(not(windows))]
#[nyanpasu_macro::rpc(capabilities(dialog, elevation))]
#[tauri::command]
#[specta::specta]
pub async fn set_custom_app_dir(_path: String) -> Result {
    Ok(())
}

#[cfg(windows)]
pub mod uwp {
    use super::{NyanpasuClient, Result, State};
    use crate::core::win_uwp;

    #[nyanpasu_macro::rpc(capabilities(elevation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool(
        app_handle: tauri::AppHandle,
        client: State<'_, NyanpasuClient>,
    ) -> Result {
        let resources_dir = (crate::utils::init::bundled_resources_dir(&app_handle))
            .map_err(anyhow::Error::from)?;
        let language = client.get_app_config().await?.language;
        (win_uwp::invoke_uwptools(&resources_dir, language).await)?;
        Ok(())
    }
}

#[nyanpasu_macro::rpc(capabilities(presentation))]
#[tauri::command]
#[specta::specta]
pub async fn set_tray_icon(
    app_handle: tauri::AppHandle,
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
    mode: TrayIcon,
    path: Option<PathBuf>,
) -> Result {
    (crate::core::tray::icon::set_icon(paths, mode, path))?;
    // Checked here, so a bad icon reaches the caller; only applying it to the
    // tray is queued.
    (crate::core::tray::icon::check_icon(&crate::core::tray::icon::get_icon(paths, &mode)))?;
    crate::core::tray::Tray::request(&app_handle, crate::core::tray::TrayWork::PART)
        .map_err(IpcError::unknown)?;
    // The widget logo may show the tray's icons.
    client.request_widget_logo_refresh()?;
    Ok(())
}

#[nyanpasu_macro::rpc(http, capabilities(presentation))]
#[tauri::command]
#[specta::specta]
pub async fn set_tray_icon_from_bytes(
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
    mode: TrayIcon,
    bytes_base64: String,
) -> Result {
    let bytes = decode_icon_upload(&bytes_base64, "Tray")?;
    crate::core::tray::icon::set_icon_from_bytes(paths, mode, &bytes)?;
    crate::core::tray::icon::check_icon(&crate::core::tray::icon::get_icon(paths, &mode))?;
    client.request_tray_refresh()?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn is_tray_icon_set(paths: State<'_, PathResolver>, mode: TrayIcon) -> Result<bool> {
    let icon_path = crate::core::tray::icon::tray_icons_path(paths, mode.as_str());
    Ok(tokio::fs::metadata(icon_path).await.is_ok())
}

/// Decodes an uploaded icon of at most 1 MiB. `kind` names the icon in the
/// error.
fn decode_icon_upload(bytes_base64: &str, kind: &str) -> Result<Vec<u8>> {
    const MAX_ICON_BYTES: usize = 1024 * 1024;
    let too_large = || Err(format!("{kind} icon image exceeds the 1 MiB limit").into());
    if bytes_base64.len() > MAX_ICON_BYTES.div_ceil(3) * 4 {
        return too_large();
    }
    let bytes = BASE64_STANDARD
        .decode(bytes_base64)
        .map_err(anyhow::Error::from)?;
    if bytes.len() > MAX_ICON_BYTES {
        return too_large();
    }
    Ok(bytes)
}

/// The statistic widget's own icon for `mode`, or the bundled one.
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_widget_icon(paths: State<'_, PathResolver>, mode: TrayIcon) -> Result<IconData> {
    let bytes = crate::core::widget_logo::get_raw_icon(paths, mode);
    Ok(IconData {
        data_url: format!("data:image/png;base64,{}", BASE64_STANDARD.encode(bytes)),
    })
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn is_widget_icon_set(paths: State<'_, PathResolver>, mode: TrayIcon) -> Result<bool> {
    Ok(crate::core::widget_logo::is_icon_set(paths, mode))
}

/// Sets the statistic widget's own icon for `mode` from a file, or resets it
/// to the bundled one when `path` is `None`.
#[nyanpasu_macro::rpc(capabilities(presentation))]
#[tauri::command]
#[specta::specta]
pub async fn set_widget_icon(
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
    mode: TrayIcon,
    path: Option<PathBuf>,
) -> Result {
    crate::core::widget_logo::set_icon(paths, mode, path)?;
    client.request_widget_logo_refresh()?;
    Ok(())
}

#[nyanpasu_macro::rpc(http, capabilities(presentation))]
#[tauri::command]
#[specta::specta]
pub async fn set_widget_icon_from_bytes(
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
    mode: TrayIcon,
    bytes_base64: String,
) -> Result {
    let bytes = decode_icon_upload(&bytes_base64, "Widget")?;
    crate::core::widget_logo::set_icon_from_bytes(paths, mode, &bytes)?;
    client.request_widget_logo_refresh()?;
    Ok(())
}

pub mod service {
    use super::{NyanpasuClient, Result};
    use tauri::State;

    #[nyanpasu_macro::rpc(capabilities(elevation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn install_service(client: State<'_, NyanpasuClient>) -> Result {
        client.install_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn uninstall_service(client: State<'_, NyanpasuClient>) -> Result {
        client.uninstall_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn start_service(client: State<'_, NyanpasuClient>) -> Result {
        client.start_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn stop_service(client: State<'_, NyanpasuClient>) -> Result {
        client.stop_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation))]
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

    #[nyanpasu_macro::rpc(capabilities(elevation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool() -> Result {
        Ok(())
    }
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_service_install_prompt(paths: State<'_, PathResolver>) -> Result<String> {
    use nyanpasu_core::service::control::{ResolveServiceDirsSnafu, get_service_install_args};
    use snafu::ResultExt;

    let args = async {
        let app_dir = paths.app_install_dir().context(ResolveServiceDirsSnafu)?;
        get_service_install_args(
            paths.app_data_dir().as_std_path(),
            paths.app_config_dir().as_std_path(),
            app_dir.as_std_path(),
        )
        .await
    }
    .await
    .context(nyanpasu_core::client::runtime_error::PrepareServiceInstallPromptSnafu)?
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

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_hotkeys(client: State<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client.get_app_config().await?.hotkeys)
}

#[nyanpasu_macro::rpc(http, capabilities(hotkey))]
#[tauri::command]
#[specta::specta]
pub async fn set_hotkeys(
    client: State<'_, NyanpasuClient>,
    hotkeys: Vec<String>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    // An unparsable list is rejected before anything is written; a shortcut the
    // OS refuses lands as a degradation on a committed config.
    Ok(client
        .patch_app_config(nyanpasu_config::application::NyanpasuAppConfigPatch {
            hotkeys: Some(hotkeys),
            ..Default::default()
        })
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    client: tauri::State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::ws::ClashWsSnapshot> {
    Ok(client.clash_ws_snapshot().await?)
}

#[nyanpasu_macro::rpc(http, result)]
#[tauri::command]
#[specta::specta]
pub async fn get_core_log(
    client: tauri::State<'_, NyanpasuClient>,
    cursor: nyanpasu_core::logs::CoreLogCursor,
) -> nyanpasu_core::logs::CoreLogResult<nyanpasu_core::logs::CoreLogRecord> {
    client.get_core_log(cursor).await
}

#[nyanpasu_macro::rpc(http, result)]
#[tauri::command]
#[specta::specta]
pub async fn clear_core_logs(
    client: tauri::State<'_, NyanpasuClient>,
) -> nyanpasu_core::logs::CoreLogResult<()> {
    client.clear_core_logs().await
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
    metric: nyanpasu_traffic::Metric,
    after: Option<nyanpasu_traffic::UsageCursor>,
    limit: usize,
) -> Result<nyanpasu_traffic::UsagePage> {
    Ok(client
        .query_traffic_usage(query, group_by, metric, after, limit)
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
    range: nyanpasu_traffic::TrafficRange,
    filters: Vec<nyanpasu_traffic::TrafficFilter>,
    before: Option<nyanpasu_traffic::ClosedCursor>,
    limit: usize,
) -> Result<nyanpasu_traffic::ClosedPage> {
    Ok(client
        .query_traffic_closed_connections(range, filters, before, limit)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn query_traffic_active_connection_ids(
    client: tauri::State<'_, NyanpasuClient>,
    filters: Vec<nyanpasu_traffic::TrafficFilter>,
) -> Result<Vec<String>> {
    Ok(client.query_traffic_active_connection_ids(filters).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn set_clash_ws_recording(
    client: tauri::State<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<nyanpasu_core::clash::ws::ClashWsRecording> {
    Ok(client.set_clash_ws_recording(kind, enabled).await?)
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clear_clash_ws_history(
    client: tauri::State<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
) -> Result {
    client.clear_clash_ws_history(kind).await?;
    Ok(())
}

// Subscription control uses UnifiedRpc; only frame delivery is a native Channel.
// HTTP consumers use the dedicated SSE adapter instead of a webview channel.
#[nyanpasu_macro::rpc(owner, capabilities(caller_webview))]
#[tauri::command]
#[specta::specta]
pub async fn subscribe_clash_connection_details(
    window: tauri::Window,
    client: tauri::State<'_, NyanpasuClient>,
    subscriptions: tauri::State<
        '_,
        crate::adapters::connection_details::ConnectionDetailSubscriptions,
    >,
    on_frame: tauri::ipc::Channel<std::sync::Arc<nyanpasu_core::clash::ws::ClashConnectionDetails>>,
) -> Result<crate::adapters::connection_details::SubscriptionId> {
    let receiver = client.subscribe_clash_connection_details().await?;
    let parent = client.shutdown_child_token();
    let (id, cancel) = subscriptions.register(&parent, window.label().to_string());
    tokio::spawn(client.track_until_shutdown(
        &cancel,
        crate::adapters::connection_details::forward_details(receiver, on_frame),
    ));
    Ok(id)
}

#[nyanpasu_macro::rpc(owner, capabilities(caller_webview))]
#[tauri::command]
#[specta::specta]
pub fn unsubscribe_clash_connection_details(
    window: tauri::Window,
    subscriptions: tauri::State<
        '_,
        crate::adapters::connection_details::ConnectionDetailSubscriptions,
    >,
    id: crate::adapters::connection_details::SubscriptionId,
) -> Result {
    subscriptions.unsubscribe(id, window.label())?;
    Ok(())
}

// Updater block

#[nyanpasu_macro::rpc(http, result)]
#[tauri::command]
#[specta::specta]
pub async fn list_log_files(
    client: tauri::State<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
) -> nyanpasu_logging::LogResult<Vec<nyanpasu_logging::LogFileInfo>> {
    client.list_log_files(source).await
}
#[nyanpasu_macro::rpc(http, result, owner)]
#[tauri::command]
#[specta::specta]
pub async fn open_log_session(
    window: tauri::Window,
    client: tauri::State<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
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
    source: nyanpasu_core::client::logs::LogSource,
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
    source: nyanpasu_core::client::logs::LogSource,
    session: String,
) -> nyanpasu_logging::LogResult<()> {
    client
        .close_log_session(source, window.label().to_string(), session)
        .await
}
#[nyanpasu_macro::rpc(http, owner)]
#[tauri::command]
#[specta::specta]
pub fn report_frontend_events(
    window: tauri::Window,
    client: tauri::State<'_, NyanpasuClient>,
    batch: nyanpasu_core::client::frontend_events::FrontendEventBatch,
) -> Result {
    client.report_frontend_events(window.label(), batch);
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ReleaseChannelInfo {
    /// The feed update checks follow.
    current: ReleaseChannel,
    /// The channel of the installed build; a nightly build cannot leave Nightly.
    installed: ReleaseChannel,
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn get_release_channel(client: State<'_, NyanpasuClient>) -> Result<ReleaseChannelInfo> {
    Ok(ReleaseChannelInfo {
        current: client.release_channel().await?,
        installed: client.installed_release_channel(),
    })
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn set_release_channel(
    client: State<'_, NyanpasuClient>,
    channel: ReleaseChannel,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_release_channel(channel).await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn get_app_update_state(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.get_app_update_state().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn get_app_update_changelog(
    client: State<'_, NyanpasuClient>,
    from: String,
    to: String,
) -> Result<nyanpasu_core::client::app_update::changelog::AppUpdateChangelog> {
    Ok(client.get_app_update_changelog(&from, &to).await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn check_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.check_app_update().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn download_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.download_app_update().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn cancel_app_update_download(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.cancel_app_update_download().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn install_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.install_app_update().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn discard_app_update_package(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.discard_app_update_package().await?)
}

#[nyanpasu_macro::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn save_window_size_state(app_handle: AppHandle, label: String) -> Result<()> {
    if label == crate::consts::MAIN_WINDOW_LABEL {
        kinds::save_main_window_state_async(&app_handle, true).await?;
    }
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn create_main_window(windows: State<'_, WindowManager>) -> Result<()> {
    windows.open(&MainWindow, None)?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn create_debug_tray_menu_window(app_handle: AppHandle) -> Result<()> {
    kinds::create_debug_tray_menu_window(&app_handle)?;
    Ok(())
}

/// Panics on purpose, so the crash reporter can be tested. The panic hook
/// exits differently when the main thread panics, hence the choice.
#[nyanpasu_macro::rpc(capabilities(lifecycle))]
#[tauri::command]
#[specta::specta]
pub async fn simulate_crash(app_handle: AppHandle, main_thread: bool) -> Result<()> {
    if main_thread {
        app_handle
            .run_on_main_thread(|| panic!("simulated crash on the main thread"))
            .map_err(IpcError::unknown)?;
    } else {
        std::thread::spawn(|| panic!("simulated crash on a background thread"));
    }
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(clipboard))]
#[tauri::command]
#[specta::specta]
pub fn copy_clash_env(
    app_handle: AppHandle,
    client: State<'_, NyanpasuClient>,
    env_type: CopyEnvOption,
) {
    proxy_env::copy_clash_env(&app_handle, client.clash_info().port, &env_type);
}

#[nyanpasu_macro::rpc(capabilities(lifecycle))]
#[tauri::command]
#[specta::specta]
pub fn quit_application(app_handle: AppHandle) {
    crate::utils::help::quit_application(&app_handle);
}

#[nyanpasu_macro::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn create_editor_window(
    windows: State<'_, WindowManager>,
    window_type: EditorWindowType,
    uid: Option<String>,
) -> Result<()> {
    let (window, params) = EditorWindow::for_type(window_type, uid.as_deref())?;
    windows.open(&window, params)?;
    Ok(())
}

/// The calling window has rendered. Its label comes from the invoking webview,
/// so a window can only report itself.
#[nyanpasu_macro::rpc(capabilities(caller_webview))]
#[tauri::command]
#[specta::specta]
pub async fn report_window_ready(
    windows: State<'_, WindowManager>,
    webview: Webview,
) -> Result<()> {
    windows.report_ready(webview.label());
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub fn get_system_accent_color() -> Result<Option<String>> {
    Ok(crate::utils::color::get_system_accent_color())
}

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ConfigurationStatusChanged(
    pub nyanpasu_core::client::configuration_status::ConfigurationStatus,
);

#[cfg(test)]
mod tests {
    use super::PendingDeepLinks;

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
) -> Result<nyanpasu_core::client::debug_http::DebugHttpStatus> {
    Ok(client.debug_http_status().await?)
}
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn set_debug_http_enabled(
    client: State<'_, NyanpasuClient>,
    enabled: bool,
) -> Result<nyanpasu_core::client::debug_http::DebugHttpStatus> {
    Ok(client.set_debug_http_enabled(enabled).await?)
}
