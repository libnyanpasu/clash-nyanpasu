pub use ipc_channel::ipc::IpcSender;
use ipc_channel::ipc::{self, IpcReceiver};

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

pub struct IPCServer {
    oneshot_server: Option<ipc::IpcOneShotServer<IpcSender<Message>>>,
    tx: Option<IpcSender<Message>>,
}

impl IPCServer {
    pub fn is_connected(&self) -> bool {
        self.tx.is_some()
    }

    pub fn connect(&mut self) -> anyhow::Result<()> {
        if self.oneshot_server.is_none() {
            anyhow::bail!("IPC server is already initialized");
        }

        let (_, tx) = self.oneshot_server.take().unwrap().accept()?;
        self.tx = Some(tx);
        Ok(())
    }

    pub fn into_tx(self) -> Option<IpcSender<Message>> {
        self.tx
    }
}

pub fn create_ipc_server() -> anyhow::Result<(IPCServer, String)> {
    let (oneshot_server, oneshot_server_name) = ipc::IpcOneShotServer::new()?;
    Ok((
        IPCServer {
            oneshot_server: Some(oneshot_server),
            tx: None,
        },
        oneshot_server_name,
    ))
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
