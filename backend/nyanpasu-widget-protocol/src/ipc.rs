//! The channel between the application and a widget process, over
//! [`nyanpasu_local_channel`]. The application binds the server and passes its
//! [`Bootstrap`] to the widget in [`Bootstrap::ENV`]; the widget connects and
//! only receives.

pub use nyanpasu_local_channel::{Bootstrap, Never};
use nyanpasu_local_channel::{BootstrapError, ConnectError, RecvError, blocking};
use snafu::{ResultExt as _, Snafu};

use crate::Message;

/// What the widget receives the application's messages on.
pub type WidgetReceiver = blocking::Receiver<Message>;

/// Why an end of the widget channel could not be set up or used.
#[derive(Debug, Snafu)]
pub enum WidgetIpcError {
    #[snafu(display("could not read the channel bootstrap from {}", Bootstrap::ENV))]
    ReadBootstrap { source: BootstrapError },
    #[snafu(display("could not connect to the application"))]
    ConnectApplication { source: ConnectError },
    #[cfg(feature = "ipc-host")]
    #[snafu(display("could not create the widget channel"))]
    CreateServer {
        source: nyanpasu_local_channel::ServerError,
    },
    #[cfg(feature = "ipc-host")]
    #[snafu(display("the widget did not connect to the channel"))]
    AcceptWidget {
        source: nyanpasu_local_channel::AcceptError,
    },
    #[cfg(feature = "ipc-host")]
    #[snafu(display("could not send the message to the widget"))]
    SendMessage {
        source: nyanpasu_local_channel::SendError,
    },
}

/// Connects the widget to the application whose server `bootstrap` names.
pub fn connect(bootstrap: &Bootstrap) -> Result<WidgetReceiver, WidgetIpcError> {
    // The widget never sends, so its sending half is dropped; the connection
    // stays open for as long as the receiver lives.
    let (_, rx) =
        blocking::connect::<Never, Message>(bootstrap).context(ConnectApplicationSnafu)?;
    Ok(rx)
}

/// Connects the widget to the application that named its server in
/// [`Bootstrap::ENV`].
pub fn connect_with_env() -> Result<WidgetReceiver, WidgetIpcError> {
    connect(&Bootstrap::from_env().context(ReadBootstrapSnafu)?)
}

/// Hands each message from `rx` to `handle` on a new thread. A channel the
/// application closed or broke hands over [`Message::Stop`] and ends the
/// thread; a message that does not decode is only reported.
pub fn spawn_receiver(mut rx: WidgetReceiver, mut handle: impl FnMut(Message) + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            match rx.recv() {
                Ok(msg) => {
                    println!("Received message: {msg:?}");
                    handle(msg);
                }
                Err(error @ RecvError::Decode { .. }) => {
                    eprintln!("Failed to decode a message: {error}");
                }
                Err(error) => {
                    if !matches!(error, RecvError::Disconnected) {
                        eprintln!("Failed to receive message: {error}");
                    }
                    handle(Message::Stop);
                    break;
                }
            }
        }
    });
}

#[cfg(feature = "ipc-host")]
pub use self::host::*;

/// The application's end.
#[cfg(feature = "ipc-host")]
mod host {
    use nyanpasu_local_channel::{Never, tokio};
    use snafu::ResultExt as _;

    use super::{
        AcceptWidgetSnafu, Bootstrap, CreateServerSnafu, SendMessageSnafu, WidgetIpcError,
    };
    use crate::Message;

    /// The server one widget connects to, once.
    pub type WidgetServer = tokio::OneShotServer<Message, Never>;
    /// What the application sends the widget messages on.
    pub type WidgetSender = tokio::Sender<Message>;

    /// Starts listening for one widget. Call it inside a Tokio runtime and
    /// pass the [`Bootstrap`] to the widget in [`Bootstrap::ENV`].
    pub fn create_server() -> Result<(WidgetServer, Bootstrap), WidgetIpcError> {
        WidgetServer::new("nyanpasu-widget").context(CreateServerSnafu)
    }

    /// Waits for the widget to connect. Dropping the future stops listening.
    pub async fn accept(server: WidgetServer) -> Result<WidgetSender, WidgetIpcError> {
        // Nothing comes back from the widget, so the receiving half goes.
        let (tx, _) = server.accept().await.context(AcceptWidgetSnafu)?;
        Ok(tx)
    }

    /// Sends `message` to the widget.
    pub async fn send_message(
        sender: &WidgetSender,
        message: Message,
    ) -> Result<(), WidgetIpcError> {
        sender.send(&message).await.context(SendMessageSnafu)
    }
}

#[cfg(all(test, feature = "ipc-host"))]
mod tests {
    use super::*;
    use crate::{Logo, LogoPreset};

    /// Runs the application's end in process and the widget's end on a
    /// thread, as the widget does, and returns what reached the widget. The
    /// tests block on that, so they run on a multi-threaded runtime, which
    /// keeps the channel's writer task going meanwhile.
    async fn widget_side(
        send: impl AsyncFnOnce(WidgetSender),
    ) -> std::sync::mpsc::Receiver<Message> {
        let (server, bootstrap) = create_server().unwrap();
        let (handled_tx, handled_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rx = connect(&bootstrap).unwrap();
            spawn_receiver(rx, move |message| handled_tx.send(message).unwrap());
        });
        let tx = accept(server).await.unwrap();
        send(tx).await;
        handled_rx
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn the_widget_receives_messages_until_stop() {
        let handled = widget_side(async |tx| {
            send_message(&tx, Message::UpdateLogo(Logo::Builtin(LogoPreset::Tun)))
                .await
                .unwrap();
            send_message(&tx, Message::Stop).await.unwrap();
            // The widget stops on the message and on the close that follows.
        })
        .await;

        assert!(matches!(
            handled.recv().unwrap(),
            Message::UpdateLogo(Logo::Builtin(LogoPreset::Tun))
        ));
        assert!(matches!(handled.recv().unwrap(), Message::Stop));
        assert!(matches!(handled.recv().unwrap(), Message::Stop));
        // The thread ended, dropping the handler and its sender.
        assert!(handled.recv().is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn receiver_hands_over_stop_once_the_application_disconnects() {
        let handled = widget_side(async |tx| {
            send_message(&tx, Message::UpdateLogo(Logo::Builtin(LogoPreset::System)))
                .await
                .unwrap();
            drop(tx);
        })
        .await;

        assert!(matches!(
            handled.recv().unwrap(),
            Message::UpdateLogo(Logo::Builtin(LogoPreset::System))
        ));
        assert!(matches!(handled.recv().unwrap(), Message::Stop));
        assert!(handled.recv().is_err());
    }
}
