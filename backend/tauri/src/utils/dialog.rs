use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use rust_i18n::t;

pub fn panic_dialog(msg: &str) {
    let msg = format!("{}\n\n{}", msg, t!("dialog.panic"));
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title("Clash Nyanpasu Crash".to_string())
        .set_description(msg)
        .set_buttons(MessageButtons::Ok)
        .show();
}

pub fn migrate_dialog() -> bool {
    let msg = format!("{}", t!("dialog.migrate"));
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Clash Nyanpasu Migration".to_string())
            .set_buttons(MessageButtons::YesNo)
            .set_description(msg)
            .show(),
        MessageDialogResult::Yes
    )
}

pub fn error_dialog(msg: String) {
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title("Clash Nyanpasu Error".to_string())
        .set_description(msg)
        .set_buttons(MessageButtons::Ok)
        .show();
}
