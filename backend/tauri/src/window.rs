//! Tauri window management mod
//!

#[cfg(target_os = "macos")]
pub mod macos {
    #![allow(non_snake_case)]
    use std::{borrow::BorrowMut, cell::RefCell};

    use objc2::{
        declare_class, msg_send_id, mutability, rc::Retained, runtime::ProtocolObject, ClassType,
        DeclaredClass,
    };
    use objc2_app_kit::{NSApplicationPresentationOptions, NSWindow, NSWindowDelegate};
    use objc2_foundation::{MainThreadMarker, NSNotification, NSObject, NSObjectProtocol};
    use tauri::{Emitter, Listener, Manager, Runtime, WebviewWindow, Window, WindowEvent};

    #[derive(Debug, Clone, Copy)]
    pub struct Position {
        pub x: f64,
        pub y: f64,
    }

    impl From<(f64, f64)> for Position {
        fn from(value: (f64, f64)) -> Self {
            Self {
                x: value.0,
                y: value.1,
            }
        }
    }

    impl From<Position> for (f64, f64) {
        fn from(value: Position) -> Self {
            (value.x, value.y)
        }
    }

    fn set_traffic_lights_pos(
        window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
        pos: Position,
    ) -> anyhow::Result<()> {
        use objc2_app_kit::NSWindowButton;
        use objc2_foundation::NSRect;
        let close = window
            .standardWindowButton(NSWindowButton::NSWindowCloseButton)
            .ok_or(anyhow::anyhow!("failed to get close button"))?;
        let miniaturize = window
            .standardWindowButton(NSWindowButton::NSWindowMiniaturizeButton)
            .ok_or(anyhow::anyhow!("failed to get miniaturize button"))?;
        let zoom = window
            .standardWindowButton(NSWindowButton::NSWindowZoomButton)
            .ok_or(anyhow::anyhow!("failed to get zoom button"))?;

        let title_bar_container_view = unsafe {
            close
                .superview()
                .and_then(|view| view.superview())
                .ok_or(anyhow::anyhow!("failed to get title bar container view"))?
        };

        let close_rect = close.frame();
        let button_height = close_rect.size.height;

        let title_bar_frame_height = button_height + pos.y;
        let mut title_bar_rect = title_bar_container_view.frame();
        title_bar_rect.size.height = title_bar_frame_height;
        title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
        unsafe {
            title_bar_container_view.setFrame(title_bar_rect);
        }

        let space_between = miniaturize.frame().origin.x - close.frame().origin.x;
        let window_buttons = vec![close, miniaturize, zoom];

        for (i, button) in window_buttons.into_iter().enumerate() {
            let mut rect: NSRect = button.frame();
            rect.origin.x = pos.x + (i as f64 * space_between);
            unsafe {
                button.setFrameOrigin(rect.origin);
            }
        }
        Ok(())
    }

    #[derive(Debug, Clone)]
    struct WindowState {
        window: WebviewWindow<tauri::Wry>,
        traffic_lights_pos: Position,
    }

    impl WindowState {
        fn new(window: WebviewWindow<tauri::Wry>, traffic_lights_pos: Position) -> Self {
            Self {
                window,
                traffic_lights_pos,
            }
        }

        fn with_ns_window<T>(&self, func: impl FnOnce(Retained<NSWindow>) -> T) -> T {
            let ns_window = self.window.ns_window().expect("window not found");
            let ns_window = unsafe { Retained::retain_autoreleased(ns_window as *mut NSWindow) }
                .expect("failed to retain window");
            func(ns_window)
        }

        fn apply_traffic_lights_pos(&self) {
            self.with_ns_window(|win| {
                set_traffic_lights_pos(win, self.traffic_lights_pos)
                    .expect("failed to set traffic lights pos");
            });
        }
    }

    #[derive(Debug)]
    struct TrafficLightsWindowDelegateIvars {
        app_box: WindowState,
        super_class: Retained<ProtocolObject<dyn NSWindowDelegate>>,
    }

    const WINDOW_DID_ENTER_FULL_SCREEN: &str = "internal:://window-did-enter-full-screen";
    const WINDOW_WILL_ENTER_FULL_SCREEN: &str = "internal:://window-will-enter-full-screen";
    const WINDOW_WILL_EXIT_FULL_SCREEN: &str = "internal:://window-will-exit-full-screen";
    const WINDOW_DID_EXIT_FULL_SCREEN: &str = "internal:://window-did-exit-full-screen";

    declare_class! {
        struct WindowDelegate;

        unsafe impl ClassType for WindowDelegate {
            type Super = NSObject;
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "TrafficLightsPosWindowDelegate";
        }

        impl DeclaredClass for WindowDelegate {
            type Ivars = TrafficLightsWindowDelegateIvars;
        }

        unsafe impl NSObjectProtocol for WindowDelegate {}

        unsafe impl NSWindowDelegate for WindowDelegate {
            #[method(windowShouldClose:)]
            unsafe fn windowShouldClose(&self, sender: &NSWindow) -> bool {
                tracing::trace!("passthrough `windowShouldClose` to TAO layer");
                self.ivars().super_class.windowShouldClose(sender)
            }

            #[method(windowWillClose:)]
            unsafe fn windowWillClose(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowWillClose` to TAO layer");
                self.ivars().super_class.windowWillClose(notification)
            }

            #[method(windowDidResize:)]
            unsafe fn windowDidResize(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidResize` to TAO layer");
                self.ivars().super_class.windowDidResize(notification)
            }

            #[method(windowDidMove:)]
            unsafe fn windowDidMove(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidMove` to TAO layer");
                self.ivars().super_class.windowDidMove(notification)
            }

            #[method(windowDidChangeBackingProperties:)]
            unsafe fn windowDidChangeBackingProperties(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidChangeBackingProperties` to TAO layer");
                self.ivars().super_class.windowDidChangeBackingProperties(notification)
            }

            #[method(windowDidBecomeKey:)]
            unsafe fn windowDidBecomeKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidBecomeKey` to TAO layer");
                self.ivars().super_class.windowDidBecomeKey(notification)
            }

            #[method(windowDidResignKey:)]
            unsafe fn windowDidResignKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidResignKey` to TAO layer");
                self.ivars().super_class.windowDidResignKey(notification)
            }

            #[method(window:willUseFullScreenPresentationOptions:)]
            unsafe fn window_willUseFullScreenPresentationOptions(&self, window: &NSWindow, options: NSApplicationPresentationOptions) -> NSApplicationPresentationOptions {
                tracing::trace!("passthrough `window_willUseFullScreenPresentationOptions` to TAO layer");
                self.ivars().super_class.window_willUseFullScreenPresentationOptions(window, options)
            }

            #[method(windowDidEnterFullScreen:)]
            unsafe fn windowDidEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-enter-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowDidEnterFullScreen` to TAO layer");
                self.ivars().super_class.windowDidEnterFullScreen(notification)
            }

            #[method(windowWillEnterFullScreen:)]
            unsafe fn windowWillEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-enter-full-screen event: {}", e);
                }
                self.ivars().super_class.windowWillEnterFullScreen(notification)
            }

            #[method(windowWillExitFullScreen:)]
            unsafe fn windowWillExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-exit-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowWillExitFullScreen` to TAO layer");
                self.ivars().super_class.windowWillExitFullScreen(notification)
            }

            #[method(windowDidExitFullScreen:)]
            unsafe fn windowDidExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-exit-full-screen event: {}", e);
                }
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidExitFullScreen` to TAO layer");
                self.ivars().super_class.windowDidExitFullScreen(notification)
            }

            #[method(windowDidFailToEnterFullScreen:)]
            unsafe fn windowDidFailToEnterFullScreen(&self,window: &NSWindow) {
                tracing::trace!("passthrough `windowDidFailToEnterFullScreen` to TAO layer");
                self.ivars().super_class.windowDidFailToEnterFullScreen(window)
            }

        }
    }

    impl WindowDelegate {
        pub fn new(window_state: WindowState, mtm: MainThreadMarker) -> Retained<Self> {
            let this = mtm.alloc::<Self>();
            let super_class = window_state
                .with_ns_window(|win| unsafe { win.delegate().expect("failed to get delegate") });
            let ivars = TrafficLightsWindowDelegateIvars {
                app_box: window_state,
                super_class,
            };
            let this = this.set_ivars(ivars);
            unsafe { msg_send_id![super(this), init] }
        }
    }

    pub struct TrafficLightsWindowDelegateGuard {
        _delegate: Retained<WindowDelegate>,
    }

    thread_local! {
        /// This is used to keep the delegate alive until the window is destroyed
        static TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD: RefCell<Option<TrafficLightsWindowDelegateGuard>> = const { RefCell::new(None) };
    }

    pub fn setup_traffic_lights_pos(window: WebviewWindow, pos: (f64, f64), mtm: MainThreadMarker) {
        let window_state = WindowState::new(window.clone(), pos.into());
        let ns_window = window_state.with_ns_window(|win| win);
        let window_state_clone = window_state.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::ThemeChanged(_) => {
                window_state_clone.apply_traffic_lights_pos();
            }
            WindowEvent::Destroyed => {
                let _ = TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD.take();
            }
            _ => {}
        });
        // first apply the traffic lights pos
        window_state.apply_traffic_lights_pos();
        let delegate = WindowDelegate::new(window_state, mtm);
        let object = ProtocolObject::from_ref(delegate.as_ref());
        ns_window.setDelegate(Some(object));
        TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD.replace(Some(TrafficLightsWindowDelegateGuard {
            _delegate: delegate,
        }));
    }
}
