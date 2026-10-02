//! Downloads and installs an application update on the backend, so the app
//! shuts down through its own exit path before the new version takes over.
use std::{fmt::Debug, future::Future};

use tauri::{Manager, ResourceId, Webview, ipc::Channel};
use tauri_plugin_updater::Update;

/// One route to the package `check_update` found.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UpdateDownload {
    pub source: nyanpasu_config::application::UpdateSource,
    pub rid: ResourceId,
}

/// Download progress, in the shape of the updater plugin's own event.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(tag = "event", content = "data")]
pub enum UpdateDownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        chunk_length: usize,
    },
    Finished,
}

/// Admits one update installation at a time: a second one would download
/// again and race the first to the installer and the shutdown.
#[derive(Default)]
pub struct UpdateInstallation(tokio::sync::Mutex<()>);

impl UpdateInstallation {
    fn admit(&self) -> anyhow::Result<tokio::sync::MutexGuard<'_, ()>> {
        self.0
            .try_lock()
            .map_err(|_| anyhow::anyhow!("an update is already being installed"))
    }
}

/// Tries each source in order and returns the first package that downloads
/// and verifies, with its bytes. Every source carries the same release, signed
/// with the same key: only the route differs, so a failure moves on.
async fn download_first<S, T, F, Fut, E>(
    candidates: Vec<(S, T)>,
    mut download: F,
) -> anyhow::Result<(T, Vec<u8>)>
where
    S: Debug,
    T: Clone,
    F: FnMut(T) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, E>>,
    E: std::fmt::Display,
{
    let mut errors = Vec::new();
    for (source, candidate) in candidates {
        match download(candidate.clone()).await {
            Ok(bytes) => return Ok((candidate, bytes)),
            Err(error) => errors.push(format!("{source:?}: {error}")),
        }
    }
    anyhow::bail!("All update package sources failed:\n{}", errors.join("\n"))
}

/// Downloads the update and installs it. It returns only on failure: on
/// Windows the installer ends the process from inside `install`, after the
/// `on_before_exit` hook `check_update` set has shut the app down; elsewhere the
/// new version is in place and the app restarts into it through the exit
/// boundary, which shuts it down first.
pub async fn install(
    webview: &Webview,
    downloads: Vec<UpdateDownload>,
    on_event: Channel<UpdateDownloadEvent>,
) -> anyhow::Result<()> {
    let installation = webview.state::<UpdateInstallation>();
    let _admitted = installation.admit()?;
    let candidates = {
        let resources = webview.resources_table();
        downloads
            .into_iter()
            .map(|download| Ok((download.source, resources.get::<Update>(download.rid)?)))
            .collect::<tauri::Result<Vec<_>>>()?
    };
    let (update, bytes) = download_first(candidates, |update| {
        let on_event = on_event.clone();
        async move {
            let mut started = false;
            update
                .download(
                    |chunk_length, content_length| {
                        if !std::mem::replace(&mut started, true) {
                            let _ = on_event.send(UpdateDownloadEvent::Started { content_length });
                        }
                        let _ = on_event.send(UpdateDownloadEvent::Progress { chunk_length });
                    },
                    || {
                        let _ = on_event.send(UpdateDownloadEvent::Finished);
                    },
                )
                .await
        }
    })
    .await?;

    // The installer and the hook block, so they run off the async workers.
    let installed = match tokio::task::spawn_blocking(move || update.install(bytes)).await {
        Ok(installed) => installed,
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    };
    let app_handle = webview.app_handle();
    if let Err(error) = installed {
        // The hook may have shut the app down before the installer failed to
        // start, e.g. when its elevation was declined: the app restarts into
        // the version it is, rather than run on without its owners.
        if super::exit::has_shut_down(app_handle) {
            super::help::restart_application(app_handle);
        }
        return Err(error.into());
    }
    super::help::restart_application(app_handle);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    type Attempts = Arc<Mutex<Vec<&'static str>>>;

    async fn fetch(
        attempts: &Attempts,
        candidate: (&'static str, Result<(), &'static str>),
    ) -> Result<Vec<u8>, &'static str> {
        attempts.lock().unwrap().push(candidate.0);
        candidate.1.map(|()| candidate.0.as_bytes().to_vec())
    }

    async fn run(
        candidates: Vec<(&'static str, Result<(), &'static str>)>,
    ) -> (anyhow::Result<&'static str>, Vec<&'static str>) {
        let attempts = Attempts::default();
        let candidates = candidates
            .into_iter()
            .map(|candidate| (candidate.0, candidate))
            .collect();
        let result = download_first(candidates, |candidate| {
            let attempts = attempts.clone();
            async move { fetch(&attempts, candidate).await }
        })
        .await
        .map(|(candidate, bytes)| {
            assert_eq!(bytes, candidate.0.as_bytes(), "the bytes of the winner");
            candidate.0
        });
        let attempts = attempts.lock().unwrap().clone();
        (result, attempts)
    }

    #[test]
    fn one_installation_is_admitted_at_a_time() {
        let installation = UpdateInstallation::default();

        let first = installation.admit().unwrap();
        assert_eq!(
            installation.admit().unwrap_err().to_string(),
            "an update is already being installed"
        );
        drop(first);
        assert!(installation.admit().is_ok(), "admitted again once it ended");
    }

    #[tokio::test]
    async fn the_first_source_that_downloads_wins_in_priority_order() {
        let (result, attempts) = run(vec![
            ("nyanpasu", Err("download failure")),
            ("github", Ok(())),
        ])
        .await;
        assert_eq!(result.unwrap(), "github");
        assert_eq!(attempts, ["nyanpasu", "github"]);
    }

    #[tokio::test]
    async fn a_failed_signature_falls_back_like_a_failed_download() {
        let (result, attempts) = run(vec![
            ("nyanpasu", Err("signature failure")),
            ("github", Ok(())),
        ])
        .await;
        assert_eq!(result.unwrap(), "github");
        assert_eq!(attempts, ["nyanpasu", "github"]);
    }

    #[tokio::test]
    async fn no_source_is_tried_after_the_first_success() {
        let (result, attempts) = run(vec![("nyanpasu", Ok(())), ("github", Ok(()))]).await;
        assert_eq!(result.unwrap(), "nyanpasu");
        assert_eq!(attempts, ["nyanpasu"]);
    }

    #[tokio::test]
    async fn every_source_error_is_reported_when_all_fail() {
        let (result, attempts) = run(vec![
            ("nyanpasu", Err("download failure")),
            ("github", Err("signature failure")),
        ])
        .await;
        assert_eq!(
            result.unwrap_err().to_string(),
            "All update package sources failed:\n\"nyanpasu\": download failure\n\"github\": signature failure"
        );
        assert_eq!(attempts, ["nyanpasu", "github"]);
    }
}
