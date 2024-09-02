#[cfg(target_os = "macos")]
pub mod macos {
    extern crate cocoa;
    extern crate objc;

    use cocoa::{
        appkit::{NSApp, NSApplication, NSApplicationActivationPolicy, NSWindow},
        base::{id, nil, BOOL, NO, YES},
    };
    use objc::{
        class,
        declare::ClassDecl,
        msg_send,
        runtime::{Object, Sel},
        sel, sel_impl,
    };
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

    pub fn setup_dock_click_handler() {
        unsafe {
            let app = NSApp();
            let superclass = class!(NSObject);
            let mut decl = ClassDecl::new("AppDelegate", superclass).unwrap();
            decl.add_method(
                sel!(applicationShouldHandleReopen:hasVisibleWindows:),
                reopen_handler as extern "C" fn(&mut Object, Sel, id, BOOL) -> BOOL,
            );
            decl.register();
            let delegate: id = msg_send![class!(AppDelegate), new];
            app.setDelegate_(delegate);
        }
    }

    extern "C" fn reopen_handler(_: &mut Object, _: Sel, _: id, has_visible_windows: BOOL) -> BOOL {
        unsafe {
            let app = NSApp();
            let current_policy: NSApplicationActivationPolicy = msg_send![app, activationPolicy];
            if current_policy
                == NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory
            {
                if has_visible_windows == NO {
                    // resolve crate window
                    let handle = crate::core::handle::Handle::global();
                    let app_handle = handle.app_handle.lock();
                    if let Some(app_handle) = app_handle.as_ref() {
                        crate::utils::resolve::create_window(app_handle);
                    }
                }
                NO
            } else {
                YES
            }
        }
    }
}
