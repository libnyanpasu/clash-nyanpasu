use rust_i18n::t;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Barrier,
};
use tauri::api::dialog::{MessageDialogBuilder, MessageDialogButtons, MessageDialogKind};

pub fn panic_dialog(msg: &str) {
    let msg = format!("{}\n\n{}", msg, t!("dialog.panic"));
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

pub fn migrate_dialog() -> bool {
    let msg = format!("{}", t!("dialog.migrate"));
    let barrier = Arc::new(Barrier::new(2));
    let barrier_ref = barrier.clone();
    let migrate = Arc::new(AtomicBool::new(false));
    let migrate_ref = migrate.clone();
    MessageDialogBuilder::new("Migration", msg)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNo)
        .show(move |_migrate| {
            migrate_ref.store(_migrate, Ordering::Relaxed);
            barrier_ref.wait();
        });
    barrier.wait();
    migrate.load(Ordering::Relaxed)
}

pub fn error_dialog(msg: String) {
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
