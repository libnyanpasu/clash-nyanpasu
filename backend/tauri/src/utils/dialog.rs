#![allow(dead_code)]

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
