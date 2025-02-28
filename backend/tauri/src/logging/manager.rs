use std::{collections::HashMap, ops::Deref, sync::Arc};

use crate::logging::indexer::Indexer;
use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use notify::EventKind;
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
};
use tokio::sync::{RwLock, mpsc::Receiver};

#[derive(Clone)]
pub struct IndexerManager {
    inner: Arc<IndexerManagerInner>,
}

impl Deref for IndexerManager {
    type Target = IndexerManagerInner;

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
        let mut inner = IndexerManagerInner::new();
        inner
            .scan(&logging_dir)
            .await
            .context("failed to scan logging directory")?;
        let mut rx = inner
            .recommended_watcher(&logging_dir)
            .context("failed to create recommended watcher")?;
        let manager = Self {
            inner: Arc::new(inner),
        };
        let this = manager.clone();
        tokio::spawn(async move {
            while let Some(events) = rx.recv().await {
                for event in events {
                    if let Err(err) = this.handle_event(event).await {
                        tracing::error!("failed to handle event: {:?}", err);
                    }
                }
            }
        });
        Ok(manager)
    }

    #[tracing::instrument(skip(self))]
    async fn handle_event(&self, event: DebouncedEvent) -> anyhow::Result<()> {
        tracing::debug!("received event: {:?}", event);
        let path = event.paths.first().context("no path in event")?;
        let path = Utf8Path::from_path(path).context("failed to convert path to Utf8Path")?;

        let create_indexer = async |path: &Utf8Path| {
            let mut map = self.inner.map.write().await;
            let indexer = Indexer::try_new(path.to_path_buf()).await?;
            map.insert(path.to_path_buf(), indexer);
            Ok::<_, anyhow::Error>(())
        };

        match event.kind {
            EventKind::Create(_) => {
                if is_log_file(path).await? {
                    tracing::debug!("create indexer for {}", path);
                    create_indexer(path).await?;
                }
            }
            EventKind::Remove(_) => {
                let mut map = self.inner.map.write().await;
                map.remove(path);
            }
            EventKind::Modify(_) => {
                if is_log_file(path).await? {
                    let mut map = self.inner.map.write().await;
                    match map.get_mut(path) {
                        Some(indexer) => {
                            indexer.on_file_change().await?;
                        }
                        None => {
                            create_indexer(path).await?;
                        }
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
}

// TODO: only keep latest log file when we detect a serious memory report on it
pub struct IndexerManagerInner {
    map: RwLock<HashMap<Utf8PathBuf, Indexer>>,
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
}

impl IndexerManagerInner {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            debouncer: None,
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn scan(&self, logging_dir: &Utf8Path) -> anyhow::Result<()> {
        let mut map = self.map.write().await;
        let mut entries = tokio::fs::read_dir(logging_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = Utf8PathBuf::from_path_buf(entry.path())
                .map_err(|e| anyhow::anyhow!("failed to convert path: {:?}", e))?;
            if is_log_file(&path).await? {
                tracing::debug!("create indexer for {}", path);
                let indexer = Indexer::try_new(path.clone()).await?;
                map.insert(path, indexer);
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

    pub async fn get_indexer(
        &self,
        path: &Utf8Path,
    ) -> Option<tokio::sync::RwLockReadGuard<'_, Indexer>> {
        let map = self.map.read().await;
        tokio::sync::RwLockReadGuard::try_map(map, |map| map.get(path)).ok()
    }
}
