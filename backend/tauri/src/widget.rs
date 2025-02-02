use crate::config::{nyanpasu::NetworkStatisticWidgetConfig, Config};

use super::core::clash::ws::ClashConnectionsConnectorEvent;

use anyhow::Context;
use nyanpasu_egui::{
    ipc::{create_ipc_server, IpcSender, Message, StatisticMessage},
    widget::StatisticWidgetVariant,
};
use std::sync::{atomic::AtomicBool, Arc};
use tauri::{utils::platform::current_exe, Manager, Runtime};
use tokio::{
    process::Child,
    sync::{
        broadcast::{error::RecvError as BroadcastRecvError, Receiver as BroadcastReceiver},
        Mutex,
    },
};

#[derive(Clone)]
pub struct WidgetManager {
    instance: Arc<Mutex<Option<WidgetManagerInstance>>>,
    listener_initd: Arc<AtomicBool>,
}

struct WidgetManagerInstance {
    tx: IpcSender<Message>,
    process: Child,
}

impl WidgetManager {
    pub fn new() -> Self {
        Self {
            instance: Arc::new(Mutex::new(None)),
            listener_initd: Arc::new(AtomicBool::new(false)),
        }
    }

    fn register_listener(&self, mut receiver: BroadcastReceiver<ClashConnectionsConnectorEvent>) {
        if self
            .listener_initd
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        let signal = self.listener_initd.clone();
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        if let Err(e) = this.handle_event(event).await {
                            log::error!("Failed to handle event: {}", e);
                        }
                    }
                    Err(e) => {
                        log::error!("Error receiving event: {}", e);
                        if BroadcastRecvError::Closed == e {
                            signal.store(false, std::sync::atomic::Ordering::Release);
                            break;
                        }
                    }
                }
            }
        });
        self.listener_initd
            .store(true, std::sync::atomic::Ordering::Release);
    }

    async fn handle_event(&self, event: ClashConnectionsConnectorEvent) -> anyhow::Result<()> {
        let mut instance = self.instance.clone().lock_owned().await;
        if let ClashConnectionsConnectorEvent::Update(info) = event {
            if instance
                .as_mut()
                .is_some_and(|instance| instance.is_alive())
            {
                tokio::task::spawn_blocking(move || {
                    let instance = instance.as_ref().unwrap();
                    // we only care about the update event now
                    instance
                        .tx
                        .send(Message::UpdateStatistic(StatisticMessage {
                            download_total: info.download_total,
                            upload_total: info.upload_total,
                            download_speed: info.download_speed,
                            upload_speed: info.upload_speed,
                        }))
                        .context("Failed to send event to widget")?;
                    Ok::<(), anyhow::Error>(())
                })
                .await
                .context("Failed to send event to widget")??;
            }
        }
        Ok(())
    }

    pub async fn start(&self, widget: StatisticWidgetVariant) -> anyhow::Result<()> {
        let mut instance = self.instance.lock().await;
        if instance.is_some() {
            log::info!("Widget already running, stopping it first...");
            self.stop().await.context("Failed to stop widget")?;
        }
        let current_exe = current_exe().context("Failed to get current executable")?;
        // This operation is blocking, but it internal just a system call, so I think it's okay
        let (mut ipc_server, server_name) = create_ipc_server()?;
        // spawn a process to run the widget
        let child = tokio::process::Command::new(current_exe)
            .arg("statistic-widget")
            .arg(serde_json::to_string(&widget).context("Failed to serialize widget")?)
            .env("NYANPASU_EGUI_IPC_SERVER", server_name)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn widget process")?;
        let tx = tokio::task::spawn_blocking(move || {
            ipc_server
                .connect()
                .context("Failed to connect to widget")?;
            ipc_server.into_tx().context("Failed to get ipc sender")
        })
        .await
        .context("Failed to read widget output")??;
        instance.replace(WidgetManagerInstance { tx, process: child });
        Ok(())
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        let Some(mut instance) = self.instance.lock().await.take() else {
            return Ok(());
        };
        if !instance.is_alive() {
            return Ok(());
        }
        // first try to stop the process gracefully
        let mut instance = tokio::task::spawn_blocking(move || {
            instance
                .tx
                .send(Message::Stop)
                .context("Failed to send stop message to widget")?;
            Ok::<WidgetManagerInstance, anyhow::Error>(instance)
        })
        .await
        .context("Failed to kill widget process")??;
        for _ in 0..5 {
            if instance.is_alive() {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            } else {
                return Ok(());
            }
        }
        // force kill the process
        instance
            .process
            .kill()
            .await
            .context("Failed to kill widget process")?;
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        let mut instance = self.instance.lock().await;
        instance
            .as_mut()
            .is_some_and(|instance| instance.is_alive())
    }
}

impl WidgetManagerInstance {
    pub fn is_alive(&mut self) -> bool {
        self.process.try_wait().is_ok_and(|status| status.is_none())
    }
}

impl Drop for WidgetManager {
    fn drop(&mut self) {
        let cleanup = async {
            let _ = self.stop().await;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(_) => {
                tokio::task::block_in_place(move || {
                    tauri::async_runtime::block_on(cleanup);
                });
            }
            Err(_) => {
                tauri::async_runtime::block_on(cleanup);
            }
        }
    }
}

pub async fn setup<R: Runtime, M: Manager<R>>(
    manager: &M,
    ws_connections_receiver: BroadcastReceiver<ClashConnectionsConnectorEvent>,
) -> anyhow::Result<()> {
    let widget_manager = WidgetManager::new();
    // TODO: use the app_handle to read initial config.
    let option = Config::verge()
        .data()
        .network_statistic_widget
        .unwrap_or_default();
    widget_manager.register_listener(ws_connections_receiver);
    if let NetworkStatisticWidgetConfig::Enabled(widget) = option {
        widget_manager.start(widget).await?;
    }

    // TODO: subscribe to the config change event
    manager.manage(widget_manager);
    Ok(())
}
