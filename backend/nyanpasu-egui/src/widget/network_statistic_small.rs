#![allow(dead_code)]
use std::sync::{Arc, LazyLock};

use eframe::egui::{
    self, Color32, CornerRadius, Id, Image, Label, Layout, Margin, RichText, Sense, Stroke, Style,
    TextWrapMode, Theme, Vec2, ViewportCommand, Visuals, WidgetText, include_image,
    style::Selection,
};
use parking_lot::RwLock;

use crate::{ipc::Message, widget::get_window_state_path};

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
    styles.spacing.window_margin = Margin::same(0);
    styles.spacing.item_spacing = Vec2::new(0.0, 0.0);
    styles.interaction.selectable_labels = false;
}

fn use_light_green_accent(style: &mut Style) {
    use_global_styles(style);
    style.visuals.override_text_color = Some(LIGHT_MODE_TEXT_COLOR);
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

#[derive(Clone)]
pub struct NyanpasuNetworkStatisticSmallWidget {
    state: Arc<RwLock<NyanpasuNetworkStatisticSmallWidgetState>>,
}

struct NyanpasuNetworkStatisticSmallWidgetState {
    // data fields
    // download_total: u64,
    // upload_total: u64,
    download_speed: u64,
    upload_speed: u64,

    // eframe ctx
    egui_ctx: egui::Context,
}

impl NyanpasuNetworkStatisticSmallWidgetState {
    fn request_repaint(&self) {
        self.egui_ctx.request_repaint();
    }
}

impl NyanpasuNetworkStatisticSmallWidget {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_custom_style(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let rx = crate::ipc::setup_ipc_receiver_with_env().unwrap();
        let widget = Self {
            state: Arc::new(RwLock::new(NyanpasuNetworkStatisticSmallWidgetState {
                egui_ctx: cc.egui_ctx.clone(),
                download_speed: 0,
                upload_speed: 0,
            })),
        };
        let this = widget.clone();
        std::thread::spawn(move || {
            loop {
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
            }
        });
        widget
    }

    pub fn run() -> eframe::Result {
        #[cfg(target_os = "macos")]
        super::set_application_activation_policy();

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([80.0, 32.0])
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_drag_and_drop(true)
                .with_resizable(false)
                .with_taskbar(false),
            run_and_return: false,
            // TODO: buggy feature, and should we manually save the window state
            // persist_window: true,
            // persistence_path: get_window_state_path().ok(),
            ..Default::default()
        };
        println!("Running widget...");
        eframe::run_native(
            "Nyanpasu Network Statistic Widget",
            options,
            Box::new(|cc| Ok(Box::new(NyanpasuNetworkStatisticSmallWidget::new(cc)))),
        )
    }

    pub fn handle_message(&self, msg: Message) -> anyhow::Result<()> {
        let mut this = self.state.write();
        match msg {
            Message::UpdateStatistic(statistic) => {
                // this.download_total = statistic.download_total;
                // this.upload_total = statistic.upload_total;
                this.download_speed = statistic.download_speed;
                this.upload_speed = statistic.upload_speed;
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
            _ => {
                eprintln!("Unsupported message: {:?}", msg);
            }
        }
        Ok(())
    }
}

impl eframe::App for NyanpasuNetworkStatisticSmallWidget {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let visuals = &ctx.style().visuals;
        let this = self.state.read();

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .corner_radius(CornerRadius::same(40))
                    .fill(if visuals.dark_mode {
                        DARK_MODE_BACKGROUND_COLOR
                    } else {
                        LIGHT_MODE_BACKGROUND_COLOR
                    })
                    .inner_margin(Margin::same(4)),
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
                        egui::Frame::NONE
                            .corner_radius(CornerRadius::same(12))
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
                            Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.add(
                                    Label::new(
                                        RichText::new(humansize::format_size(
                                            this.upload_speed,
                                            humansize::DECIMAL.suffix("/s"),
                                        ))
                                        .size(8.0),
                                    )
                                    .selectable(false)
                                    .wrap_mode(TextWrapMode::Extend),
                                );
                            },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, height),
                            Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.add(
                                    Label::new(WidgetText::from(
                                        RichText::new(humansize::format_size(
                                            this.download_speed,
                                            humansize::DECIMAL.suffix("/s"),
                                        ))
                                        .size(8.0),
                                    ))
                                    .selectable(false)
                                    .wrap_mode(TextWrapMode::Extend),
                                );
                            },
                        );
                    });
                })
            });
    }
}
