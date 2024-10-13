use std::sync::Arc;

use futures_util::StreamExt;
use parking_lot::{Mutex, RwLock};
use serde::Deserialize;
use tokio::sync::mpsc::Receiver;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tracing::instrument]
async fn connect_clash_server<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
    url: &str,
) -> anyhow::Result<Receiver<T>> {
    let (stream, _) = connect_async(url).await?;
    let (_, mut read) = stream.split();
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    tokio::spawn(async move {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => match serde_json::from_str(&text) {
                    Ok(data) => {
                        let _ = tx.send(data).await;
                    }
                    Err(e) => {
                        tracing::error!("failed to deserialize json: {}", e);
                    }
                },
                Ok(Message::Binary(bin)) => match serde_json::from_slice(&bin) {
                    Ok(data) => {
                        let _ = tx.send(data).await;
                    }
                    Err(e) => {
                        tracing::error!("failed to deserialize json: {}", e);
                    }
                },
                Ok(Message::Close(_)) => {
                    tracing::info!("server closed connection");
                    break;
                }
                Err(e) => {
                    tracing::error!("failed to read message: {}", e);
                }
                _ => {}
            }
        }
    });
    Ok(rx)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClashConnectionsMessage {
    download_total: u64,
    upload_total: u64,
    // other fields are omitted
}

pub struct ClashConnectionsConnector {
    receiver: Option<Receiver<ClashConnectionsMessage>>,
    subscriptions: Vec<Box<dyn Fn(ClashConnectionsConnector) + Sync + Send + 'static>>,
    info: Arc<RwLock<ClashConnectionsInfo>>,
}

struct ClashConnectionsInfo {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
}


impl ClashConnectionsConnector {
    pub fn url() -> String {
        let (server, port, secret) = {
            let info = crate::Config::clash().data().get_client_info();
            (info.server, info.port, info.secret)
        };
        let mut url = format!("ws://{}:{}/connections", server, port);
        if let Some(secret) = secret {
            url.push_str(&format!("?secret={}", urlencoding::encode(&secret)));
        }
        url
    }

    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            receiver: None,
            download_total: 0,
            upload_total: 0,
            download_speed: 0,
            upload_speed: 0,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.receiver.as_ref().is_some_and(|rx| rx.is_closed())
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        let rx = connect_clash_server::<ClashConnectionsMessage>(&Self::url()).await?;
        self.receiver = Some(rx);
        let this = Arc
        Ok(())
    }

    pub async fn update(this: Arc<Mutex<Self>>) {
        while let Some(message) = self.receiver.try_recv().await {
            self.download_speed = message.download_total - self.download_total;
            self.upload_speed = message.upload_total - self.upload_total;
            self.download_total = message.download_total;
            self.upload_total = message.upload_total;
        }
    }
}
