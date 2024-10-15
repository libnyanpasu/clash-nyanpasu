use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::mpsc::Receiver;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::log_err;

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

struct ClashConnectionsConnectorInner {
    is_connected: AtomicBool,
    stop_signal: AtomicBool,
    subscriptions: Mutex<Vec<Box<dyn Fn(ClashConnectionsInfoMessage) + Sync + Send + 'static>>>,
    info: ClashConnectionsInfo,
}

#[derive(Clone)]
struct ClashConnectionsConnector {
    inner: Arc<ClashConnectionsConnectorInner>,
}

#[derive(Debug, Default)]
struct ClashConnectionsInfo {
    pub download_total: AtomicU64,
    pub upload_total: AtomicU64,
    pub download_speed: AtomicU64,
    pub upload_speed: AtomicU64,
}

#[derive(Debug, Clone, Default, Copy)]
pub struct ClashConnectionsInfoMessage {
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

    pub fn new() -> Self {
        Self {
            inner: Arc::new(ClashConnectionsConnectorInner {
                is_connected: AtomicBool::new(false),
                stop_signal: AtomicBool::new(false),
                subscriptions: Mutex::new(Vec::new()),
                info: ClashConnectionsInfo::default(),
            }),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected.load(Ordering::Acquire)
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let this = self.clone();
        let mut rx = connect_clash_server::<ClashConnectionsMessage>(&Self::url()).await?;
        self.inner.is_connected.store(true, Ordering::Release);
        tokio::spawn(async move {
            loop {
                if this.inner.stop_signal.load(Ordering::Acquire) {
                    break; // drop connection
                }
                match rx.recv().await {
                    Some(msg) => {
                        this.update(msg);
                        let msg = ClashConnectionsInfoMessage {
                            download_total: this.inner.info.download_total.load(Ordering::Acquire),
                            upload_total: this.inner.info.upload_total.load(Ordering::Acquire),
                            download_speed: this.inner.info.download_speed.load(Ordering::Acquire),
                            upload_speed: this.inner.info.upload_speed.load(Ordering::Acquire),
                        };
                        let subs = this.inner.subscriptions.lock();
                        subs.iter().for_each(|call| {
                            call(msg);
                        });
                    }
                    None => {
                        // The connection was closed, let's restart the connector
                        // TODO: add a backoff counter
                        this.inner.is_connected.store(false, Ordering::Release);
                        let another = this.clone();
                        std::thread::spawn(move || {
                            nyanpasu_utils::runtime::block_on(async move {
                                log_err!(another.start().await);
                            })
                        });
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    pub fn update(&self, msg: ClashConnectionsMessage) {
        let elder_download_total = self
            .inner
            .info
            .download_total
            .swap(msg.download_total, Ordering::Release);
        let elder_upload_total = self
            .inner
            .info
            .upload_total
            .swap(msg.upload_total, Ordering::Release);
        self.inner.info.download_speed.store(
            msg.download_total
                .checked_sub(elder_download_total)
                .unwrap_or_default(),
            Ordering::Release,
        );
        self.inner.info.upload_speed.store(
            msg.upload_total
                .checked_sub(elder_upload_total)
                .unwrap_or_default(),
            Ordering::Release,
        );
    }

    pub fn stop(&self) {
        self.inner.stop_signal.store(true, Ordering::Acquire);
    }
}

impl Drop for ClashConnectionsConnector {
    fn drop(&mut self) {
        self.stop();
    }
}
