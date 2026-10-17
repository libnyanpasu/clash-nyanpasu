use i_slint_backend_winit::{Backend, winit::window::WindowAttributes};
use slint::PlatformError;

/// The renderer unless `SLINT_BACKEND` names one. Windows and macOS render in
/// software: softbuffer keeps per-pixel alpha there (on macOS through the
/// workspace's softbuffer patch) and no GPU driver gets loaded. On Linux
/// softbuffer 0.4 has no alpha, so the window renders through OpenGL. Slint
/// 1.18 only falls back to the software renderer when the GL one cannot be
/// constructed; without a usable driver, `SLINT_BACKEND=winit-software` forces
/// it.
const DEFAULT_RENDERER: &str = if cfg!(any(windows, target_os = "macos")) {
    "software"
} else {
    "femtovg"
};

/// Makes the winit backend the process's Slint platform. On Windows it also
/// disables the IME for the whole process, which only works before the first
/// window exists. Call it once, before the first window is created.
pub(crate) fn select_backend() -> Result<(), PlatformError> {
    #[cfg(windows)]
    // SAFETY: no precondition. Disabling IME for every thread of the process
    // (`u32::MAX`) keeps the text services framework, its threads and its
    // memory out of a window without input. The result is ignored: if it
    // fails, the widget only costs those resources and still works.
    unsafe {
        windows_sys::Win32::UI::Input::Ime::ImmDisableIME(u32::MAX);
    }

    let builder = Backend::builder()
        .with_renderer_name(renderer_name())
        .with_window_attributes_hook(window_attributes);
    #[cfg(target_os = "macos")]
    let builder = builder.with_event_loop_builder(accessory_event_loop());
    slint::platform::set_platform(Box::new(builder.build()?))
        .map_err(|error| PlatformError::from(error.to_string()))
}

/// The renderer `SLINT_BACKEND=winit-<renderer>` names, read the way Slint's
/// backend selector reads it, or [`DEFAULT_RENDERER`].
fn renderer_name() -> String {
    std::env::var("SLINT_BACKEND")
        .ok()
        .and_then(|backend| backend.strip_prefix("winit-").map(str::to_owned))
        .unwrap_or_else(|| DEFAULT_RENDERER.to_owned())
}

fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    #[cfg(windows)]
    let attributes = {
        use i_slint_backend_winit::winit::platform::windows::WindowAttributesExtWindows as _;
        // Drag and drop would initialize OLE, which starts a COM worker thread.
        attributes.with_skip_taskbar(true).with_drag_and_drop(false)
    };
    attributes
}

/// Keeps the widget out of the Dock and the application switcher.
#[cfg(target_os = "macos")]
fn accessory_event_loop() -> i_slint_backend_winit::EventLoopBuilder {
    use i_slint_backend_winit::winit::{
        event_loop::EventLoop,
        platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS as _},
    };

    let mut builder = EventLoop::with_user_event();
    builder.with_activation_policy(ActivationPolicy::Accessory);
    builder
}
