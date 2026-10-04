//! Tauri updater, proxy, event, and installer boundaries for application updates.

use std::{future::Future, sync::Arc, time::Duration};

use anyhow::{Context as _, Result, anyhow};
use async_trait::async_trait;
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event as _;
use tokio_util::sync::CancellationToken;

use super::{
    AppUpdateBackend, AppUpdateDownloadProgress, AppUpdateEventSink, AppUpdateRelease,
    AppUpdateSettings, AppUpdateSnapshot, AppUpdateStateChanged, PreparedAppUpdate,
    VerifiedAppUpdate,
};
use crate::service::profile_file::SelfProxyPortSource;
use nyanpasu_config::application::UpdateSource;

struct DownloadCandidates(Vec<(UpdateSource, Update)>);

pub(crate) struct TauriAppUpdateBackend {
    app: tauri::AppHandle,
    proxy_port: Arc<dyn SelfProxyPortSource>,
}

impl TauriAppUpdateBackend {
    pub fn new(app: tauri::AppHandle, proxy_port: Arc<dyn SelfProxyPortSource>) -> Self {
        Self { app, proxy_port }
    }
}

#[async_trait]
impl AppUpdateBackend for TauriAppUpdateBackend {
    async fn check(
        &self,
        settings: &AppUpdateSettings,
        cancellation: CancellationToken,
    ) -> Result<Option<PreparedAppUpdate>> {
        let check = async {
            let channel = settings.channel;
            let local = semver::Version::parse(crate::consts::BUILD_INFO.pkg_version)?;
            let build_time = time::OffsetDateTime::parse(
                crate::consts::BUILD_INFO.build_date,
                &time::format_description::well_known::Rfc3339,
            )?;
            let mut builder = self
                .app
                .updater_builder()
                .endpoints(
                    crate::bundle::update_endpoints(channel)
                        .into_iter()
                        .map(|endpoint| endpoint.parse())
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                )?
                .timeout(Duration::from_secs(30))
                .configure_client(|client| {
                    client
                        .connect_timeout(Duration::from_secs(15))
                        .read_timeout(Duration::from_secs(60))
                })
                .version_comparator(move |_, remote| {
                    crate::bundle::is_newer_release(channel, &local, &remote, build_time)
                })
                .on_before_exit({
                    let app = self.app.clone();
                    move || {
                        crate::utils::exit::shutdown_before_exit_blocking(&app);
                        app.cleanup_before_exit();
                    }
                });
            if let Some(port) = self.proxy_port.mixed_port() {
                builder = builder.proxy(crate::utils::config::get_self_proxy(port).parse()?);
            }
            if let Ok(Some(proxy)) = crate::utils::config::get_system_proxy() {
                builder = builder.proxy(proxy.parse().context("invalid system proxy")?);
            }
            let Some(mut update) = builder.build()?.check().await? else {
                return Ok(None);
            };
            // The check has a total deadline; package downloads only have connection
            // and stalled-read deadlines, so a large, steadily downloading package
            // is not interrupted by the check's short timeout.
            update.timeout = None;
            let identity = format!(
                "{:?}|{}|{}|{}|{:?}",
                channel, update.target, update.version, update.signature, update.date,
            );
            let candidates = crate::bundle::update_download_urls(
                &update.download_url,
                &settings.sources,
                &update.raw_json,
                &update.target,
            )?
            .into_iter()
            .map(|(source, url)| {
                let mut candidate = update.clone();
                candidate.download_url = url;
                (source, candidate)
            })
            .collect::<Vec<_>>();
            Ok(Some(PreparedAppUpdate {
                identity,
                release: AppUpdateRelease {
                    version: update.version,
                    date: update.date.map(|date| {
                        date.format(&time::format_description::well_known::Rfc3339)
                            .expect("release dates have an RFC3339 representation")
                    }),
                    body: update.body,
                },
                context: Arc::new(DownloadCandidates(candidates)),
            }))
        };
        tokio::select! {
            biased;
            () = cancellation.cancelled() => anyhow::bail!("application update check cancelled"),
            result = check => result,
        }
    }

    async fn download(
        &self,
        update: PreparedAppUpdate,
        cancellation: CancellationToken,
        progress: Arc<dyn Fn(AppUpdateDownloadProgress) + Send + Sync>,
    ) -> Result<VerifiedAppUpdate> {
        let candidates = update
            .context
            .downcast_ref::<DownloadCandidates>()
            .context("application update has no platform download context")?;
        let (source, bytes) = download_first(&candidates.0, &cancellation, |source, candidate| {
            let progress = progress.clone();
            async move {
                progress(AppUpdateDownloadProgress::Source {
                    source,
                    content_length: None,
                });
                let mut downloaded = 0;
                let mut started = false;
                candidate
                    .download(
                        |chunk, total| {
                            if !std::mem::replace(&mut started, true) {
                                progress(AppUpdateDownloadProgress::Source {
                                    source,
                                    content_length: total,
                                });
                            }
                            downloaded += chunk as u64;
                            progress(AppUpdateDownloadProgress::Chunk { downloaded, total });
                        },
                        || progress(AppUpdateDownloadProgress::Verifying),
                    )
                    .await
            }
        })
        .await?;
        Ok(VerifiedAppUpdate {
            bytes: Arc::new(bytes),
            source,
        })
    }

    async fn install(&self, update: PreparedAppUpdate, package: VerifiedAppUpdate) -> Result<()> {
        let candidates = update
            .context
            .downcast_ref::<DownloadCandidates>()
            .context("application update has no platform installer context")?;
        let installer = candidates
            .0
            .iter()
            .find(|(source, _)| *source == package.source)
            .map(|(_, update)| update.clone())
            .context("verified package source has no installer context")?;
        let installed =
            match tokio::task::spawn_blocking(move || installer.install(package.bytes.as_slice()))
                .await
            {
                Ok(result) => result,
                Err(error) => match error.try_into_panic() {
                    Ok(panic) => std::panic::resume_unwind(panic),
                    Err(error) => return Err(error.into()),
                },
            };
        if let Err(error) = installed {
            // An installer may fail after its exit hook shut down the owners.
            // Relaunch the installed version instead of leaving a stopped app.
            if crate::utils::exit::has_shut_down(&self.app) {
                crate::utils::help::restart_application(&self.app);
            }
            return Err(error.into());
        }
        crate::utils::help::restart_application(&self.app);
        Ok(())
    }
}

async fn download_first<T: Clone, F, Fut, E>(
    candidates: &[(UpdateSource, T)],
    cancellation: &CancellationToken,
    mut download: F,
) -> Result<(UpdateSource, Vec<u8>)>
where
    F: FnMut(UpdateSource, T) -> Fut,
    Fut: Future<Output = std::result::Result<Vec<u8>, E>>,
    E: std::fmt::Display,
{
    let mut errors = Vec::new();
    for (source, candidate) in candidates {
        // Only explicit owner cancellation drops the HTTP future. A caller
        // leaving the page or dropping an RPC waiter does not reach this token.
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => anyhow::bail!("application update download cancelled"),
            result = download(*source, candidate.clone()) => result,
        };
        match result {
            Ok(bytes) => return Ok((*source, bytes)),
            Err(error) => errors.push(format!("{source:?}: {error}")),
        }
    }
    Err(anyhow!(
        "All update package sources failed:\n{}",
        errors.join("\n")
    ))
}

pub(crate) struct TauriAppUpdateEventSink(tauri::AppHandle);

impl TauriAppUpdateEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self(app)
    }
}

impl AppUpdateEventSink for TauriAppUpdateEventSink {
    fn publish(&self, snapshot: AppUpdateSnapshot) {
        crate::log_err!(AppUpdateStateChanged(snapshot).emit(&self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn signature_or_download_failure_tries_the_next_source_in_order() {
        let candidates = [
            (UpdateSource::Nyanpasu, Err("signature failure")),
            (UpdateSource::Sourceforge, Err("404 not found")),
            (UpdateSource::Ghfast, Ok(vec![1, 2, 3])),
            (UpdateSource::Github, Ok(vec![4])),
        ];
        let mut attempts = Vec::new();
        let (source, bytes) = download_first(
            &candidates,
            &CancellationToken::new(),
            |source, response| {
                attempts.push(source);
                std::future::ready(response)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            attempts,
            [
                UpdateSource::Nyanpasu,
                UpdateSource::Sourceforge,
                UpdateSource::Ghfast
            ]
        );
        assert_eq!(source, UpdateSource::Ghfast);
        assert_eq!(bytes, [1, 2, 3]);
    }

    #[tokio::test]
    async fn all_source_failures_keep_their_causes() {
        let candidates: [(UpdateSource, Result<Vec<u8>, &str>); 2] = [
            (UpdateSource::Nyanpasu, Err("download failure")),
            (UpdateSource::Github, Err("signature failure")),
        ];
        let error = download_first(&candidates, &CancellationToken::new(), |_, result| {
            std::future::ready(result)
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Nyanpasu: download failure"));
        assert!(error.to_string().contains("Github: signature failure"));
    }

    #[tokio::test]
    async fn cancellation_ends_a_stalled_attempt_without_trying_another_source() {
        let candidates = [(UpdateSource::Nyanpasu, ()), (UpdateSource::Github, ())];
        let cancellation = CancellationToken::new();
        let signal = cancellation.clone();
        let mut attempts = Vec::new();
        let error = download_first(&candidates, &cancellation, |source, ()| {
            attempts.push(source);
            signal.cancel();
            std::future::pending::<Result<Vec<u8>, &str>>()
        })
        .await
        .unwrap_err();
        assert_eq!(attempts, [UpdateSource::Nyanpasu]);
        assert!(error.to_string().contains("cancelled"));
    }
}
