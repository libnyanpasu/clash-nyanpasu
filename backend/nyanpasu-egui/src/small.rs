#![allow(dead_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example

use eframe::egui;
use nyanpasu_egui::widget::NyanpasuNetworkStatisticSmallWidget;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([80.0, 32.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_drag_and_drop(true)
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "egui example: custom style",
        options,
        Box::new(|cc| Ok(Box::new(NyanpasuNetworkStatisticSmallWidget::new(cc)))),
    )
}
