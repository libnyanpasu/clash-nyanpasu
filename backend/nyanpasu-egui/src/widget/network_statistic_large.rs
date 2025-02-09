#![allow(dead_code)]
use std::sync::{Arc, LazyLock};

use crate::{
    ipc::Message,
    utils::svg::{render_svg_with_current_color_replace, SvgExt},
    widget::get_window_state_path,
};
use eframe::{
    egui::{
        self, style::Selection, Color32, CornerRadius, Id, Image, Label, Layout, Margin, Sense,
        Stroke, Style, TextWrapMode, TextureOptions, Theme, ThemePreference, Vec2, ViewportCommand,
        Visuals,
    },
    epaint::CornerRadiusF32,
};
use parking_lot::RwLock;

// Presets
const STATUS_ICON_CONTAINER_WIDTH: f32 = 20.0;
const STATUS_ICON_WIDTH: f32 = 12.0;
const LOGO_CONTAINER_WIDTH: f32 = 44.0;
const LOGO_SIZE: Vec2 = Vec2::new(26.0, 31.0);

// Themes
const GLOBAL_ALPHA: u8 = 128;
const LIGHT_MODE_BACKGROUND_COLOR: Color32 =
    Color32::from_rgba_premultiplied(254, 247, 255, GLOBAL_ALPHA);
const DARK_MODE_TEXT_COLOR: Color32 = Color32::from_rgb(254, 247, 255);
const DARK_MODE_BACKGROUND_COLOR: Color32 =
    Color32::from_rgba_premultiplied(29, 27, 32, GLOBAL_ALPHA);
const DARK_MODE_STATUS_SHEET_COLOR: Color32 =
    Color32::from_rgba_premultiplied(73, 69, 79, GLOBAL_ALPHA);
const STATUS_ICON_CONTAINER_COLOR: Color32 = Color32::from_rgb(79, 55, 139);
static LOGO_CONTAINER_COLOR: LazyLock<Color32> =
    LazyLock::new(|| Color32::from_rgba_unmultiplied(79, 55, 139, GLOBAL_ALPHA));

// Icons
const DOWNLOAD_ICON: &[u8] = include_bytes!("../../assets/download.svg");
const UPLOAD_ICON: &[u8] = include_bytes!("../../assets/upload.svg");
const UP_ICON: &[u8] = include_bytes!("../../assets/up.svg");
const DOWN_ICON: &[u8] = include_bytes!("../../assets/down.svg");

fn setup_custom_style(ctx: &egui::Context) {
    ctx.style_mut_of(Theme::Light, use_light_green_accent);
    ctx.style_mut_of(Theme::Dark, use_dark_purple_accent);
    ctx.options_mut(|opts| {
        // set theme preference to dark, avoid system theme
        opts.theme_preference = ThemePreference::Dark;
    });
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
            font_id.size = 10.0;
        }
    }
    styles.spacing.window_margin = Margin::same(0);
    styles.spacing.item_spacing = Vec2::new(0.0, 0.0);
    styles.interaction.selectable_labels = false; // disable text selection
}

fn use_light_green_accent(style: &mut Style) {
    use_global_styles(style);
    style.visuals.override_text_color = Some(DARK_MODE_TEXT_COLOR);
    style.visuals.hyperlink_color = Color32::from_rgb(18, 180, 85);
    style.visuals.text_cursor.stroke.color = Color32::from_rgb(28, 92, 48);
    style.visuals.selection = Selection {
        bg_fill: Color32::from_rgb(157, 218, 169),
        stroke: Stroke::new(1.0, Color32::from_rgb(28, 92, 48)),
    };
}

fn use_dark_purple_accent(style: &mut Style) {
    use_global_styles(style);
    style.visuals.override_text_color = Some(DARK_MODE_TEXT_COLOR);
    style.visuals.hyperlink_color = Color32::from_rgb(202, 135, 227);
    style.visuals.text_cursor.stroke.color = Color32::from_rgb(234, 208, 244);
    style.visuals.selection = Selection {
        bg_fill: Color32::from_rgb(105, 67, 119),
        stroke: Stroke::new(1.0, Color32::from_rgb(234, 208, 244)),
    };
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogoPreset {
    #[default]
    Default,
    System,
    Tun,
}

#[derive(Debug)]
pub struct NyanpasuNetworkStatisticLargeWidgetInner {
    // data fields
    logo_preset: LogoPreset,
    download_total: u64,
    upload_total: u64,
    download_speed: u64,
    upload_speed: u64,

    // eframe ctx
    egui_ctx: egui::Context,
}

impl NyanpasuNetworkStatisticLargeWidgetInner {
    fn request_repaint(&self) {
        self.egui_ctx.request_repaint();
    }
}

#[derive(Debug, Clone)]
pub struct NyanpasuNetworkStatisticLargeWidget {
    inner: Arc<RwLock<NyanpasuNetworkStatisticLargeWidgetInner>>,
}

impl NyanpasuNetworkStatisticLargeWidget {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(Visuals::dark());
        setup_fonts(&cc.egui_ctx);
        setup_custom_style(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let rx = crate::ipc::setup_ipc_receiver_with_env().unwrap();
        let widget = NyanpasuNetworkStatisticLargeWidget {
            inner: Arc::new(RwLock::new(NyanpasuNetworkStatisticLargeWidgetInner {
                egui_ctx: cc.egui_ctx.clone(),
                logo_preset: LogoPreset::default(),
                download_total: 0,
                upload_total: 0,
                download_speed: 0,
                upload_speed: 0,
            })),
        };
        let this = widget.clone();
        std::thread::spawn(move || loop {
            match rx.recv() {
                Ok(msg) => {
                    println!("Received message: {:?}", msg);
                    let _ = this.handle_message(msg);
                }
                Err(e) => {
                    eprintln!("Failed to receive message: {}", e);
                    if matches!(
                        e,
                        ipc_channel::ipc::IpcError::Disconnected
                            | ipc_channel::ipc::IpcError::Io(_)
                    ) {
                        let _ = this.handle_message(Message::Stop);
                        break;
                    }
                }
            }
        });
        widget
    }

    pub fn run() -> eframe::Result {
        #[cfg(target_os = "macos")]
        super::set_application_activation_policy();

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([206.0, 60.0])
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_drag_and_drop(true)
                .with_resizable(false)
                .with_taskbar(false),
            run_and_return: false,
            // persist_window: true,
            // persistence_path: get_window_state_path().ok(),
            ..Default::default()
        };
        println!("Running widget...");
        eframe::run_native(
            "Nyanpasu Network Statistic Widget",
            options,
            Box::new(|cc| Ok(Box::new(NyanpasuNetworkStatisticLargeWidget::new(cc)))),
        )
    }

    pub fn handle_message(&self, msg: Message) -> anyhow::Result<()> {
        let mut this = self.inner.write();
        match msg {
            Message::UpdateStatistic(statistic) => {
                this.download_total = statistic.download_total;
                this.upload_total = statistic.upload_total;
                this.download_speed = statistic.download_speed;
                this.upload_speed = statistic.upload_speed;
                this.request_repaint();
            }
            Message::UpdateLogo(logo_preset) => {
                this.logo_preset = logo_preset;
                this.request_repaint();
            }
            Message::Stop => {
                std::thread::spawn(move || {
                    // wait for 5 seconds to ensure the widget is closed, or the app will be terminated
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    std::process::exit(0);
                });
                this.egui_ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        Ok(())
    }
}

impl eframe::App for NyanpasuNetworkStatisticLargeWidget {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let this = self.inner.read();
        let visuals = &ctx.style().visuals;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .corner_radius(CornerRadius::same(12))
                    .fill(if visuals.dark_mode { DARK_MODE_BACKGROUND_COLOR } else { LIGHT_MODE_BACKGROUND_COLOR })
                    .inner_margin(Margin::symmetric(9, 6)),
            )
            .show(ctx, |ui| {
                if ui.interact(ui.max_rect(), Id::new("window-drag"), Sense::drag()).dragged() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }

                let available_height = ui.available_height();
                ui.horizontal_centered(|ui| {
                    let width = ui.available_width();

                    // LOGO Column
                    ui.allocate_ui_with_layout(
                        egui::Vec2::new(LOGO_CONTAINER_WIDTH, LOGO_CONTAINER_WIDTH),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            egui::Frame::NONE.fill(*LOGO_CONTAINER_COLOR).corner_radius(CornerRadiusF32::same(LOGO_CONTAINER_WIDTH / 2.0)).show(ui, |ui| {
                                ui.centered_and_justified(|ui| {
                                    ui.add(Image::new(eframe::egui::include_image!("../../assets/tray-icon.png")).max_size(LOGO_SIZE));
                                });
                            });
                        },
                    );

                    let grid_gap = 7.0;

                    ui.add_space(grid_gap); // gap

                    let col_width = (width - LOGO_CONTAINER_WIDTH - grid_gap * 2.0) / 2.0;
                    let row_height = STATUS_ICON_CONTAINER_WIDTH;
                    let vertical_padding = LOGO_CONTAINER_WIDTH - row_height * 2.0;
                    let top_gap = (available_height - (row_height * 2.0 + vertical_padding)) / 2.0;

                    // Download Column
                    ui.allocate_ui_with_layout(egui::Vec2::new(col_width, available_height), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        ui.add_space(top_gap);
                        // Download Total
                        ui.allocate_ui_with_layout(egui::Vec2::new(col_width, row_height), Layout::left_to_right(egui::Align::Center), |ui| {
                            egui::Frame::NONE.corner_radius(CornerRadius::same(14)).fill(DARK_MODE_STATUS_SHEET_COLOR).show(ui, |ui| {
                                ui.allocate_ui(egui::Vec2::new(STATUS_ICON_CONTAINER_WIDTH, STATUS_ICON_CONTAINER_WIDTH), |ui| {
                                    egui::Frame::NONE
                                        .fill(STATUS_ICON_CONTAINER_COLOR)
                                        .corner_radius(CornerRadius::same(STATUS_ICON_WIDTH as u8))
                                        .show(ui, |ui| {
                                            let image = render_svg_with_current_color_replace(
                                                unsafe { String::from_utf8_unchecked(DOWNLOAD_ICON.to_vec()) }.as_str(),
                                                csscolorparser::parse(&DARK_MODE_TEXT_COLOR.to_hex()).unwrap(),
                                                (STATUS_ICON_WIDTH).round() as u32,
                                                (STATUS_ICON_WIDTH).round() as u32,
                                            )
                                            .unwrap()
                                            .into_wrapper()
                                            .into_egui_image();
                                            let texture_handle = ui.ctx().load_texture("download_icon", image, TextureOptions::default());
                                            ui.centered_and_justified(|ui| {
                                                ui.add(Image::from_texture(&texture_handle));
                                            });
                                        });
                                });
                                let width = ui.available_width();
                                let height = ui.available_height();
                                ui.allocate_ui_with_layout(egui::Vec2::new(width, height), Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                                    ui.add(
                                        Label::new(
                                            humansize::format_size(this.download_total, humansize::DECIMAL))
                                            .wrap_mode(TextWrapMode::Extend)
                                    );
                                });
                            });
                        });

                        ui.add_space(vertical_padding); // gap

                        // Download Speed
                        ui.allocate_ui_with_layout(egui::Vec2::new(col_width, row_height), Layout::left_to_right(egui::Align::Center), |ui| {
                            egui::Frame::NONE.corner_radius(CornerRadius::same(14)).fill(DARK_MODE_STATUS_SHEET_COLOR).show(ui, |ui| {
                                ui.allocate_ui(egui::Vec2::new(STATUS_ICON_CONTAINER_WIDTH, STATUS_ICON_CONTAINER_WIDTH), |ui| {
                                    egui::Frame::NONE
                                        .fill(STATUS_ICON_CONTAINER_COLOR)
                                        .corner_radius(CornerRadius::same(STATUS_ICON_WIDTH as u8))
                                        .show(ui, |ui| {
                                            let image = render_svg_with_current_color_replace(
                                                unsafe { String::from_utf8_unchecked(DOWN_ICON.to_vec()) }.as_str(),
                                                csscolorparser::parse(&DARK_MODE_TEXT_COLOR.to_hex()).unwrap(),
                                                (STATUS_ICON_WIDTH).round() as u32,
                                                (STATUS_ICON_WIDTH).round() as u32,
                                            )
                                            .unwrap()
                                            .into_wrapper()
                                            .into_egui_image();
                                            let texture_handle = ui.ctx().load_texture("down_icon", image, TextureOptions::default());
                                            ui.centered_and_justified(|ui| {
                                                ui.add(Image::from_texture(&texture_handle));
                                            });
                                        });
                                });
                                let width = ui.available_width();
                                let height = ui.available_height();
                                ui.allocate_ui_with_layout(egui::Vec2::new(width, height), Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                                    ui.add(Label::new(humansize::format_size(this.download_speed, humansize::DECIMAL.suffix("/s"))).wrap_mode(TextWrapMode::Extend));
                                });
                            });
                        })
                    });

                    ui.add_space(grid_gap); // gap

                    // Upload Column
                    ui.allocate_ui_with_layout(egui::Vec2::new(col_width, available_height), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        ui.add_space(top_gap);

                        // Upload Total
                        ui.allocate_ui_with_layout(egui::Vec2::new(col_width, row_height), Layout::left_to_right(egui::Align::Center), |ui| {
                            egui::Frame::NONE.corner_radius(CornerRadius::same(14)).fill(DARK_MODE_STATUS_SHEET_COLOR).show(ui, |ui| {
                                ui.allocate_ui(egui::Vec2::new(STATUS_ICON_CONTAINER_WIDTH, STATUS_ICON_CONTAINER_WIDTH), |ui| {
                                    egui::Frame::NONE
                                        .fill(STATUS_ICON_CONTAINER_COLOR)
                                        .corner_radius(CornerRadius::same(STATUS_ICON_WIDTH as u8))
                                        .show(ui, |ui| {
                                            let image = render_svg_with_current_color_replace(
                                                unsafe { String::from_utf8_unchecked(UPLOAD_ICON.to_vec()) }.as_str(),
                                                csscolorparser::parse(&DARK_MODE_TEXT_COLOR.to_hex()).unwrap(),
                                                (STATUS_ICON_WIDTH).round() as u32,
                                                (STATUS_ICON_WIDTH).round() as u32,
                                            )
                                            .unwrap()
                                            .into_wrapper()
                                            .into_egui_image();
                                            let texture_handle = ui.ctx().load_texture("upload_icon", image, TextureOptions::default());
                                            ui.centered_and_justified(|ui| {
                                                ui.add(Image::from_texture(&texture_handle));
                                            });
                                        });
                                });
                                let width = ui.available_width();
                                let height = ui.available_height();
                                ui.allocate_ui_with_layout(egui::Vec2::new(width, height), Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                                    ui.add(Label::new(humansize::format_size(this.upload_total, humansize::DECIMAL)).wrap_mode(TextWrapMode::Extend));
                                });
                            });
                        });

                        ui.add_space(vertical_padding); // gap

                        // Upload Speed
                        ui.allocate_ui_with_layout(egui::Vec2::new(col_width, row_height), Layout::left_to_right(egui::Align::Center), |ui| {
                            egui::Frame::NONE.corner_radius(CornerRadius::same(14)).fill(DARK_MODE_STATUS_SHEET_COLOR).show(ui, |ui| {
                                ui.allocate_ui(egui::Vec2::new(STATUS_ICON_CONTAINER_WIDTH, STATUS_ICON_CONTAINER_WIDTH), |ui| {
                                    egui::Frame::NONE
                                        .fill(STATUS_ICON_CONTAINER_COLOR)
                                        .corner_radius(CornerRadius::same(STATUS_ICON_WIDTH as u8))
                                        .show(ui, |ui| {
                                            let image = render_svg_with_current_color_replace(
                                                unsafe { String::from_utf8_unchecked(UP_ICON.to_vec()) }.as_str(),
                                                csscolorparser::parse(&DARK_MODE_TEXT_COLOR.to_hex()).unwrap(),
                                                (STATUS_ICON_WIDTH).round() as u32,
                                                (STATUS_ICON_WIDTH).round() as u32,
                                            )
                                            .unwrap()
                                            .into_wrapper()
                                            .into_egui_image();
                                            let texture_handle = ui.ctx().load_texture("up_icon", image, TextureOptions::default());
                                            ui.centered_and_justified(|ui| {
                                                ui.add(Image::from_texture(&texture_handle));
                                            });
                                        });
                                });
                                let width = ui.available_width();
                                let height = ui.available_height();
                                ui.allocate_ui_with_layout(egui::Vec2::new(width, height), Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                                    ui.add(Label::new(humansize::format_size(this.upload_speed, humansize::DECIMAL.suffix("/s"))).wrap_mode(TextWrapMode::Extend));
                                });
                            });
                        })
                    });
                });
            });
    }
}
