// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use nyanpasu_widget_protocol::StatisticWidgetVariant;

/// Shows the network statistic widget for the application that started it.
#[derive(Parser)]
struct Args {
    /// Which widget to show.
    #[arg(value_enum)]
    variant: StatisticWidgetVariant,
}

fn main() -> Result<(), nyanpasu_widget::WidgetError> {
    nyanpasu_widget::start_statistic_widget(Args::parse().variant)
}
