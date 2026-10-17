use nyanpasu_widget_protocol::LogoPreset;
use slint::{ComponentHandle, Global, Image, Rgba8Pixel, SharedPixelBuffer};

use crate::{format::StatisticText, logo, ui};

/// A widget window that shows the shared [`ui::StatisticData`]. Every window
/// exported by `ui/widgets.slint` is one, so the IPC wiring is written once.
pub(crate) trait StatisticView: ComponentHandle + 'static {
    fn show_statistic(&self, text: StatisticText);
    /// Shows the bundled cat in the colour of `preset`.
    fn show_builtin_logo(&self, preset: LogoPreset);
    /// Shows `source`, a decoded image logo, rasterized at `size` × `size`
    /// physical pixels.
    fn show_image_logo(&self, source: SharedPixelBuffer<Rgba8Pixel>, size: u32);
    /// Rasterizes the shown image logo again at `size`, once the scale factor
    /// changed. Does nothing while the bundled cat is shown.
    fn rescale_image_logo(&self, size: u32);
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

    fn show_builtin_logo(&self, preset: LogoPreset) {
        let preset = match preset {
            LogoPreset::Default => ui::LogoPreset::Default,
            LogoPreset::System => ui::LogoPreset::System,
            LogoPreset::Tun => ui::LogoPreset::Tun,
        };
        let data = self.global::<ui::StatisticData>();
        data.set_logo(preset);
        data.set_logo_image_shown(false);
        // Releases the pixels of an image logo shown before.
        data.set_logo_source(Image::default());
        data.set_logo_image(Image::default());
    }

    fn show_image_logo(&self, source: SharedPixelBuffer<Rgba8Pixel>, size: u32) {
        let data = self.global::<ui::StatisticData>();
        data.set_logo_image(Image::from_rgba8_premultiplied(logo::rasterize(
            &source, size,
        )));
        data.set_logo_source(Image::from_rgba8_premultiplied(source));
        data.set_logo_image_shown(true);
    }

    fn rescale_image_logo(&self, size: u32) {
        let data = self.global::<ui::StatisticData>();
        if !data.get_logo_image_shown() {
            return;
        }
        if let Some(source) = data.get_logo_source().to_rgba8_premultiplied() {
            data.set_logo_image(Image::from_rgba8_premultiplied(logo::rasterize(
                &source, size,
            )));
        }
    }
}
