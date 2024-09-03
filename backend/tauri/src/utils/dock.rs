#[cfg(target_os = "macos")]
pub mod macos {
    extern crate cocoa;
    extern crate objc;

    use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicy};
    use objc::runtime::YES;

    pub unsafe fn show_dock_icon() {
        let app = NSApp();
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyRegular,
        );
        app.activateIgnoringOtherApps_(YES);
    }

    pub unsafe fn hide_dock_icon() {
        let app = NSApp();
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory,
        );
    }
}
