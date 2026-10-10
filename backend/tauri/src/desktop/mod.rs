//! Desktop adapters and presentation owners.

pub mod app_update;
pub mod effects;
mod event_sink;
pub mod hotkey;
pub mod main_thread;
pub mod system_proxy_adapters;
pub mod ui_effects;

#[cfg(test)]
pub use event_sink::NoopUiEventSink;
pub use event_sink::{
    MainThreadHandoff, MainThreadWork, STATE_CHANGED_URI, StateChanged, TauriMainThread,
    TauriUiEventSink, UiEventSink,
};
pub use main_thread::MainThreadExecutor;

#[cfg(test)]
pub(crate) mod effects_test_support;
#[cfg(test)]
pub(crate) mod platform_test_support;

#[cfg(test)]
mod app_lifecycle_tests;
#[cfg(test)]
pub(crate) mod logs_test_support;
#[cfg(test)]
pub(crate) mod test_support;
