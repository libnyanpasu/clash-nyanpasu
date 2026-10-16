use ipc_channel::ipc;
pub use ipc_channel::ipc::{IpcReceiver, IpcSender};
use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::Message;

/// The environment variable through which the application tells a widget
/// process the name of the one-shot server to connect to. The name is part of
/// the contract with already built widget binaries, so it keeps its egui-era
/// value.
pub const IPC_SERVER_ENV: &str = "NYANPASU_EGUI_IPC_SERVER";

/// Why an end of the widget channel could not be set up or used.
#[derive(Debug, Snafu)]
pub enum WidgetIpcError {
    #[snafu(display("the IPC server has already accepted a connection"))]
    AlreadyAccepted,
    #[snafu(display("could not create the IPC server"))]
    CreateServer { source: std::io::Error },
    #[snafu(display("the widget did not connect to the IPC server"))]
    AcceptWidget { source: ipc_channel::IpcError },
    #[snafu(display("could not read the IPC server name from {IPC_SERVER_ENV}"))]
    ReadServerName { source: std::env::VarError },
    #[snafu(display("could not connect to the IPC server {name}"))]
    ConnectServer {
        name: String,
        source: std::io::Error,
    },
    #[snafu(display("could not create the IPC channel"))]
    CreateChannel { source: std::io::Error },
    #[snafu(display("could not send the handshake"))]
    SendHandshake { source: ipc_channel::IpcError },
    #[snafu(display("could not send the message to the widget"))]
    SendMessage { source: ipc_channel::IpcError },
}

pub struct IPCServer {
    oneshot_server: Option<ipc::IpcOneShotServer<IpcSender<Message>>>,
    tx: Option<IpcSender<Message>>,
}

impl IPCServer {
    pub fn is_connected(&self) -> bool {
        self.tx.is_some()
    }

    pub fn connect(&mut self) -> Result<(), WidgetIpcError> {
        let server = self.oneshot_server.take().context(AlreadyAcceptedSnafu)?;
        let (_, tx) = server.accept().context(AcceptWidgetSnafu)?;
        self.tx = Some(tx);
        Ok(())
    }

    pub fn into_tx(self) -> Option<IpcSender<Message>> {
        self.tx
    }
}

pub fn create_ipc_server() -> Result<(IPCServer, String), WidgetIpcError> {
    let (oneshot_server, oneshot_server_name) =
        ipc::IpcOneShotServer::new().context(CreateServerSnafu)?;
    Ok((
        IPCServer {
            oneshot_server: Some(oneshot_server),
            tx: None,
        },
        oneshot_server_name,
    ))
}

/// Connects to a pending one-shot server as its client, the way the widget
/// does, so a handshake blocked in `accept` returns. The sender it hands over
/// leads nowhere: this is only for a parent that stopped waiting for a widget
/// which never connected.
pub fn release_server(name: &str) -> Result<(), WidgetIpcError> {
    setup_ipc_receiver(name).map(drop)
}

/// Sends `message` to the widget.
pub fn send_message(sender: &IpcSender<Message>, message: Message) -> Result<(), WidgetIpcError> {
    sender.send(message).context(SendMessageSnafu)
}

fn setup_ipc_receiver(name: &str) -> Result<IpcReceiver<Message>, WidgetIpcError> {
    let oneshot_sender: IpcSender<IpcSender<Message>> =
        ipc::IpcSender::connect(name.to_string()).context(ConnectServerSnafu { name })?;
    let (tx, rx) = ipc::channel().context(CreateChannelSnafu)?;
    oneshot_sender.send(tx).context(SendHandshakeSnafu)?;
    Ok(rx)
}

/// Connects the widget to the server the application named in
/// [`IPC_SERVER_ENV`], returning where the application's messages
/// arrive.
pub fn setup_ipc_receiver_with_env() -> Result<IpcReceiver<Message>, WidgetIpcError> {
    let name = std::env::var(IPC_SERVER_ENV).context(ReadServerNameSnafu)?;
    setup_ipc_receiver(&name)
}

/// Hands each message from `rx` to `handle` on a new thread. A channel the
/// application closed or broke hands over [`Message::Stop`] and ends the
/// thread; other receive errors are only reported.
pub fn spawn_receiver(rx: IpcReceiver<Message>, mut handle: impl FnMut(Message) + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            match rx.recv() {
                Ok(msg) => {
                    println!("Received message: {msg:?}");
                    handle(msg);
                }
                Err(e) => {
                    eprintln!("Failed to receive message: {e}");
                    if matches!(
                        e,
                        ipc_channel::IpcError::Disconnected | ipc_channel::IpcError::Io(_)
                    ) {
                        handle(Message::Stop);
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogoPreset;

    #[test]
    fn release_server_unblocks_a_pending_accept() {
        let (mut server, name) = create_ipc_server().unwrap();
        let accept = std::thread::spawn(move || {
            server.connect().unwrap();
            server.is_connected()
        });

        release_server(&name).unwrap();

        assert!(accept.join().unwrap(), "accept returned with a sender");
    }

    #[test]
    fn receiver_hands_over_stop_once_the_application_disconnects() {
        let (tx, rx) = ipc::channel().unwrap();
        let (handled_tx, handled_rx) = std::sync::mpsc::channel();
        spawn_receiver(rx, move |message| handled_tx.send(message).unwrap());

        send_message(&tx, Message::UpdateLogo(LogoPreset::Tun)).unwrap();
        drop(tx);

        assert!(matches!(
            handled_rx.recv().unwrap(),
            Message::UpdateLogo(LogoPreset::Tun)
        ));
        assert!(matches!(handled_rx.recv().unwrap(), Message::Stop));
        // The thread ended, dropping the handler and its sender.
        assert!(handled_rx.recv().is_err());
    }
}
