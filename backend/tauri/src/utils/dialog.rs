use std::sync::{Arc, Barrier};
use tauri::api::dialog::{MessageDialogBuilder, MessageDialogButtons, MessageDialogKind};

pub fn panic_dialog(msg: &str) {
    let msg = format!(
        "{}\n\nPlease report this issue to Github issue tracker.",
        msg
    );
    let barrier = Arc::new(Barrier::new(2));
    let barrier_ref = barrier.clone();
    MessageDialogBuilder::new("Error", msg)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::Ok)
        .show(move |_| {
            barrier_ref.wait();
        });
    barrier.wait();
}
