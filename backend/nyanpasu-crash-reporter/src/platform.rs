use i_slint_backend_winit::{
    Backend, CustomApplicationHandler, EventResult, winit::event_loop::ActiveEventLoop,
};
use slint::{PlatformError, fontique_012::fontique};

/// Extends Slint's script fallbacks before any text is shaped. Windows can pick
/// a Japanese Han font even for Chinese text; one family does not cover all CJK.
pub(crate) fn configure_fonts() {
    extend_cjk_fallbacks(
        &mut slint::fontique_012::shared_collection(),
        &[
            "Microsoft YaHei UI",
            "Microsoft JhengHei UI",
            "Yu Gothic UI",
            "Malgun Gothic",
            "PingFang SC",
            "PingFang TC",
            "Hiragino Sans",
            "Apple SD Gothic Neo",
            "Noto Sans CJK SC",
            "Noto Sans CJK TC",
            "Noto Sans CJK JP",
            "Noto Sans CJK KR",
            "Source Han Sans SC",
            "Source Han Sans TC",
            "Source Han Sans JP",
            "Source Han Sans KR",
            "WenQuanYi Micro Hei",
        ],
    );
}

pub(crate) fn extend_cjk_fallbacks(collection: &mut fontique::Collection, families: &[&str]) {
    let additions: Vec<_> = families
        .iter()
        .filter_map(|name| collection.family_id(name))
        .collect();
    for script in [*b"Hani", *b"Hira", *b"Kana", *b"Hang"] {
        let key = fontique::FallbackKey::new(fontique::Script::from_bytes(script), None);
        let mut fallbacks: Vec<_> = collection.fallback_families(key).collect();
        for family in &additions {
            if !fallbacks.contains(family) {
                fallbacks.push(*family);
            }
        }
        collection.set_fallbacks(key, fallbacks.into_iter());
    }
}

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
