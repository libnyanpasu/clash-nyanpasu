//! The desktop-only `system.os` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::os`.

use chrono::Local;
use log::debug;
use nyanpasu_config::{
    application::{NyanpasuAppConfigPatch, ProfileFileOpener},
    profile::ProfileId,
};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc::error::{IpcError, Result};
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;
use nyanpasu_runtime_values::profiles::ProfileFileMissingSnafu;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{
    DialogExt, FileDialogBuilder, MessageDialogButtons as TauriDialogButtons,
    MessageDialogKind as TauriDialogKind, MessageDialogResult as TauriDialogResult,
};
use tauri_plugin_notification::NotificationExt;

use crate::{
    adapters::cli_install::{self, CliInstallation},
    utils::{help, proxy_env, proxy_env::CopyEnvOption},
};

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

#[nyanpasu_rpc::rpc(capabilities(dialog), expose = [])]
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

#[nyanpasu_rpc::rpc(capabilities(dialog), expose = [])]
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

#[nyanpasu_rpc::rpc(capabilities(clipboard), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn copy_clash_env(
    app_handle: AppHandle,
    client: State<'_, NyanpasuClient>,
    env_type: CopyEnvOption,
) {
    proxy_env::copy_clash_env(&app_handle, client.clash_info().port, &env_type);
}

#[nyanpasu_rpc::rpc(capabilities(elevation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn flush_system_dns_cache(client: State<'_, NyanpasuClient>) -> Result {
    client.flush_system_dns_cache().await?;
    Ok(())
}

#[cfg(windows)]
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
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
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[tauri::command]
#[specta::specta]
pub fn get_custom_app_dir() -> Result<Option<String>> {
    Ok(None)
}

/// Links the bundled `nyanpasu-cli` onto the user's `PATH`, for the macOS app and the AppImage.
#[nyanpasu_rpc::rpc(capabilities(os_integration), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn install_cli(paths: State<'_, PathResolver>) -> Result<CliInstallation> {
    cli_install::install_cli(&paths)
        .await
        .map_err(IpcError::unknown)
}

#[cfg(windows)]
#[nyanpasu_rpc::rpc(capabilities(elevation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn invoke_uwp_tool(
    app_handle: tauri::AppHandle,
    client: State<'_, NyanpasuClient>,
) -> Result {
    let resources_dir =
        (crate::utils::init::bundled_resources_dir(&app_handle)).map_err(anyhow::Error::from)?;
    let language = client.get_app_config().await?.language;
    (crate::core::win_uwp::invoke_uwptools(&resources_dir, language).await)?;
    Ok(())
}

#[cfg(not(windows))]
#[nyanpasu_rpc::rpc(expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn invoke_uwp_tool() -> Result {
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn open_app_config_dir(paths: State<'_, PathResolver>) -> Result<()> {
    (crate::utils::open::that(paths.app_config_dir()))?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn open_app_data_dir(paths: State<'_, PathResolver>) -> Result<()> {
    (crate::utils::open::that(paths.app_data_dir()))?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn open_backups_dir(client: State<'_, NyanpasuClient>) -> Result<()> {
    (crate::utils::open::that(client.backups_dir()?))?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn open_logs_dir(paths: State<'_, PathResolver>) -> Result<()> {
    (crate::utils::open::that(paths.app_logs_dir()))?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(dialog), expose = [])]
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

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn open_that(path: String) -> Result {
    (crate::utils::open::that(path))?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn open_web_url(url: String) -> Result<()> {
    (crate::utils::open::that(url))?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(clipboard), query, expose = [], sensitive)]
#[tauri::command]
#[specta::specta]
pub async fn read_clipboard_text(app_handle: AppHandle) -> Result<String> {
    Ok(app_handle
        .clipboard()
        .read_text()
        .map_err(|error| error.to_string())?)
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
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

#[cfg(windows)]
#[nyanpasu_rpc::rpc(capabilities(dialog, lifecycle, elevation), expose = [])]
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

#[cfg(not(windows))]
#[nyanpasu_rpc::rpc(expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn set_custom_app_dir(_path: String) -> Result {
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(dialog), expose = [])]
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

#[nyanpasu_rpc::rpc(capabilities(notification), expose = [])]
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

/// Take and clear the queued deep links, oldest first. The frontend calls it
/// once its [`SchemeRequestReceivedEvent`] listener is registered, and again on
/// every such event.
#[nyanpasu_rpc::rpc(capabilities(deep_link), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn take_pending_deep_links(pending: State<'_, PendingDeepLinks>) -> Result<Vec<String>> {
    Ok(pending.take_all())
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn view_profile(client: State<'_, NyanpasuClient>, uid: ProfileId) -> Result {
    let path = client.get_profile_materialized_path(uid.clone()).await?;
    if !path.exists() {
        return Err(ProfileFileMissingSnafu { uid, path: &path }.build().into());
    }
    help::reveal_in_file_manager(&path)?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn open_profile(client: State<'_, NyanpasuClient>, uid: ProfileId) -> Result {
    let path = client.get_profile_materialized_path(uid.clone()).await?;
    if !path.exists() {
        return Err(ProfileFileMissingSnafu { uid, path: &path }.build().into());
    }
    help::open_profile_file(&path, &client.get_app_config().await?.profile_file_opener)?;
    Ok(())
}

/// The selected program is a desktop-only, device-local preference.
#[nyanpasu_rpc::rpc(privacy, capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn set_profile_file_opener(
    client: State<'_, NyanpasuClient>,
    opener: ProfileFileOpener,
) -> Result<RpcUnitMutationOutcome> {
    help::validate_profile_file_opener(&opener)?;
    Ok(client
        .patch_app_config(NyanpasuAppConfigPatch {
            profile_file_opener: Some(opener),
            ..Default::default()
        })
        .await?
        .into())
}

#[nyanpasu_rpc::rpc(capabilities(clipboard), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn write_clipboard_text(app_handle: AppHandle, text: String) -> Result {
    app_handle
        .clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())?;
    Ok(())
}

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
