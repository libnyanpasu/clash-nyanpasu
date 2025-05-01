use std::{collections::HashMap, ops::Deref, sync::Arc};

use crate::logging::indexer::Indexer;
use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use notify::EventKind;
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
};
use tokio::{
    sync::{
        RwLock,
        mpsc::{Receiver, UnboundedSender},
        oneshot,
    },
    task::{JoinHandle, LocalSet},
};
use tokio_util::sync::CancellationToken;

use super::{LogEntry, Query};

#[derive(Clone)]
pub struct IndexerManager {
    inner: Arc<IndexerRunnerGuard>,
}

impl Deref for IndexerManager {
    type Target = IndexerRunnerGuard;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

async fn is_log_file(path: &Utf8Path) -> anyhow::Result<bool> {
    let metadata = tokio::fs::metadata(path).await?;
    Ok(metadata.is_file()
        && path
            .file_name()
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".log")))
}

impl IndexerManager {
    pub async fn try_new(logging_dir: Utf8PathBuf) -> anyhow::Result<Self> {
        let inner = IndexerManagerRunner::new_and_spawn().await;
        let manager = Self {
            inner: Arc::new(inner),
        };
        manager.watch(&logging_dir).await?;
        Ok(manager)
    }
}

// TODO: only keep latest log file when we detect a serious memory report on it
pub struct IndexerManagerRunner {
    map: HashMap<Utf8PathBuf, Indexer>,
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
}

pub enum IndexerRunnerCmd {
    /// scan the logging directory for new log files
    Watch(Utf8PathBuf, oneshot::Sender<anyhow::Result<()>>),
    /// remove the indexer for the given path
    // Unwatch(Utf8PathBuf, oneshot::Sender<anyhow::Result<()>>),
    /// query the indexer for the given path
    AddLogFile(Utf8PathBuf, oneshot::Sender<anyhow::Result<()>>),
    RemoveLogFile(Utf8PathBuf, oneshot::Sender<anyhow::Result<()>>),
    LogFileChanged(Utf8PathBuf, oneshot::Sender<anyhow::Result<()>>),
    Query(Utf8PathBuf, Query, oneshot::Sender<Option<Vec<LogEntry>>>),
}

pub struct IndexerRunnerGuard {
    cancel_token: CancellationToken,
    handle: JoinHandle<()>,
    tx: tokio::sync::mpsc::UnboundedSender<IndexerRunnerCmd>,
}

impl IndexerManagerRunner {
    pub async fn new_and_spawn() -> IndexerRunnerGuard {
        let cancel_token = CancellationToken::new();
        let (handle, rx) = Self::spawn_task(cancel_token.clone());
        let tx = rx.await.unwrap();

        IndexerRunnerGuard {
            cancel_token,
            handle,
            tx,
        }
    }

    fn spawn_task(
        cancel_token: CancellationToken,
    ) -> (
        JoinHandle<()>,
        tokio::sync::oneshot::Receiver<tokio::sync::mpsc::UnboundedSender<IndexerRunnerCmd>>,
    ) {
        let (tx, rx) = oneshot::channel();
        let handle = tauri::async_runtime::spawn_blocking(move || {
            let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut runner = Self {
                map: HashMap::new(),
                debouncer: None,
            };
            let local = LocalSet::new();
            let cmd_tx_clone = cmd_tx.clone();
            local.spawn_local(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    runner.run_cmd(&cmd_tx_clone, cmd).await;
                }
            });
            tx.send(cmd_tx).unwrap();
            tauri::async_runtime::block_on(async {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        tracing::info!("cancel token triggered, shutting down");
                    }
                    _ = local => {}
                }
            });
        });

        // unwrap the join handle
        match handle {
            tauri::async_runtime::JoinHandle::Tokio(handle) => (handle, rx),
        }
    }

    async fn run_cmd(&mut self, cmd_tx: &UnboundedSender<IndexerRunnerCmd>, cmd: IndexerRunnerCmd) {
        match cmd {
            IndexerRunnerCmd::Watch(path, tx) => {
                if let Err(err) = self.scan(&path).await {
                    tx.send(Err(err)).unwrap();
                    return;
                }
                let watcher = self.recommended_watcher(&path).unwrap();
                let cmd_tx = cmd_tx.clone();
                nyanpasu_utils::runtime::spawn(Self::spawn_watcher(watcher, cmd_tx));
                tx.send(Ok(())).unwrap();
            }
            IndexerRunnerCmd::Query(path, query, tx) => {
                let indexer = self.map.get(&path).unwrap();
                let result = indexer.query(query);
                tx.send(result).unwrap();
            }
            IndexerRunnerCmd::AddLogFile(path, tx) => {
                let mut indexer = Indexer::new(path.clone());
                if let Err(err) = indexer.build_index().await {
                    tx.send(Err(err)).unwrap();
                    return;
                }
                self.map.insert(path, indexer);
                tx.send(Ok(())).unwrap();
            }
            IndexerRunnerCmd::RemoveLogFile(path, tx) => {
                self.map.remove(&path);
                tx.send(Ok(())).unwrap();
            }
            IndexerRunnerCmd::LogFileChanged(path, tx) => {
                let indexer = self.map.get_mut(&path).unwrap();
                if let Err(err) = indexer.on_file_change().await {
                    tx.send(Err(err)).unwrap();
                    return;
                }
                tx.send(Ok(())).unwrap();
            }
        }
    }

    async fn spawn_watcher(
        mut watcher: Receiver<Vec<DebouncedEvent>>,
        cmd_tx: UnboundedSender<IndexerRunnerCmd>,
    ) {
        while let Some(events) = watcher.recv().await {
            for event in events {
                let path = Utf8Path::from_path(event.paths.first().unwrap()).unwrap();
                match event.kind {
                    EventKind::Create(_) => {
                        if is_log_file(path).await.is_ok_and(|ok| ok) {
                            tracing::debug!("create indexer for {}", path);
                            let (tx, rx) = oneshot::channel();
                            cmd_tx
                                .send(IndexerRunnerCmd::AddLogFile(path.to_path_buf(), tx))
                                .unwrap();
                            match rx.await {
                                Ok(_) => {
                                    tracing::debug!("indexer for {} created", path);
                                }
                                Err(err) => {
                                    tracing::error!("failed to create indexer for {}", path);
                                }
                            }
                        }
                    }
                    EventKind::Remove(_) => {
                        let (tx, rx) = oneshot::channel();
                        cmd_tx
                            .send(IndexerRunnerCmd::RemoveLogFile(path.to_path_buf(), tx))
                            .unwrap();
                        match rx.await {
                            Ok(_) => {
                                tracing::debug!("indexer for {} removed", path);
                            }
                            Err(err) => {
                                tracing::error!("failed to remove indexer for {}", path);
                            }
                        }
                    }
                    EventKind::Modify(_) => {
                        if is_log_file(path).await.is_ok_and(|ok| ok) {
                            let (tx, rx) = oneshot::channel();
                            cmd_tx
                                .send(IndexerRunnerCmd::LogFileChanged(path.to_path_buf(), tx))
                                .unwrap();
                            match rx.await {
                                Ok(_) => {
                                    tracing::debug!("indexer for {} updated", path);
                                }
                                Err(err) => {
                                    tracing::error!("failed to update indexer for {}", path);
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn scan(&mut self, logging_dir: &Utf8Path) -> anyhow::Result<()> {
        let mut entries = tokio::fs::read_dir(logging_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = Utf8PathBuf::from_path_buf(entry.path())
                .map_err(|e| anyhow::anyhow!("failed to convert path: {:?}", e))?;
            if is_log_file(&path).await? {
                tracing::debug!("create indexer for {}", path);
                let mut indexer = Indexer::new(path.clone());
                indexer.build_index().await?;
                self.map.insert(path, indexer);
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn recommended_watcher(
        &mut self,
        logging_dir: &Utf8Path,
    ) -> anyhow::Result<Receiver<Vec<DebouncedEvent>>> {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let mut debouncer = new_debouncer(
            std::time::Duration::from_secs(2),
            None,
            move |events_result: DebounceEventResult| match events_result {
                Ok(events) => {
                    let tx = tx.clone();
                    nyanpasu_utils::runtime::spawn(async move {
                        if let Err(err) = tx.send(events).await {
                            tracing::error!("failed to send events to channel: {:?}", err);
                        }
                    });
                }
                Err(errors) => {
                    tracing::error!(
                        "failed to receive events from logging directory: {:?}",
                        errors
                    );
                }
            },
        )?;
        debouncer
            .watch(logging_dir, RecursiveMode::Recursive)
            .context("failed to watch logging directory")?;

        self.debouncer = Some(debouncer);

        Ok(rx)
    }
}

impl IndexerRunnerGuard {
    pub async fn watch(&self, logging_dir: &Utf8Path) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(IndexerRunnerCmd::Watch(logging_dir.to_path_buf(), tx))
            .context("failed to send watch command")?;
        rx.await.context("failed to receive watch command")??;
        Ok(())
    }
}
