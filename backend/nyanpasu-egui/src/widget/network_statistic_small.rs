use std::sync::Arc;
use std::sync::LazyLock;

use eframe::egui::{
    self, include_image, style::Selection, Color32, Id, Image, Layout, Margin, RichText, Rounding,
    Sense, Stroke, Style, Theme, Vec2, ViewportCommand, Visuals, WidgetText,
};

// Presets
const STATUS_ICON_CONTAINER_WIDTH: f32 = 20.0;
const LOGO_CONTAINER_WIDTH: f32 = 44.0;
const LOGO_SIZE: Vec2 = Vec2::new(26.0, 31.0);

// Themes
const GLOBAL_ALPHA: u8 = 128;
const LIGHT_MODE_BACKGROUND_COLOR: Color32 = Color32::from_rgb(234, 221, 255);
const LIGHT_MODE_TEXT_COLOR: Color32 = Color32::from_rgb(29, 27, 32);
const DARK_MODE_TEXT_COLOR: Color32 = Color32::from_rgb(254, 247, 255);
const DARK_MODE_BACKGROUND_COLOR: Color32 = Color32::from_rgb(29, 27, 32);
const DARK_MODE_STATUS_SHEET_COLOR: Color32 = Color32::from_rgb(73, 69, 79);
const STATUS_ICON_CONTAINER_COLOR: Color32 = Color32::from_rgb(79, 55, 139);
static LOGO_CONTAINER_COLOR: LazyLock<Color32> =
    LazyLock::new(|| Color32::from_rgba_unmultiplied(79, 55, 139, GLOBAL_ALPHA));

// Icons
const UP_ICON: &[u8] = include_bytes!("../../assets/up.svg");
const DOWN_ICON: &[u8] = include_bytes!("../../assets/down.svg");

fn setup_custom_style(ctx: &egui::Context) {
    ctx.style_mut(use_global_styles);
    ctx.style_mut_of(Theme::Light, use_light_green_accent);
    ctx.style_mut_of(Theme::Dark, use_dark_purple_accent);
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "Inter".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/Inter-Regular.ttf"
        ))),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".to_owned());

    ctx.set_fonts(fonts);
}

fn use_global_styles(styles: &mut Style) {
    for (text_style, font_id) in styles.text_styles.iter_mut() {
        if matches!(text_style, egui::TextStyle::Body) {
            font_id.size = 8.0;
        }
    }
    styles.spacing.window_margin = Margin::same(0.0);
    styles.spacing.item_spacing = Vec2::new(0.0, 0.0);
    styles.interaction.selectable_labels = false;
}

fn use_light_green_accent(style: &mut Style) {
    style.visuals.override_text_color = Some(LIGHT_MODE_TEXT_COLOR);
    style.visuals.hyperlink_color = Color32::from_rgb(18, 180, 85);
    style.visuals.text_cursor.stroke.color = Color32::from_rgb(28, 92, 48);
    style.visuals.selection = Selection {
        bg_fill: Color32::from_rgb(157, 218, 169),
        stroke: Stroke::new(1.0, Color32::from_rgb(28, 92, 48)),
    };
}

fn use_dark_purple_accent(style: &mut Style) {
    style.visuals.override_text_color = Some(DARK_MODE_TEXT_COLOR);
    style.visuals.hyperlink_color = Color32::from_rgb(202, 135, 227);
    style.visuals.text_cursor.stroke.color = Color32::from_rgb(234, 208, 244);
    style.visuals.selection = Selection {
        bg_fill: Color32::from_rgb(105, 67, 119),
        stroke: Stroke::new(1.0, Color32::from_rgb(234, 208, 244)),
    };
}

pub struct NyanpasuNetworkStatisticSmallWidget {
    demo_size: u64,
}

impl NyanpasuNetworkStatisticSmallWidget {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(Visuals::light());
        setup_fonts(&cc.egui_ctx);
        setup_custom_style(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self {
            demo_size: 100_000_000,
        }
    }
}

impl eframe::App for NyanpasuNetworkStatisticSmallWidget {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let visuals = &ctx.style().visuals;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .rounding(Rounding::same(40.0))
                    .fill(if visuals.dark_mode {
                        DARK_MODE_BACKGROUND_COLOR
                    } else {
                        LIGHT_MODE_BACKGROUND_COLOR
                    })
                    .inner_margin(Margin::same(4.0)),
            )
            .show(ctx, |ui| {
                if ui
                    .interact(ui.max_rect(), Id::new("window-drag"), Sense::drag())
                    .dragged()
                {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                ui.horizontal(|ui| {
                    ui.allocate_ui(Vec2::new(24.0, 24.0), |ui| {
                        egui::Frame::none()
                            .rounding(Rounding::same(12.0))
                            .fill(*LOGO_CONTAINER_COLOR)
                            .show(ui, |ui| {
                                ui.allocate_ui_with_layout(
                                    Vec2::new(24.0, 24.0),
                                    Layout::centered_and_justified(egui::Direction::TopDown),
                                    |ui| {
                                        ui.add(
                                            Image::new(include_image!(
                                                "../../assets/tray-icon.png"
                                            ))
                                            .max_size(Vec2::new(9.84, 13.78)),
                                        )
                                    },
                                )
                            });
                    });

                    ui.add_space(1.0);
                    ui.vertical(|ui| {
                        let width = ui.available_width();
                        let height = ui.available_height() / 2.0;
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, height),
                            Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    WidgetText::from(RichText::new(format!(
                                        "{}/s",
                                        humansize::format_size(self.demo_size, humansize::DECIMAL)
                                    )))
                                    .color(LIGHT_MODE_TEXT_COLOR),
                                );
                            },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, height),
                            Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    WidgetText::from(RichText::new(format!(
                                        "{}/s",
                                        humansize::format_size(self.demo_size, humansize::DECIMAL)
                                    )))
                                    .color(LIGHT_MODE_TEXT_COLOR),
                                );
                            },
                        );
                    });
                })
            });
    }
}
