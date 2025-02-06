pub mod network_statistic_large;
pub mod network_statistic_small;

pub use network_statistic_large::NyanpasuNetworkStatisticLargeWidget;
pub use network_statistic_small::NyanpasuNetworkStatisticSmallWidget;

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
