//! Unified file-download engine backed by [`bolt_load`].
//!
//! This module is the single entry point for downloading files in the backend
//! (clash core artifacts today, app-update artifacts later). It wraps a
//! [`bolt_load`] task driven by [`NyanpasuReqwestAdapter`] and exposes a small,
//! pollable status surface that stays compatible with the existing updater UI
//! (`state` / `downloaded` / `total` / `speed`).

mod adapter;

use std::{path::PathBuf, sync::Arc};

use adapter::NyanpasuReqwestAdapter;
use bolt_load::{
    adapter::AnyAdapter,
    runtime::ThreadedRuntimeImpl,
    task::{Task, instance::TaskEvent},
};
use parking_lot::RwLock;
use serde::Serialize;
use smol_cancellation_token::CancellationToken;
use tokio::sync::Mutex;
use url::Url;

/// The high-level state of a download session.
#[derive(Debug, Clone, Serialize, Default, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DownloaderState {
    #[default]
    Idle,
    Downloading,
    Failed(String),
    Finished,
}

/// A snapshot of a download session's progress, polled by IPC.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct DownloadStatus {
    pub state: DownloaderState,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
}

#[derive(Debug, Default, Clone, Copy)]
struct ProgressSnapshot {
    downloaded: u64,
    total: u64,
    speed: f64,
}

struct SessionInner {
    state: RwLock<DownloaderState>,
    progress: RwLock<ProgressSnapshot>,
    cancel: CancellationToken,
}

impl SessionInner {
    fn on_event(&self, event: TaskEvent) {
        match event {
            TaskEvent::Initializing => {
                *self.state.write() = DownloaderState::Downloading;
            }
            TaskEvent::Downloading(progress) => {
                {
                    let mut prog = self.progress.write();
                    prog.downloaded = progress.downloaded();
                    prog.total = progress.total().unwrap_or(prog.total);
                    prog.speed = progress.speed();
                }
                *self.state.write() = DownloaderState::Downloading;
            }
            TaskEvent::Finished(progress) => {
                {
                    let mut prog = self.progress.write();
                    prog.downloaded = progress.downloaded;
                    if let Some(total) = progress.total {
                        prog.total = total;
                    }
                    prog.speed = 0.0;
                }
                *self.state.write() = DownloaderState::Finished;
            }
            TaskEvent::Failed(err) => {
                *self.state.write() = DownloaderState::Failed(err.to_string());
            }
        }
    }
}

pub struct DownloadSession {
    inner: Arc<SessionInner>,
    task: Mutex<Task>,
}

impl DownloadSession {
    pub async fn new(
        client: reqwest::Client,
        url: Url,
        save_path: PathBuf,
    ) -> anyhow::Result<Self> {
        let inner = Arc::new(SessionInner {
            state: RwLock::new(DownloaderState::Idle),
            progress: RwLock::new(ProgressSnapshot::default()),
            cancel: CancellationToken::new(),
        });
        let adapter: AnyAdapter = Box::new(NyanpasuReqwestAdapter::new(client, url));
        let cb_inner = inner.clone();
        let task = Task::builder()
            .adapter(adapter)
            .save_path(save_path)
            .threaded_runtime(ThreadedRuntimeImpl::new_tokio_rt())
            .cancel_token(inner.cancel.clone())
            .on_task_state_changed(move |event| cb_inner.on_event(event))
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("failed to build download task: {e}"))?;
        Ok(Self {
            inner,
            task: Mutex::new(task),
        })
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let mut task = self.task.lock().await;
        task.run()
            .await
            .map_err(|e| anyhow::anyhow!("failed to start download: {e}"))?;
        match task.wait().await {
            Ok(()) => Ok(()),
            Err(e) => {
                *self.inner.state.write() = DownloaderState::Failed(e.to_string());
                Err(anyhow::anyhow!("download failed: {e}"))
            }
        }
    }

    pub fn status(&self) -> DownloadStatus {
        let prog = *self.inner.progress.read();
        DownloadStatus {
            state: self.inner.state.read().clone(),
            downloaded: prog.downloaded,
            total: prog.total,
            speed: prog.speed,
        }
    }

    #[allow(dead_code)]
    pub fn cancel(&self) {
        self.inner.cancel.cancel();
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, extract::State, routing::get};
    use sha2::{Digest, Sha256 as Sha2};
    use std::{io::Read, sync::Arc as StdArc, time::Duration};
    use tempfile::TempDir;
    use tokio::net::TcpListener;

    fn sha256(data: &[u8]) -> String {
        hex::encode(Sha2::digest(data))
    }

    fn test_content(size: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(size);
        let mut i = 0u64;
        while out.len() < size {
            let line = format!("{:016x} the quick brown fox jumps over the lazy dog\n", i);
            let b = line.as_bytes();
            out.extend_from_slice(&b[..(size - out.len()).min(b.len())]);
            i += 1;
        }
        out
    }

    async fn spawn(router: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{addr}")
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    struct SvcState {
        content: Vec<u8>,
        fail_get: bool,
    }

    fn content_router(content: Vec<u8>, fail_get: bool) -> Router {
        let s = StdArc::new(SvcState { content, fail_get });
        async fn h(
            State(s): State<StdArc<SvcState>>,
            req: axum::http::Request<Body>,
        ) -> axum::http::Response<Body> {
            if req.method() == axum::http::Method::HEAD {
                let mut r = axum::http::Response::new(Body::empty());
                let hd = r.headers_mut();
                hd.insert(
                    axum::http::header::ACCEPT_RANGES,
                    axum::http::HeaderValue::from_static("bytes"),
                );
                hd.insert(
                    axum::http::header::CONTENT_LENGTH,
                    axum::http::HeaderValue::from_str(&s.content.len().to_string()).unwrap(),
                );
                return r;
            }
            if s.fail_get {
                return axum::http::Response::builder()
                    .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap();
            }
            let total = s.content.len();
            let (status, body) = match req
                .headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("bytes=").map(|v| v.to_owned()))
            {
                Some(r) => {
                    let mut parts = r.split('-');
                    let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let end_incl: usize = parts
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(total - 1);
                    let end_excl = (end_incl + 1).min(total);
                    (
                        axum::http::StatusCode::PARTIAL_CONTENT,
                        s.content[start..end_excl].to_vec(),
                    )
                }
                None => (axum::http::StatusCode::OK, s.content.clone()),
            };
            let len = body.len();
            let mut r = axum::http::Response::new(Body::from(body));
            let hd = r.headers_mut();
            hd.insert(
                axum::http::header::ACCEPT_RANGES,
                axum::http::HeaderValue::from_static("bytes"),
            );
            hd.insert(
                axum::http::header::CONTENT_LENGTH,
                axum::http::HeaderValue::from_str(&len.to_string()).unwrap(),
            );
            *r.status_mut() = status;
            r
        }
        Router::new().route("/data", get(h)).with_state(s)
    }

    #[tokio::test]
    async fn download_and_verify_hash() {
        let content = test_content(128 * 1024);
        let hash = sha256(&content);
        let u = format!(
            "{}/data",
            spawn(content_router(content.clone(), false)).await
        );

        let tmp = TempDir::new().unwrap();
        let sp = tmp.path().join("payload.bin");
        let session = DownloadSession::new(test_client(), Url::parse(&u).unwrap(), sp.clone())
            .await
            .unwrap();

        assert!(matches!(session.status().state, DownloaderState::Idle));
        session.start().await.unwrap();

        let s = session.status();
        assert!(matches!(s.state, DownloaderState::Finished));
        assert_eq!(s.downloaded, 128 * 1024);

        let mut buf = Vec::new();
        std::fs::File::open(&sp)
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        assert_eq!(sha256(&buf), hash);
    }

    #[tokio::test]
    async fn cancel_mid_download() {
        let content = test_content(8 * 1024 * 1024);
        let u = format!(
            "{}/data",
            spawn(content_router(content.clone(), false)).await
        );

        let tmp = TempDir::new().unwrap();
        let sp = tmp.path().join("large.bin");
        let session = StdArc::new(
            DownloadSession::new(test_client(), Url::parse(&u).unwrap(), sp)
                .await
                .unwrap(),
        );

        let s2 = session.clone();
        let h = tokio::spawn(async move { s2.start().await });
        tokio::time::sleep(Duration::from_millis(50)).await;

        session.cancel();
        let r = h.await.unwrap();
        assert!(r.is_err());

        let s = session.status();
        assert!(matches!(
            &s.state,
            DownloaderState::Failed(reason) if reason.contains("cancelled") || reason.contains("stopped")
        ));
    }

    #[tokio::test]
    async fn download_error_produces_failed() {
        let content = test_content(32 * 1024);
        let u = format!("{}/data", spawn(content_router(content, true)).await);

        let tmp = TempDir::new().unwrap();
        let sp = tmp.path().join("payload.bin");
        let session = DownloadSession::new(test_client(), Url::parse(&u).unwrap(), sp)
            .await
            .unwrap();

        assert!(session.start().await.is_err());
        assert!(matches!(session.status().state, DownloaderState::Failed(_)));
    }
}
