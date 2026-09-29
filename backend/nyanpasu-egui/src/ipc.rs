pub use ipc_channel::ipc::IpcSender;
use ipc_channel::ipc::{self, IpcReceiver};
use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::widget::network_statistic_large::LogoPreset;

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct StatisticMessage {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum Message {
    Stop,
    UpdateStatistic(StatisticMessage),
    UpdateLogo(LogoPreset),
}

/// Why the parent's end of the widget channel could not be set up or used.
#[derive(Debug, Snafu)]
pub enum WidgetIpcError {
    #[snafu(display("the IPC server has already accepted a connection"))]
    AlreadyAccepted,
    #[snafu(display("could not create the IPC server"))]
    CreateServer { source: std::io::Error },
    #[snafu(display("the widget did not connect to the IPC server"))]
    AcceptWidget { source: ipc_channel::IpcError },
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
    let oneshot_sender: IpcSender<IpcSender<Message>> =
        ipc::IpcSender::connect(name.to_string()).context(ConnectServerSnafu { name })?;
    let (tx, _rx) = ipc::channel().context(CreateChannelSnafu)?;
    oneshot_sender.send(tx).context(SendHandshakeSnafu)?;
    Ok(())
}

/// Sends `message` to the widget.
pub fn send_message(sender: &IpcSender<Message>, message: Message) -> Result<(), WidgetIpcError> {
    sender.send(message).context(SendMessageSnafu)
}

pub(crate) fn setup_ipc_receiver(name: &str) -> anyhow::Result<IpcReceiver<Message>> {
    let oneshot_sender: IpcSender<IpcSender<Message>> = ipc::IpcSender::connect(name.to_string())?;
    let (tx, rx) = ipc::channel()?;
    oneshot_sender.send(tx)?;
    Ok(rx)
}

pub(crate) fn setup_ipc_receiver_with_env() -> anyhow::Result<IpcReceiver<Message>> {
    let name = std::env::var("NYANPASU_EGUI_IPC_SERVER")?;
    setup_ipc_receiver(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
