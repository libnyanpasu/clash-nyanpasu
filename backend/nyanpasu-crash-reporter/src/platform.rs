use i_slint_backend_winit::{
    Backend, CustomApplicationHandler, EventResult, winit::event_loop::ActiveEventLoop,
};
use slint::{LogicalSize, PhysicalPosition, PlatformError};

/// Makes the software-rendered winit backend the process's Slint platform.
/// `on_resumed` runs when the event loop starts, before winit creates the
/// shown windows. Call once, on the main thread, before creating the window.
pub(crate) fn select_backend(
    on_resumed: impl FnMut(&ActiveEventLoop) + 'static,
) -> Result<(), PlatformError> {
    let backend = Backend::builder()
        .with_renderer_name("software")
        .with_custom_application_handler(Box::new(OnResumed(on_resumed)))
        .build()?;
    slint::platform::set_platform(Box::new(backend))
        .map_err(|error| PlatformError::from(error.to_string()))
}

struct OnResumed<F>(F);

impl<F: FnMut(&ActiveEventLoop)> CustomApplicationHandler for OnResumed<F> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) -> EventResult {
        (self.0)(event_loop);
        EventResult::Propagate
    }
}

/// Places a window of `size` in the middle of the primary monitor. Before
/// winit creates the window, Slint keeps the position for its creation, so
/// the window opens there instead of where the system puts new windows.
pub(crate) fn centre(window: &slint::Window, size: LogicalSize, event_loop: &ActiveEventLoop) {
    let Some(monitor) = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
    else {
        return;
    };
    let size = size.to_physical(monitor.scale_factor() as f32);
    let (origin, area) = (monitor.position(), monitor.size());
    window.set_position(PhysicalPosition::new(
        origin.x + (area.width as i32 - size.width as i32) / 2,
        origin.y + (area.height as i32 - size.height as i32) / 2,
    ));
}

/// The application's monospace stack, in the order of `--font-mono` in
/// `frontend/nyanpasu/src/assets/styles/tailwind.css`, so the report reads like
/// the logs and editors there. CSS generic names stand for the system's choice.
const MONOSPACE_STACK: &[&str] = &[
    "Cascadia Mono",
    "Fira Code",
    "ui-monospace",
    "SFMono-Regular",
    "Menlo",
    "Monaco",
    "Consolas",
    "Liberation Mono",
    "Courier New",
    "Sarasa Mono SC",
    "monospace",
];

/// The first family of [`MONOSPACE_STACK`] installed on this system. Slint takes
/// one family and falls back to a sans-serif font, not a monospace one, when it
/// is missing, so the stack is resolved here against the system fonts.
pub(crate) fn monospace_font() -> String {
    let mut collection = fontique::Collection::new(fontique::CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    first_installed(MONOSPACE_STACK, |name| {
        let id = match fontique::GenericFamily::parse(name) {
            Some(generic) => collection.generic_families(generic).next(),
            None => collection.family_id(name),
        }?;
        Some(collection.family(id)?.name().to_owned())
    })
    .unwrap_or_default()
}

/// The family `resolve` finds for the first entry of `stack` that has one.
fn first_installed(stack: &[&str], resolve: impl FnMut(&str) -> Option<String>) -> Option<String> {
    stack.iter().copied().find_map(resolve)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_installed_family_of_the_stack_wins() {
        let installed = ["Consolas", "Menlo"];
        let resolved = first_installed(MONOSPACE_STACK, |name| {
            installed.contains(&name).then(|| name.to_owned())
        });
        assert_eq!(resolved.as_deref(), Some("Menlo"));
        assert_eq!(first_installed(MONOSPACE_STACK, |_| None), None);
    }

    #[test]
    fn this_system_has_a_monospace_family() {
        assert!(!monospace_font().is_empty());
    }
}
