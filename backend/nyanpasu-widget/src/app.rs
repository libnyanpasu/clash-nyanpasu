use std::time::Duration;

use nyanpasu_widget_protocol::{
    Message, StatisticMessage,
    ipc::{self, WidgetReceiver},
};
use slint::{EventLoopError, Weak};
use snafu::ResultExt as _;

use crate::{RunEventLoopSnafu, WidgetError, format::StatisticText, view::StatisticView};

/// How long the widget may take to close after [`Message::Stop`] before the
/// process exits anyway.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Shows the application's messages from `rx` in `view` and runs the event
/// loop until the application stops the widget or goes away.
pub(crate) fn run(view: impl StatisticView, rx: WidgetReceiver) -> Result<(), WidgetError> {
    view.show_statistic(StatisticText::from(&StatisticMessage::default()));
    let weak = view.as_weak();
    ipc::spawn_receiver(rx, move |message| {
        if let Err(error) = handle_message(&weak, message) {
            eprintln!("Failed to hand the message to the event loop: {error}");
        }
    });
    view.run().context(RunEventLoopSnafu)
}

/// Runs on the IPC receiver thread.
fn handle_message<V: StatisticView>(
    view: &Weak<V>,
    message: Message,
) -> Result<(), EventLoopError> {
    match message {
        Message::UpdateStatistic(statistic) => {
            let text = StatisticText::from(&statistic);
            view.upgrade_in_event_loop(move |view| view.show_statistic(text))
        }
        Message::UpdateLogo(preset) => {
            view.upgrade_in_event_loop(move |view| view.show_logo(preset))
        }
        Message::Stop => {
            std::thread::spawn(|| {
                std::thread::sleep(STOP_GRACE);
                std::process::exit(0);
            });
            slint::quit_event_loop()
        }
    }
}
