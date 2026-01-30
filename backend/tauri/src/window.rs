//! Tauri window management mod
//!

use crate::{
    config::{Config, nyanpasu::WindowState},
    log_err, trace_err,
};
use anyhow::Result;
use std::sync::atomic::{AtomicU16, Ordering};
use tauri::{AppHandle, Manager};

/// Global counter for tracking open windows
static OPEN_WINDOWS_COUNTER: AtomicU16 = AtomicU16::new(0);

/// Trait for window management
pub trait AppWindow {
    /// Get window label (e.g., "main", "editor")
    fn label(&self) -> &str;

    /// Get window title
    fn title(&self) -> &str;

    /// Get window URL path
    fn url(&self) -> &str;

    /// Get window state from config
    fn get_window_state(&self) -> Option<WindowState>;

    /// Set window state to config
    fn set_window_state(&self, state: Option<WindowState>);

    fn reset_window_open_counter(&self) {
        OPEN_WINDOWS_COUNTER.fetch_sub(1, Ordering::Release);
    }

    /// Create window with default implementation
    fn create(&self, app_handle: &AppHandle) -> Result<()> {
        if let Some(window) = app_handle.get_webview_window(self.label()) {
            tracing::debug!("{} window is already opened, try to show it", self.label());
            if OPEN_WINDOWS_COUNTER.load(Ordering::Acquire) == 0 {
                trace_err!(window.unminimize(), "set win unminimize");
                trace_err!(window.show(), "set win visible");
                trace_err!(window.set_focus(), "set win focus");
            }
            return Ok(());
        }

        let always_on_top = {
            *Config::verge()
                .latest()
                .always_on_top
                .as_ref()
                .unwrap_or(&false)
        };

        tracing::debug!("create {} window...", self.label());
        let mut builder = tauri::WebviewWindowBuilder::new(
            app_handle,
            self.label().clone(),
            tauri::WebviewUrl::App(self.url().into()),
        )
        .title(self.title())
        .fullscreen(false)
        .always_on_top(always_on_top)
        .min_inner_size(400.0, 600.0)
        .disable_drag_drop_handler();

        let win_state = &self.get_window_state();
        match win_state {
            Some(_) => {
                builder = builder.inner_size(800., 800.).position(0., 0.);
            }
            _ => {
                #[cfg(target_os = "windows")]
                {
                    builder = builder.inner_size(800.0, 636.0).center();
                }

                #[cfg(target_os = "macos")]
                {
                    builder = builder.inner_size(800.0, 642.0).center();
                }

                #[cfg(target_os = "linux")]
                {
                    builder = builder.inner_size(800.0, 642.0).center();
                }
            }
        };

        #[cfg(windows)]
        let win_res = builder
            .decorations(false)
            .transparent(true)
            .visible(false)
            .additional_browser_args("--enable-features=msWebView2EnableDraggableRegions --disable-features=OverscrollHistoryNavigation,msExperimentalScrolling")
            .build();
        #[cfg(target_os = "macos")]
        let win_res = builder
            .decorations(true)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .build();
        #[cfg(target_os = "linux")]
        let win_res = builder.decorations(true).transparent(false).build();

        match win_res {
            Ok(win) => {
                use tauri::{PhysicalPosition, PhysicalSize};

                if win_state.is_some() {
                    let state = win_state.as_ref().unwrap();
                    win.set_position(PhysicalPosition {
                        x: state.x,
                        y: state.y,
                    })
                    .unwrap();
                    win.set_size(PhysicalSize {
                        width: state.width,
                        height: state.height,
                    })
                    .unwrap();
                }

                if let Some(state) = win_state {
                    if state.maximized {
                        trace_err!(win.maximize(), "set win maximize");
                    }
                    if state.fullscreen {
                        trace_err!(win.set_fullscreen(true), "set win fullscreen");
                    }
                }
                #[cfg(windows)]
                trace_err!(win.set_shadow(true), "set win shadow");
                log::trace!("try to calculate the monitor size");
                let center = (|| -> Result<bool> {
                    let center;
                    if let Some(state) = win_state {
                        let monitor = win.current_monitor()?.ok_or(anyhow::anyhow!(""))?;
                        let PhysicalPosition { x, y } = *monitor.position();
                        let PhysicalSize { width, height } = *monitor.size();
                        let left = x;
                        let right = x + width as i32;
                        let top = y;
                        let bottom = y + height as i32;

                        let x = state.x;
                        let y = state.y;
                        let width = state.width as i32;
                        let height = state.height as i32;
                        center = ![
                            (x, y),
                            (x + width, y),
                            (x, y + height),
                            (x + width, y + height),
                        ]
                        .into_iter()
                        .any(|(x, y)| x >= left && x < right && y >= top && y < bottom);
                    } else {
                        center = true;
                    }
                    Ok(center)
                })();

                if center.unwrap_or(true) {
                    trace_err!(win.center(), "set win center");
                }

                #[cfg(debug_assertions)]
                {
                    if let Some(webview_window) = win.get_webview_window(self.label()) {
                        webview_window.open_devtools();
                    }
                }

                #[cfg(target_os = "macos")]
                {
                    tracing::trace!("setup traffic lights pos");
                    let mtm = objc2_foundation::MainThreadMarker::new().unwrap();
                    crate::window::macos::setup_traffic_lights_pos(win.clone(), (18.0, 22.0), mtm);
                }

                OPEN_WINDOWS_COUNTER.fetch_add(1, Ordering::Release);
            }
            Err(err) => {
                log::error!(target: "app", "failed to create window, {err:?}");
                if let Some(win) = app_handle.get_webview_window(self.label()) {
                    // Cleanup window if failed to create, it's a workaround for tauri bug
                    log_err!(
                        win.destroy(),
                        "occur error when close window while failed to create"
                    );
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings6;
            use windows_core::Interface;

            app_handle
                .get_webview_window(self.label())
                .ok_or(anyhow::anyhow!("failed to get window"))?
                .with_webview(|webview| unsafe {
                    let settings = webview
                        .controller()
                        .CoreWebView2()
                        .unwrap()
                        .Settings()
                        .unwrap();
                    let settings: ICoreWebView2Settings6 =
                        settings.cast::<ICoreWebView2Settings6>().unwrap();
                    settings.SetIsSwipeNavigationEnabled(false).unwrap();
                })
                .unwrap();
        }

        Ok(())
    }

    /// Close window with default implementation
    fn close(&self, app_handle: &AppHandle) {
        if let Some(window) = app_handle.get_webview_window(self.label()) {
            trace_err!(window.close(), "close window");
            self.reset_window_open_counter()
        }
    }

    /// Check if window is open with default implementation
    fn is_open(&self, app_handle: &AppHandle) -> bool {
        app_handle.get_webview_window(self.label()).is_some()
    }

    /// Save window state with default implementation
    fn save_state(&self, app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
        let win = app_handle
            .get_webview_window(self.label())
            .ok_or(anyhow::anyhow!("failed to get window"))?;
        let current_monitor = win.current_monitor()?;

        let state = match current_monitor {
            Some(_) => {
                let mut state = WindowState {
                    maximized: win.is_maximized()?,
                    fullscreen: win.is_fullscreen()?,
                    ..WindowState::default()
                };
                let is_minimized = win.is_minimized()?;

                let size = win.inner_size()?;
                if size.width > 0 && size.height > 0 && !state.maximized && !is_minimized {
                    state.width = size.width;
                    state.height = size.height;
                }
                let position = win.outer_position()?;
                if !state.maximized && !is_minimized {
                    state.x = position.x;
                    state.y = position.y;
                }
                Some(state)
            }
            None => None,
        };

        self.set_window_state(state);

        if save_to_file {
            Config::verge().data().save_file()?;
        }

        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    #![allow(non_snake_case)]
    use std::cell::RefCell;

    use objc2::{
        DeclaredClass, MainThreadOnly, define_class, msg_send, rc::Retained,
        runtime::ProtocolObject,
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
            .standardWindowButton(NSWindowButton::CloseButton)
            .ok_or(anyhow::anyhow!("failed to get close button"))?;
        let miniaturize = window
            .standardWindowButton(NSWindowButton::MiniaturizeButton)
            .ok_or(anyhow::anyhow!("failed to get miniaturize button"))?;
        let zoom = window
            .standardWindowButton(NSWindowButton::ZoomButton)
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

    define_class! {
        #[unsafe(super(NSObject))]
        #[name = "TrafficLightsPosWindowDelegate"]
        #[thread_kind = MainThreadOnly]
        #[ivars = TrafficLightsWindowDelegateIvars]
        struct WindowDelegate;

        unsafe impl NSObjectProtocol for WindowDelegate {}

        unsafe impl NSWindowDelegate for WindowDelegate {
            #[unsafe(method(windowShouldClose:))]
            unsafe fn windowShouldClose(&self, sender: &NSWindow) -> bool {
                tracing::trace!("passthrough `windowShouldClose` to TAO layer");
                unsafe { self.ivars().super_class.windowShouldClose(sender) }
            }

            #[unsafe(method(windowWillClose:))]
            unsafe fn windowWillClose(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowWillClose` to TAO layer");
                unsafe { self.ivars().super_class.windowWillClose(notification) }
            }

            #[unsafe(method(windowDidResize:))]
            unsafe fn windowDidResize(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidResize` to TAO layer");
                unsafe { self.ivars().super_class.windowDidResize(notification) }
            }

            #[unsafe(method(windowDidMove:))]
            unsafe fn windowDidMove(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidMove` to TAO layer");
                unsafe { self.ivars().super_class.windowDidMove(notification) }
            }

            #[unsafe(method(windowDidChangeBackingProperties:))]
            unsafe fn windowDidChangeBackingProperties(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidChangeBackingProperties` to TAO layer");
                unsafe { self.ivars().super_class.windowDidChangeBackingProperties(notification) }
            }

            #[unsafe(method(windowDidBecomeKey:))]
            unsafe fn windowDidBecomeKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidBecomeKey` to TAO layer");
                unsafe { self.ivars().super_class.windowDidBecomeKey(notification) }
            }

            #[unsafe(method(windowDidResignKey:))]
            unsafe fn windowDidResignKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidResignKey` to TAO layer");
                unsafe { self.ivars().super_class.windowDidResignKey(notification) }
            }

            #[unsafe(method(window:willUseFullScreenPresentationOptions:))]
            unsafe fn window_willUseFullScreenPresentationOptions(&self, window: &NSWindow, options: NSApplicationPresentationOptions) -> NSApplicationPresentationOptions {
                tracing::trace!("passthrough `window_willUseFullScreenPresentationOptions` to TAO layer");
                unsafe { self.ivars().super_class.window_willUseFullScreenPresentationOptions(window, options) }
            }

            #[unsafe(method(windowDidEnterFullScreen:))]
            unsafe fn windowDidEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-enter-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowDidEnterFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidEnterFullScreen(notification) }
            }

            #[unsafe(method(windowWillEnterFullScreen:))]
            unsafe fn windowWillEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-enter-full-screen event: {}", e);
                }
                unsafe { self.ivars().super_class.windowWillEnterFullScreen(notification) }
            }

            #[unsafe(method(windowWillExitFullScreen:))]
            unsafe fn windowWillExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-exit-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowWillExitFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowWillExitFullScreen(notification) }
            }

            #[unsafe(method(windowDidExitFullScreen:))]
            unsafe fn windowDidExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-exit-full-screen event: {}", e);
                }
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidExitFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidExitFullScreen(notification) }
            }

            #[unsafe(method(windowDidFailToEnterFullScreen:))]
            unsafe fn windowDidFailToEnterFullScreen(&self,window: &NSWindow) {
                tracing::trace!("passthrough `windowDidFailToEnterFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidFailToEnterFullScreen(window) }
            }

        }
    }

    impl WindowDelegate {
        pub fn new(window_state: WindowState, mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm);
            let super_class = window_state
                .with_ns_window(|win| unsafe { win.delegate().expect("failed to get delegate") });
            let ivars = TrafficLightsWindowDelegateIvars {
                app_box: window_state,
                super_class,
            };
            let this = this.set_ivars(ivars);
            unsafe { msg_send![super(this), init] }
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
        let object: &ProtocolObject<dyn NSWindowDelegate> = ProtocolObject::from_ref(&*delegate);
        ns_window.setDelegate(Some(object));
        TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD.replace(Some(TrafficLightsWindowDelegateGuard {
            _delegate: delegate,
        }));
    }
}
