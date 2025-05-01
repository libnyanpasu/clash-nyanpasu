pub mod network_statistic_large;
pub mod network_statistic_small;

use std::path::PathBuf;

pub use network_statistic_large::NyanpasuNetworkStatisticLargeWidget;
pub use network_statistic_small::NyanpasuNetworkStatisticSmallWidget;

fn get_window_state_path() -> std::io::Result<PathBuf> {
    let env = std::env::var("NYANPASU_EGUI_WINDOW_STATE_PATH").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "NYANPASU_EGUI_WINDOW_STATE_PATH is not set",
        )
    })?;

    let path = PathBuf::from(env);
    Ok(path)
}

#[cfg(target_os = "macos")]
// TODO: move this to nyanpasu-utils
fn set_application_activation_policy() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;
    use std::cell::Cell;
    thread_local! {
        static MARK: Cell<MainThreadMarker> = Cell::new(MainThreadMarker::new().unwrap());
    }

    let app = NSApplication::sharedApplication(MARK.get());
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    unsafe {
        app.activate();
    }
}

// pub fn launch_widget<'app, T: Send + Sync + Sized, A: EframeAppCreator<'app, T>>(
//     name: &str,
//     opts: eframe::NativeOptions,
//     creator: A,
// ) -> std::io::Result<Receiver<WidgetEvent<T>>> {
//     let (tx, rx) = mpsc::channel();
// }

#[derive(
    Debug,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
    Copy,
    Clone,
    PartialEq,
    Eq,
    clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum StatisticWidgetVariant {
    Large,
    Small,
}

impl std::fmt::Display for StatisticWidgetVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatisticWidgetVariant::Large => write!(f, "large"),
            StatisticWidgetVariant::Small => write!(f, "small"),
        }
    }
}

pub fn start_statistic_widget(size: StatisticWidgetVariant) -> eframe::Result {
    match size {
        StatisticWidgetVariant::Large => NyanpasuNetworkStatisticLargeWidget::run(),
        StatisticWidgetVariant::Small => NyanpasuNetworkStatisticSmallWidget::run(),
    }
}
