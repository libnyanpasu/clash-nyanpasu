#[cfg(target_os = "macos")]
pub mod macos {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;
    use std::cell::Cell;
    thread_local! {
        static MARK: Cell<MainThreadMarker> = Cell::new(MainThreadMarker::new().unwrap());
    }

    pub fn show_dock_icon() {
        let app = NSApplication::sharedApplication(MARK.get());
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        unsafe {
            app.activate();
        }
    }

    pub fn hide_dock_icon() {
        let app = NSApplication::sharedApplication(MARK.get());
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}
