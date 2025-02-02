use std::{
    future::Future,
    ops::Deref,
    sync::{atomic::Ordering, Arc},
};

use anyhow::Context;
use atomic_enum::atomic_enum;
use backon::Retryable;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::{sync::mpsc::Receiver, task::JoinHandle};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, handshake::client::Request, protocol::Message},
};

use crate::log_err;

#[tracing::instrument]
async fn connect_clash_server<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
    endpoint: Request,
) -> anyhow::Result<Receiver<T>> {
    let (stream, _) = connect_async(endpoint).await?;
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

#[derive(Debug, Clone, Default, Copy, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionsInfo {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashConnectionsConnectorEvent {
    StateChanged(ClashConnectionsConnectorState),
    Update(ClashConnectionsInfo),
}

#[derive(PartialEq, Eq, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[atomic_enum]
pub enum ClashConnectionsConnectorState {
    Disconnected,
    Connecting,
    Connected,
}

pub struct ClashConnectionsConnectorInner {
    state: AtomicClashConnectionsConnectorState,
    connection_handler: Mutex<Option<JoinHandle<()>>>,
    broadcast_tx: tokio::sync::broadcast::Sender<ClashConnectionsConnectorEvent>,
    info: Mutex<ClashConnectionsInfo>,
}

// TODO:
#[derive(Clone)]
pub struct ClashConnectionsConnector {
    inner: Arc<ClashConnectionsConnectorInner>,
}

impl Deref for ClashConnectionsConnector {
    type Target = ClashConnectionsConnectorInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl ClashConnectionsConnector {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ClashConnectionsConnectorInner::new()),
        }
    }

    pub fn endpoint() -> anyhow::Result<Request> {
        let (server, secret) = {
            let info = crate::Config::clash().data().get_client_info();
            (info.server, info.secret)
        };
        let url = format!("ws://{}/connections", server);
        let mut request = url
            .into_client_request()
            .context("failed to create client request")?;
        if let Some(secret) = secret {
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {}", secret)
                    .parse()
                    .context("failed to create header value")?,
            );
        }
        Ok(request)
    }

    #[allow(clippy::manual_async_fn)]
    // FIXME: move to async fn while rust new solver got merged
    // ref: https://github.com/rust-lang/rust/issues/123072
    fn start_internal(&self) -> impl Future<Output = anyhow::Result<()>> + Send + use<'_> {
        async {
            self.dispatch_state_changed(ClashConnectionsConnectorState::Connecting);
            let endpoint = Self::endpoint().context("failed to create endpoint")?;
            log::debug!("connecting to clash connections ws server: {:?}", endpoint);
            let mut rx = connect_clash_server::<ClashConnectionsMessage>(endpoint).await?;
            self.dispatch_state_changed(ClashConnectionsConnectorState::Connected);
            let this = self.clone();
            let mut connection_handler = self.connection_handler.lock();
            let handle = tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Some(msg) => {
                            this.update(msg);
                        }
                        None => {
                            tracing::info!("clash ws server closed connection, trying to restart");
                            // The connection was closed, let's restart the connector
                            this.dispatch_state_changed(
                                ClashConnectionsConnectorState::Disconnected,
                            );
                            tokio::spawn(async move {
                                let restart = async || this.restart().await;
                                log_err!(restart
                                    .retry(backon::ExponentialBuilder::default())
                                    .sleep(tokio::time::sleep)
                                    .await
                                    .context("failed to restart clash connections"));
                            });
                            break;
                        }
                    }
                }
            });
            *connection_handler = Some(handle);
            Ok(())
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        self.start_internal().await.inspect_err(|_| {
            self.dispatch_state_changed(ClashConnectionsConnectorState::Disconnected);
        })
    }

    pub async fn restart(&self) -> anyhow::Result<()> {
        self.stop().await;
        self.start().await
    }
}

impl ClashConnectionsConnectorInner {
    pub fn new() -> Self {
        Self {
            state: AtomicClashConnectionsConnectorState::new(
                ClashConnectionsConnectorState::Disconnected,
            ),
            connection_handler: Mutex::new(None),
            broadcast_tx: tokio::sync::broadcast::channel(5).0,
            info: Mutex::new(ClashConnectionsInfo::default()),
        }
    }

    pub fn state(&self) -> ClashConnectionsConnectorState {
        self.state.load(Ordering::Acquire)
    }

    fn dispatch_state_changed(&self, state: ClashConnectionsConnectorState) {
        self.state.store(state, Ordering::Release);
        // SAFETY: the failures only there no active receivers,
        // so that the message will be dropped directly
        let _ = self
            .broadcast_tx
            .send(ClashConnectionsConnectorEvent::StateChanged(state));
    }

    /// Subscribe to the ClashConnectionsConnectorEvent
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ClashConnectionsConnectorEvent> {
        self.broadcast_tx.subscribe()
    }

    fn update(&self, msg: ClashConnectionsMessage) {
        let mut info = self.info.lock();
        let previous_download_total =
            std::mem::replace(&mut info.download_total, msg.download_total);
        let previous_upload_total = std::mem::replace(&mut info.upload_total, msg.upload_total);
        info.download_speed = msg
            .download_total
            .checked_sub(previous_download_total)
            .unwrap_or_default();
        info.upload_speed = msg
            .upload_total
            .checked_sub(previous_upload_total)
            .unwrap_or_default();

        // SAFETY: the failures only there no active receivers,
        // so that the message will be dropped directly
        let _ = self
            .broadcast_tx
            .send(ClashConnectionsConnectorEvent::Update(*info));
    }

    pub async fn stop(&self) {
        log::info!("stopping clash connections ws server");
        let handle = self.connection_handler.lock().take();
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
        }
        self.dispatch_state_changed(ClashConnectionsConnectorState::Disconnected);
    }
}

impl Drop for ClashConnectionsConnectorInner {
    fn drop(&mut self) {
        let cleanup = async move {
            self.stop().await;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| {
                tauri::async_runtime::block_on(cleanup);
            }),
            Err(_) => {
                tauri::async_runtime::block_on(cleanup);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_clash_server() {
        "ws://127.0.0.1:12649:10808/connections"
            .into_client_request()
            .unwrap();
    }
}
