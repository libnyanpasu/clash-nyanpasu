use crate::{
    utils::{help, proxy_env, proxy_env::CopyEnvOption},
    window::{
        WindowManager, kinds,
        kinds::{EditorWindow, EditorWindowType, MainWindow},
    },
};
use chrono::Local;
use log::debug;
use nyanpasu_contracts::net::StunServerTest;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc::error::{IpcError, Result};
use nyanpasu_rpc_protocol::values::TrayIcon;
use nyanpasu_runtime_values::profiles::{InvalidSubscriptionUrlSnafu, ProfileFileMissingSnafu};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, State, Webview};

use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{
    DialogExt, FileDialogBuilder, MessageDialogButtons as TauriDialogButtons,
    MessageDialogKind as TauriDialogKind, MessageDialogResult as TauriDialogResult,
};
use tauri_plugin_notification::NotificationExt;

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

use nyanpasu_config::profile::{ProfileId, RemoteProfileOptionsPatch};

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

/// Desktop-only: the host is whatever the caller types, like a URL probe.
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn test_stun_server(
    client: State<'_, NyanpasuClient>,
    host: String,
    port: u16,
) -> Result<StunServerTest> {
    Ok(client.test_stun_server(&host, port).await)
}

// ---- typed configuration commands (thin adapters over NyanpasuClient) ----

use nyanpasu_config::application::{ClashCore, ReleaseChannel};

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

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn get_core_version(
    client: State<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<String> {
    Ok(client.get_core_version(core_type).await?)
}

#[nyanpasu_macro::rpc(capabilities(dialog))]
#[tauri::command]
#[specta::specta]
pub async fn collect_logs(
    app_handle: AppHandle,
    paths: State<'_, PathResolver>,
    client: State<'_, NyanpasuClient>,
) -> Result {
    let archive =
        nyanpasu_rpc::commands::archive::logs_archive((*paths).clone(), (*client).clone()).await?;
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

#[nyanpasu_macro::rpc(optional_capabilities(elevation, presentation))]
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
#[nyanpasu_macro::rpc(capabilities(dialog, lifecycle, elevation))]
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
    (res.map_err(IpcError::unknown)?)?;
    Ok(())
}

#[nyanpasu_macro::rpc(capabilities(lifecycle))]
#[tauri::command]
#[specta::specta]
pub fn restart_application(app_handle: tauri::AppHandle) -> Result {
    crate::utils::help::restart_application(&app_handle);
    Ok(())
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

pub mod service {
    use super::{NyanpasuClient, Result};
    use tauri::State;

    #[nyanpasu_macro::rpc(capabilities(elevation), optional_capabilities(presentation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn install_service(client: State<'_, NyanpasuClient>) -> Result {
        client.install_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation), optional_capabilities(presentation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn uninstall_service(client: State<'_, NyanpasuClient>) -> Result {
        client.uninstall_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation), optional_capabilities(presentation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn start_service(client: State<'_, NyanpasuClient>) -> Result {
        client.start_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation), optional_capabilities(presentation))]
    #[tauri::command]
    #[specta::specta]
    pub async fn stop_service(client: State<'_, NyanpasuClient>) -> Result {
        client.stop_service().await?;
        Ok(())
    }

    #[nyanpasu_macro::rpc(capabilities(elevation), optional_capabilities(presentation))]
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

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ReleaseChannelInfo {
    /// The feed update checks follow.
    current: ReleaseChannel,
    /// The channel of the installed build; a nightly build cannot leave Nightly.
    installed: ReleaseChannel,
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn get_release_channel(client: State<'_, NyanpasuClient>) -> Result<ReleaseChannelInfo> {
    Ok(ReleaseChannelInfo {
        current: client.release_channel().await?,
        installed: client.installed_release_channel(),
    })
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn set_release_channel(
    client: State<'_, NyanpasuClient>,
    channel: ReleaseChannel,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.set_release_channel(channel).await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn get_app_update_state(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSnapshot> {
    Ok(client.get_app_update_state().await?)
}

#[nyanpasu_macro::rpc]
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
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSnapshot> {
    Ok(client.check_app_update().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn download_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSnapshot> {
    Ok(client.download_app_update().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn cancel_app_update_download(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSnapshot> {
    Ok(client.cancel_app_update_download().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn install_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSnapshot> {
    Ok(client.install_app_update().await?)
}

#[nyanpasu_macro::rpc(capabilities(app_update))]
#[tauri::command]
#[specta::specta]
pub async fn discard_app_update_package(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSnapshot> {
    Ok(client.discard_app_update_package().await?)
}

#[nyanpasu_macro::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn save_window_size_state(
    windows: State<'_, WindowManager>,
    label: String,
) -> Result<()> {
    windows.save_window_geometry(&label).await?;
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

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ConfigurationStatusChanged(
    pub nyanpasu_runtime_values::configuration::ConfigurationStatus,
);

#[cfg(test)]
mod tests {
    use super::{IpcError, PendingDeepLinks};
    use nyanpasu_config::profile::{ManagedProfilePath, ProfileId};
    use nyanpasu_contracts::system_dns::SystemDnsError;
    use nyanpasu_core::{
        client::{ClientError, debug_http::DebugHttpControlError},
        state::config_error::ConfigError,
    };
    use nyanpasu_runtime_values::profiles::ProfilesError;
    use nyanpasu_state_engine::ReplaceIfVersionError;
    use serde_json::json;
    use snafu::IntoError;

    use nyanpasu_contracts::{
        profiles::error::{ProfileFileError, SubscriptionFetchError},
        system_proxy::OsProxyError,
    };
    use nyanpasu_runtime_values::{
        effects::EffectsError,
        mutation::{CommitAborted, RuntimeAftermath, WriteConfigSnafu},
        runtime_error::RuntimeError,
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
        let failure = nyanpasu_runtime_values::runtime_error::ApplyRuntimeSnafu.into_error(
            nyanpasu_runtime_values::runtime_error::CoreFailure {
                kind: Some(nyanpasu_core_manager::CoreErrorKind::ApplyFailed),
                message: "the core kept the previous configuration".into(),
                retryable: false,
                operation_id: None,
            },
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
            serde_json::to_value(IpcError::from(RuntimeError::Isolated)).unwrap()["kind"],
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
            wire(ConfigError::ValidateHotkeyCapability)["kind"],
            json!({ "domain": "config", "error": { "kind": "validate_hotkey_capability" } })
        );
        for (error, kind) in [
            (
                DebugHttpControlError::ReadStatusUnsupported,
                "read_status_unsupported",
            ),
            (
                DebugHttpControlError::ChangeEnabledUnsupported,
                "change_enabled_unsupported",
            ),
        ] {
            assert_eq!(
                serde_json::to_value(IpcError::from(error)).unwrap()["kind"],
                json!({ "domain": "debug_http", "error": { "kind": kind } })
            );
        }
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
        for (error, kind) in [
            (
                DebugHttpControlError::ReadStatus {
                    source: anyhow::anyhow!("read cause"),
                },
                "read_status",
            ),
            (
                DebugHttpControlError::ChangeEnabled {
                    source: anyhow::anyhow!("change cause"),
                },
                "change_enabled",
            ),
        ] {
            let wire = serde_json::to_value(IpcError::from(error)).unwrap();
            assert_eq!(
                wire["kind"],
                json!({ "domain": "debug_http", "error": { "kind": kind } })
            );
            assert!(wire["detail"].as_str().unwrap().contains("cause"));
        }
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
        .into_error(Box::new(cause) as nyanpasu_contracts::BoxedCause);
        assert!(matches!(aborted, CommitAborted::WriteConfig { .. }));

        let wire = wire(ProfilesError::Commit {
            uid: Some(ProfileId("r1".into())),
            path: Some(ManagedProfilePath::new("r1.yaml").unwrap()),
            source: aborted,
            cleanup_failures: Vec::new(),
        });
        assert_eq!(
            wire["kind"]["error"],
            json!({
                "kind": "commit",
                "uid": "r1",
                "path": "r1.yaml",
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
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unsupported_engine_mutation_serializes_typed_ipc_envelope() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        use nyanpasu_config::{
            application::{ClashCore, NyanpasuAppConfig},
            profile::ScriptRuntime,
            runtime::{
                executor::{
                    PortError, ProfileContentSource, ScriptExecutionError, ScriptOperation,
                    ScriptRunner, StepLogEntry,
                },
                value::ConfigValue,
            },
        };
        use nyanpasu_core::client::{
            NyanpasuClient, runtime_build::RuntimeBuildService, test_support,
            test_support::TestControlEndpoint,
        };
        use struct_patch::Patch as _;

        struct Scripts(Arc<AtomicBool>);
        impl ScriptRunner for Scripts {
            fn run(
                &self,
                runtime: ScriptRuntime,
                _: &str,
                config: &ConfigValue,
                _: &mut Vec<StepLogEntry>,
            ) -> Result<ConfigValue, PortError> {
                if self.0.load(Ordering::SeqCst) {
                    Err(ScriptExecutionError::StartEngine {
                        runtime,
                        operation: ScriptOperation::Transform,
                    }
                    .into())
                } else {
                    Ok(config.clone())
                }
            }
            fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
                panic!("builtin transform")
            }
            fn eval_item_expr(&self, _: &str, _: &ConfigValue) -> Result<ConfigValue, PortError> {
                panic!("builtin transform")
            }
        }
        struct Content;
        impl ProfileContentSource for Content {
            fn read(&self, _: &ManagedProfilePath) -> Result<String, PortError> {
                Ok("proxies: []".into())
            }
        }

        for (core, runtime) in [
            (ClashCore::Mihomo, ScriptRuntime::JavaScript),
            (ClashCore::ClashRs, ScriptRuntime::Lua),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let endpoint = TestControlEndpoint::succeeding();
            let mut args =
                crate::test_support::test_client_args_with_endpoint(&dir, endpoint.clone()).await;
            let unavailable = Arc::new(AtomicBool::new(false));
            args.host_adapters.runtime_build = Arc::new(RuntimeBuildService::new(
                Arc::new(Content),
                Arc::new(test_support::runtime_core_spec),
                args.runtime_paths.clone(),
                {
                    let unavailable = unavailable.clone();
                    Arc::new(move || Ok(Box::new(Scripts(unavailable.clone()))))
                },
            ));
            let app_path = args.paths.application_config_path();
            let profiles_path = args.paths.profiles_path();
            let shutdown = args.shutdown.clone();
            let tasks = args.tasks.clone();
            let client = NyanpasuClient::try_new(args).await.unwrap();
            endpoint.prime(&client).await;
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.core = Some(core);
            patch.enable_builtin_enhanced = Some(true);
            client.patch_app_config(patch).await.unwrap();
            let uid = client
                .add_profile(
                    test_support::minimal_file_profile_request(),
                    Some("proxies: []".into()),
                )
                .await
                .unwrap()
                .into_value();
            let profile_path = client.get_profiles().await.unwrap().items[&uid]
                .definition
                .source()
                .unwrap()
                .materialized()
                .file
                .to_string();
            let before = client.configuration_status().source_versions;
            let app_bytes = std::fs::read(&app_path).ok();
            let profiles_bytes = std::fs::read(&profiles_path).ok();
            let submissions = endpoint.submissions();
            unavailable.store(true, Ordering::SeqCst);

            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.core = Some(core);
            let config_error = client.patch_app_config(patch).await.unwrap_err();
            let profile_error = client
                .activate_profile(Some(uid.clone()))
                .await
                .unwrap_err();
            for (domain, error) in [("config", config_error), ("profiles", profile_error)] {
                let message = error.to_string();
                let detail = format!("{error:?}");
                let wire = serde_json::to_value(IpcError::from(error)).unwrap();
                assert_eq!(wire["kind"]["domain"], domain);
                let source_error = &wire["kind"]["error"];
                assert_eq!(source_error["kind"], "commit");
                if domain == "config" {
                    assert_eq!(source_error["domain"], "application");
                } else {
                    assert_eq!(source_error["uid"], uid.to_string());
                    assert_eq!(source_error["path"], profile_path);
                }
                let commit = &source_error["source"];
                assert_eq!(commit["kind"], "runtime_refused");
                assert_eq!(commit["runtime"], serde_json::json!({"kind": "untouched"}));
                assert_eq!(
                    commit["errors"],
                    serde_json::json!([{
                        "kind": "build_runtime", "source": {
                            "kind": "run_pipeline", "source": {
                                "kind": "execute_script", "source": {
                                    "kind": "start_engine", "runtime": runtime, "operation": "transform",
                                }
                            }
                        }
                    }])
                );
                assert_eq!(wire["message"], message);
                assert_eq!(wire["detail"], detail);
                assert!(detail.contains("StartEngine"));
                assert!(detail.contains("Transform"));
                assert!(detail.contains(&format!("{runtime:?}")));
            }
            let after = client.configuration_status().source_versions;
            assert_eq!(
                (after.application, after.clash, after.profiles),
                (before.application, before.clash, before.profiles)
            );
            assert_eq!(std::fs::read(&app_path).ok(), app_bytes);
            assert_eq!(std::fs::read(&profiles_path).ok(), profiles_bytes);
            assert_eq!(
                endpoint.submissions(),
                submissions,
                "failure precedes core application"
            );
            shutdown.cancel();
            tasks.close();
            tasks.wait().await;
        }
    }
}

#[nyanpasu_macro::rpc(capabilities(debug_http))]
#[tauri::command]
#[specta::specta]
pub async fn set_debug_http_enabled(
    client: State<'_, NyanpasuClient>,
    enabled: bool,
) -> Result<nyanpasu_core::client::debug_http::DebugHttpStatus> {
    Ok(client.set_debug_http_enabled(enabled).await?)
}

#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn get_transparent_proxy_status(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus> {
    Ok(client.get_transparent_proxy_status().await?)
}
