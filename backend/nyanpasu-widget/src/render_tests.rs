//! Renders both windows with the software renderer, without an OS window.

use std::rc::Rc;

use nyanpasu_widget_protocol::{LogoPreset, StatisticMessage};
use slint::{
    ComponentHandle as _, PhysicalSize, PlatformError, Rgba8Pixel, SharedPixelBuffer,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};

use crate::{format::StatisticText, logo, ui, view::StatisticView};

/// How far a logo pixel may differ from the reference, per premultiplied
/// channel: rounding through un- and re-premultiplication and the renderer's
/// own blending.
const LOGO_TOLERANCE: u32 = 2;

struct HeadlessPlatform;

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

struct Snapshot {
    pixels: SharedPixelBuffer<Rgba8Pixel>,
    scale: f32,
}

impl Snapshot {
    fn take(view: &impl StatisticView, logical: (f32, f32), scale: f32) -> Self {
        let window = view.window();
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        window.set_size(PhysicalSize::new(
            (logical.0 * scale) as u32,
            (logical.1 * scale) as u32,
        ));
        let pixels = window.take_snapshot().unwrap();
        assert_eq!(
            (pixels.width(), pixels.height()),
            ((logical.0 * scale) as u32, (logical.1 * scale) as u32)
        );
        Self { pixels, scale }
    }

    /// The pixel at the logical point `(x, y)`.
    fn at(&self, x: f32, y: f32) -> Rgba8Pixel {
        let x = (x * self.scale) as usize;
        let y = (y * self.scale) as usize;
        self.pixels.as_slice()[y * self.pixels.width() as usize + x]
    }

    fn assert_corners_transparent(&self) {
        let (w, h) = (self.pixels.width() - 1, self.pixels.height() - 1);
        for (x, y) in [(0, 0), (w, 0), (0, h), (w, h)] {
            let pixel = self.pixels.as_slice()[(y * self.pixels.width() + x) as usize];
            assert_eq!(pixel.a, 0, "corner ({x}, {y}) at scale {}", self.scale);
        }
    }

    /// Counts the light, mostly opaque pixels in the logical rectangle: icon and
    /// text pixels in the dark theme.
    fn light_pixels(&self, x: (f32, f32), y: (f32, f32)) -> usize {
        let range =
            |(from, to): (f32, f32)| (from * self.scale) as usize..(to * self.scale) as usize;
        let width = self.pixels.width() as usize;
        range(y)
            .flat_map(|y| range(x).map(move |x| y * width + x))
            .map(|index| self.pixels.as_slice()[index])
            .filter(|p| p.a > 200 && p.r > 160 && p.g > 160 && p.b > 160)
            .count()
    }

    /// Compares the logo whose box is at the logical `origin` and `height`
    /// with `svg` rasterized at exactly that box's physical size and
    /// composited over the badge pixel at `badge`. A logo that is resampled,
    /// shifted or clipped differs at its edges.
    fn assert_logo_is_exact(&self, svg: &[u8], origin: (f32, f32), height: f32, badge: (f32, f32)) {
        let physical = |v: f32| (v * self.scale).round() as u32;
        let (left, top) = (physical(origin.0), physical(origin.1));
        let width = physical(origin.0 + height * 6.0 / 7.0) - left;
        let height = physical(origin.1 + height) - top;
        let reference = rasterize_logo(svg, width, height);
        let under = premultiplied(self.at(badge.0, badge.1));
        self.assert_composited(
            (left, top),
            (width, height),
            |x, y| {
                let logo = reference.pixel(x, y).unwrap();
                [logo.red(), logo.green(), logo.blue(), logo.alpha()].map(u32::from)
            },
            |_, _| under,
        );
    }

    /// Compares the image logo filling the `size` badge at the logical
    /// `origin` with the logo rasterized at the badge's physical size, over
    /// the same window showing a transparent image logo: the image must be
    /// drawn unscaled.
    fn assert_image_logo_is_exact(
        &self,
        background: &Snapshot,
        source: &SharedPixelBuffer<Rgba8Pixel>,
        origin: (f32, f32),
        size: f32,
    ) {
        let physical = |v: f32| (v * self.scale).round() as u32;
        let (left, top) = (physical(origin.0), physical(origin.1));
        let size = physical(origin.0 + size) - left;
        let reference = logo::rasterize(source, size);
        let width = self.pixels.width();
        self.assert_composited(
            (left, top),
            (size, size),
            |x, y| {
                let pixel = reference.as_slice()[(y * size + x) as usize];
                [pixel.r, pixel.g, pixel.b, pixel.a].map(u32::from)
            },
            |x, y| premultiplied(background.pixels.as_slice()[(y * width + x) as usize]),
        );
    }

    /// Checks that the physical rectangle at `origin` shows the premultiplied
    /// `expected` pixels composited over `under` at each physical pixel.
    fn assert_composited(
        &self,
        (left, top): (u32, u32),
        (width, height): (u32, u32),
        expected: impl Fn(u32, u32) -> [u32; 4],
        under: impl Fn(u32, u32) -> [u32; 4],
    ) {
        let mut worst = 0;
        for y in 0..height {
            for x in 0..width {
                let over = expected(x, y);
                let under = under(left + x, top + y);
                let index = ((top + y) * self.pixels.width() + left + x) as usize;
                let actual = premultiplied(self.pixels.as_slice()[index]);
                for channel in 0..4 {
                    let expected = over[channel] + under[channel] * (255 - over[3]) / 255;
                    worst = worst.max(actual[channel].abs_diff(expected));
                }
            }
        }
        assert!(
            worst <= LOGO_TOLERANCE,
            "logo {width}x{height} at ({left}, {top}) differs by {worst} at scale {}",
            self.scale
        );
    }
}

fn premultiplied(pixel: Rgba8Pixel) -> [u32; 4] {
    let alpha = u32::from(pixel.a);
    let channel = |value: u8| u32::from(value) * alpha / 255;
    [channel(pixel.r), channel(pixel.g), channel(pixel.b), alpha]
}

/// The family name of a TrueType font as Windows names it in English: the
/// typographic family (name ID 16), else the family (name ID 1).
fn font_family(font: &[u8]) -> String {
    let u16_at = |at: usize| usize::from(u16::from_be_bytes([font[at], font[at + 1]]));
    let u32_at = |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) as usize;
    let table = (0..u16_at(4))
        .map(|index| 12 + 16 * index)
        .find(|&record| &font[record..record + 4] == b"name")
        .expect("the font has a name table");
    let names = u32_at(table + 8);
    let strings = names + u16_at(names + 4);
    let name = |id: usize| {
        (0..u16_at(names + 2))
            .map(|index| names + 6 + 12 * index)
            .find(|&record| {
                u16_at(record) == 3 && u16_at(record + 4) == 0x409 && u16_at(record + 6) == id
            })
            .map(|record| {
                let start = strings + u16_at(record + 10);
                let utf16: Vec<u16> = font[start..start + u16_at(record + 8)]
                    .chunks(2)
                    .map(|unit| u16::from_be_bytes([unit[0], unit[1]]))
                    .collect();
                String::from_utf16(&utf16).unwrap()
            })
    };
    name(16)
        .or_else(|| name(1))
        .expect("the font has a family name")
}

const LOGOS: [(LogoPreset, &[u8]); 3] = [
    (LogoPreset::Default, include_bytes!("../assets/logo.svg")),
    (
        LogoPreset::System,
        include_bytes!("../assets/logo-system-proxy.svg"),
    ),
    (LogoPreset::Tun, include_bytes!("../assets/logo-tun.svg")),
];

fn rasterize_logo(svg: &[u8], width: u32, height: u32) -> resvg::tiny_skia::Pixmap {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).unwrap();
    let size = tree.size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).unwrap();
    let scale = resvg::tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, scale, &mut pixmap.as_mut());
    pixmap
}

/// A 192 × 192 disc with a soft edge and a colour gradient, like the mascot
/// artwork: detail and partial alpha that minification can blur or fringe.
fn image_logo() -> SharedPixelBuffer<Rgba8Pixel> {
    let mut data = Vec::new();
    for y in 0..192 {
        for x in 0..192 {
            let distance = ((x as f32 - 95.5).powi(2) + (y as f32 - 95.5).powi(2)).sqrt();
            let alpha = ((96.0 - distance) * 64.0).clamp(0.0, 255.0) as u8;
            let stripe = if (x / 6) % 2 == 0 { 230 } else { 40 };
            data.extend_from_slice(&[stripe, y as u8, 255 - y as u8, alpha]);
        }
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 192, 192);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&data)
        .unwrap();
    logo::decode(&png).unwrap()
}

/// An image logo without a visible pixel: the window behind the badge.
fn transparent_logo() -> SharedPixelBuffer<Rgba8Pixel> {
    SharedPixelBuffer::new(1, 1)
}

/// Download values of ordinary width, upload values as wide as a value gets.
fn sample() -> StatisticText {
    StatisticText::from(&StatisticMessage {
        download_total: 123_456_789,
        upload_total: 888_884_999,
        download_speed: 345_678,
        upload_speed: 888_884_999,
    })
}

/// One test: the Slint platform is per process and the components belong to
/// the thread that created them.
#[test]
fn windows_render_translucent_panels_with_icons_and_values() {
    slint::platform::set_platform(Box::new(HeadlessPlatform)).unwrap();
    let large = ui::LargeWidget::new().unwrap();
    large.show_statistic(sample());
    let small = ui::SmallWidget::new().unwrap();
    small.show_statistic(sample());

    // A family the embedded font does not carry falls back to whatever the
    // system has, which fits on one machine and elides on another.
    assert_eq!(
        large.global::<ui::Theme>().get_font_family(),
        font_family(include_bytes!("../assets/Inter-Regular.ttf")),
    );

    for scale in [1.0, 1.5, 2.0] {
        let shot = Snapshot::take(&large, (206.0, 60.0), scale);
        shot.assert_corners_transparent();
        // Between the two value columns and the two rows: only the panel,
        // #1d1b20 at half alpha (give or take premultiplication rounding).
        let panel = shot.at(123.0, 30.0);
        let near = |actual: u8, expected: u8| actual.abs_diff(expected) <= 2;
        assert!(
            near(panel.r, 0x1d) && near(panel.g, 0x1b) && near(panel.b, 0x20) && near(panel.a, 128),
            "large panel {panel:?} at scale {scale}"
        );
        // The first badge (logical 53..73 x 8..28) shows its icon, and its
        // value cell its text.
        assert!(
            shot.light_pixels((57.0, 69.0), (12.0, 24.0)) >= 10,
            "icon at {scale}"
        );
        assert!(
            shot.light_pixels((73.0, 121.0), (8.0, 28.0)) >= 20,
            "value at {scale}"
        );
        // The 44px badge at (5, 8) centers the 24x28 logo, in each state's
        // colours.
        for (preset, svg) in LOGOS {
            large.show_builtin_logo(preset);
            let shot = Snapshot::take(&large, (206.0, 60.0), scale);
            shot.assert_logo_is_exact(svg, (15.0, 16.0), 28.0, (10.0, 30.0));
        }
        // An image logo fills the badge instead of the disc, unscaled.
        large.show_image_logo(transparent_logo(), (44.0 * scale).round() as u32);
        let background = Snapshot::take(&large, (206.0, 60.0), scale);
        large.show_image_logo(image_logo(), (44.0 * scale).round() as u32);
        let shot = Snapshot::take(&large, (206.0, 60.0), scale);
        shot.assert_image_logo_is_exact(&background, &image_logo(), (5.0, 8.0), 44.0);
        large.show_builtin_logo(LogoPreset::Default);
        // The sample's upload values are the widest a value gets; they fit,
        // and a character more does not.
        assert!(
            large.get_values_fit(),
            "large values elided at scale {scale}"
        );
        for wider in [
            StatisticText {
                upload_total: "888.88 MB8".into(),
                ..sample()
            },
            StatisticText {
                upload_speed: "888.88 MB/s8".into(),
                ..sample()
            },
        ] {
            large.show_statistic(wider);
            assert!(
                !large.get_values_fit(),
                "wider large value fits at scale {scale}"
            );
        }
        large.show_statistic(sample());

        let shot = Snapshot::take(&small, (80.0, 32.0), scale);
        shot.assert_corners_transparent();
        assert_eq!(shot.at(40.0, 2.0).a, 255, "small panel at scale {scale}");
        assert!(
            shot.light_pixels((29.0, 78.0), (4.0, 16.0)) >= 10,
            "upload at {scale}"
        );
        assert!(
            shot.light_pixels((29.0, 78.0), (16.0, 28.0)) >= 10,
            "download at {scale}"
        );
        // The 24px badge at (4, 4) centers the 12x14 logo.
        for (preset, svg) in LOGOS {
            small.show_builtin_logo(preset);
            let shot = Snapshot::take(&small, (80.0, 32.0), scale);
            shot.assert_logo_is_exact(svg, (10.0, 9.0), 14.0, (6.0, 16.0));
        }
        small.show_image_logo(transparent_logo(), (24.0 * scale).round() as u32);
        let background = Snapshot::take(&small, (80.0, 32.0), scale);
        small.show_image_logo(image_logo(), (24.0 * scale).round() as u32);
        let shot = Snapshot::take(&small, (80.0, 32.0), scale);
        shot.assert_image_logo_is_exact(&background, &image_logo(), (4.0, 4.0), 24.0);
        small.show_builtin_logo(LogoPreset::Default);
        assert!(
            small.get_values_fit(),
            "small values elided at scale {scale}"
        );
        small.show_statistic(StatisticText {
            upload_speed: "888.88 MB/s8".into(),
            ..sample()
        });
        assert!(
            !small.get_values_fit(),
            "wider small value fits at scale {scale}"
        );
        small.show_statistic(sample());
    }
}
