#![allow(dead_code)]

use nyanpasu_paths::ResolvedPaths;
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use rust_i18n::t;

pub fn panic_dialog(msg: &str) {
    let msg = format!("{}\n\n{}", msg, t!("dialog.panic"));
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title("Clash Nyanpasu Crash")
        .set_description(msg.as_str())
        .set_buttons(MessageButtons::Ok)
        .show();
}

/// Reports a failed startup migration and returns once the user chooses to exit.
/// Opening the backups dir or the log keeps the dialog up. `backup_failed` means
/// the pre-migration backup failed, so no file was modified and there is
/// nothing to restore.
pub fn migration_failed_dialog(error: &str, paths: &ResolvedPaths, backup_failed: bool) {
    let backups_dir = paths.backups_dir();
    let log_path = paths.app_data_dir().join("migration.log");
    let open_backups = t!("dialog.migration_failed.open_backups").to_string();
    let open_log = t!("dialog.migration_failed.open_log").to_string();
    let exit = t!("dialog.migration_failed.exit").to_string();

    let description = if backup_failed {
        format!("{error}\n\n{}", t!("dialog.migration_failed.not_modified"))
    } else {
        format!(
            "{error}\n\n{}\n\n{}",
            t!(
                "dialog.migration_failed.backup_kept",
                path = backups_dir.display()
            ),
            t!("dialog.migration_failed.recovery")
        )
    };

    loop {
        let buttons = if backup_failed {
            MessageButtons::OkCancelCustom(open_log.clone(), exit.clone())
        } else {
            MessageButtons::YesNoCancelCustom(open_backups.clone(), open_log.clone(), exit.clone())
        };
        let result = MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("Clash Nyanpasu Migration Failed")
            .set_description(description.as_str())
            .set_buttons(buttons)
            .show();

        let target = match result {
            MessageDialogResult::Custom(choice) if choice == open_backups => &backups_dir,
            MessageDialogResult::Custom(choice) if choice == open_log => &log_path,
            _ => return,
        };
        if let Err(error) = crate::utils::open::that(target) {
            tracing::warn!("failed to open {}: {error}", target.display());
        }
    }
}

pub fn migrate_dialog(msg: &str) -> bool {
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Clash Nyanpasu Migration")
            .set_buttons(MessageButtons::YesNo)
            .set_description(msg)
            .show(),
        MessageDialogResult::Yes
    )
}

pub fn error_dialog<T: Into<String>>(msg: T) {
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title("Clash Nyanpasu Error")
        .set_description(msg.into())
        .set_buttons(MessageButtons::Ok)
        .show();
}

pub fn warning_dialog<T: Into<String>>(msg: T) {
    MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("Clash Nyanpasu Warning")
        .set_description(msg.into())
        .set_buttons(MessageButtons::Ok)
        .show();
}

pub fn ask_dialog<T: Into<String>>(msg: T) -> bool {
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Info)
            .set_title("Clash Nyanpasu")
            .set_buttons(MessageButtons::YesNo)
            .set_description(msg.into())
            .show(),
        MessageDialogResult::Yes
    )
}
