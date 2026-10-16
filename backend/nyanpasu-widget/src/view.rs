use nyanpasu_widget_protocol::LogoPreset;
use slint::{ComponentHandle, Global};

use crate::{format::StatisticText, ui};

/// A widget window that shows the shared [`ui::StatisticData`]. Every window
/// exported by `ui/widgets.slint` is one, so the IPC wiring is written once.
pub(crate) trait StatisticView: ComponentHandle + 'static {
    fn show_statistic(&self, text: StatisticText);
    fn show_logo(&self, preset: LogoPreset);
}

impl<T> StatisticView for T
where
    T: ComponentHandle + 'static,
    for<'a> ui::StatisticData<'a>: Global<'a, T>,
{
    fn show_statistic(&self, text: StatisticText) {
        let data = self.global::<ui::StatisticData>();
        data.set_download_total(text.download_total);
        data.set_upload_total(text.upload_total);
        data.set_download_speed(text.download_speed);
        data.set_upload_speed(text.upload_speed);
    }

    fn show_logo(&self, preset: LogoPreset) {
        let preset = match preset {
            LogoPreset::Default => ui::LogoPreset::Default,
            LogoPreset::System => ui::LogoPreset::System,
            LogoPreset::Tun => ui::LogoPreset::Tun,
        };
        self.global::<ui::StatisticData>().set_logo(preset);
    }
}
