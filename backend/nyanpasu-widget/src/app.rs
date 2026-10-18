use std::time::Duration;

use i_slint_backend_winit::{EventResult, WinitWindowAccessor as _, winit::event::WindowEvent};
use nyanpasu_widget_protocol::{
    Logo, LogoPreset, Message, StatisticMessage,
    ipc::{self, WidgetReceiver},
};
use slint::{EventLoopError, Weak};
use snafu::ResultExt as _;

use crate::{RunEventLoopSnafu, WidgetError, format::StatisticText, logo, view::StatisticView};

/// How long the widget may take to close after [`Message::Stop`] before the
/// process exits anyway.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Shows the application's messages from `rx` in `view` and runs the event
/// loop until the application stops the widget or goes away. `logo_size` is
/// the logical size of the window's logo badge.
pub(crate) fn run(
    view: impl StatisticView,
    rx: WidgetReceiver,
    logo_size: f32,
) -> Result<(), WidgetError> {
    view.show_statistic(StatisticText::from(&StatisticMessage::default()));
    let weak = view.as_weak();
    view.window().on_winit_window_event(move |_, event| {
        if let WindowEvent::ScaleFactorChanged { scale_factor, .. } = event
            && let Some(view) = weak.upgrade()
        {
            view.rescale_image_logo(logo_pixels(logo_size, *scale_factor as f32));
        }
        EventResult::Propagate
    });
    let weak = view.as_weak();
    ipc::spawn_receiver(rx, move |message| {
        if let Err(error) = handle_message(&weak, message, logo_size) {
            eprintln!("Failed to hand the message to the event loop: {error}");
        }
    });
    view.run().context(RunEventLoopSnafu)
}

/// The physical size of a `logo_size` logical pixel badge.
fn logo_pixels(logo_size: f32, scale_factor: f32) -> u32 {
    (logo_size * scale_factor).round() as u32
}

/// Runs on the IPC receiver thread, which also decodes image logos, so the
/// event loop only rasterizes them.
fn handle_message<V: StatisticView>(
    view: &Weak<V>,
    message: Message,
    logo_size: f32,
) -> Result<(), EventLoopError> {
    match message {
        Message::UpdateStatistic(statistic) => {
            let text = StatisticText::from(&statistic);
            view.upgrade_in_event_loop(move |view| view.show_statistic(text))
        }
        Message::UpdateLogo(Logo::Builtin(preset)) => {
            view.upgrade_in_event_loop(move |view| view.show_builtin_logo(preset))
        }
        Message::UpdateLogo(Logo::Image(bytes)) => match logo::decode(&bytes) {
            Ok(source) => view.upgrade_in_event_loop(move |view| {
                let size = logo_pixels(logo_size, view.window().scale_factor());
                view.show_image_logo(source, size);
            }),
            Err(error) => {
                eprintln!("Failed to load the logo, showing the bundled one: {error}");
                view.upgrade_in_event_loop(|view| view.show_builtin_logo(LogoPreset::Default))
            }
        },
        Message::Stop => {
            std::thread::spawn(|| {
                std::thread::sleep(STOP_GRACE);
                std::process::exit(0);
            });
            slint::quit_event_loop()
        }
    }
}
