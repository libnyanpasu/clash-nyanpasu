use eframe::{App, AppCreator, CreationContext};
use std::error::Error;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

trait EframeAppCreator<'app, T: Send + Sync + Sized> =
    FnOnce(
        &CreationContext<'_>,
    )
        -> Result<Box<dyn 'app + AppWithListener<T>>, Box<dyn std::error::Error + Send + Sync>>;

pub trait AppWithListener<T: Send + Sync + Sized>: App + Listener<T> {}

pub mod network_statistic_large;
pub mod network_statistic_small;

pub use network_statistic_large::NyanpasuNetworkStatisticLargeWidget;
pub use network_statistic_small::NyanpasuNetworkStatisticSmallWidget;

trait Listener<T: Send + Sync + Sized> {
    fn listen(&self, event: WidgetEvent<T>);
}

pub struct WidgetEvent<T: Send + Sync + Sized> {
    pub id: u32,
    pub payload: T,
}

pub enum WidgetEventPayload<T: Send + Sync + Sized> {
    /// Terminate the egui window
    Terminate,
    /// User defined event
    Custom(T),
}

// pub fn launch_widget<'app, T: Send + Sync + Sized, A: EframeAppCreator<'app, T>>(
//     name: &str,
//     opts: eframe::NativeOptions,
//     creator: A,
// ) -> std::io::Result<Receiver<WidgetEvent<T>>> {
//     let (tx, rx) = mpsc::channel();
// }
