//! Bounded PAC downloads and OS auto-proxy configuration.

use std::time::Duration;

use camino::Utf8PathBuf;
use nyanpasu_contracts::system_proxy::{
    BuildHttpClientSnafu, ClearPacSnafu, DownloadCancelledSnafu, InstallPacSnafu,
    MissingEntryPointSnafu, PacError, PacPort, ReadScriptBodySnafu, RequestScriptSnafu,
    ScriptNotUtf8Snafu, ScriptStatusSnafu, ScriptTooLargeSnafu,
};
use snafu::{ResultExt as _, ensure};
use sysproxy::Autoproxy;
use tokio_util::sync::CancellationToken;

const PAC_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// A PAC script is a small JavaScript file. The cap is what stops a hostile or
/// misconfigured url from streaming an unbounded body into this process.
const PAC_MAX_BODY: usize = 8 * 1024 * 1024;

/// Downloads the PAC script, caches it and points the OS auto-config at the
/// same URL. There is no local PAC server: the OS fetches the URL itself, and
/// the cache only exists so the script can be inspected after the fact.
pub struct HttpPacBackend {
    client: reqwest::Client,
    cache_path: Utf8PathBuf,
}

impl HttpPacBackend {
    pub fn new(cache_path: Utf8PathBuf) -> Result<Self, PacError> {
        let client = reqwest::Client::builder()
            .timeout(PAC_DOWNLOAD_TIMEOUT)
            .build()
            .boxed()
            .context(BuildHttpClientSnafu)?;
        Ok(Self { client, cache_path })
    }

    /// One bounded attempt, raced against the token: it runs on the actor's
    /// mailbox, and a shutdown that had to wait it out would exit with the
    /// proxy still on. Retrying a failed download is the effects actor's job.
    async fn download(
        &self,
        url: &url::Url,
        cancel: &CancellationToken,
    ) -> Result<String, PacError> {
        cancel
            .run_until_cancelled(self.fetch(url))
            .await
            .unwrap_or_else(|| DownloadCancelledSnafu.fail())
    }

    async fn fetch(&self, url: &url::Url) -> Result<String, PacError> {
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .await
            .boxed()
            .context(RequestScriptSnafu { url: url.as_str() })?;
        let status = response.status();
        ensure!(
            status.is_success(),
            ScriptStatusSnafu {
                url: url.as_str(),
                status: status.as_u16()
            }
        );
        // Checked first where the server declares it, and again while reading,
        // because the declaration is only a claim.
        ensure!(
            response
                .content_length()
                .is_none_or(|length| length <= PAC_MAX_BODY as u64),
            ScriptTooLargeSnafu {
                url: url.as_str(),
                limit: PAC_MAX_BODY
            }
        );

        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .boxed()
            .context(ReadScriptBodySnafu { url: url.as_str() })?
        {
            ensure!(
                body.len() + chunk.len() <= PAC_MAX_BODY,
                ScriptTooLargeSnafu {
                    url: url.as_str(),
                    limit: PAC_MAX_BODY
                }
            );
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).context(ScriptNotUtf8Snafu { url: url.as_str() })
    }

    async fn cache(&self, script: &str) {
        if let Some(parent) = self.cache_path.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            tracing::warn!(%error, "failed to create the PAC cache directory");
            return;
        }
        // Best effort: the OS fetches the url itself, so a missing cache file
        // changes nothing about whether the proxy works.
        if let Err(error) = tokio::fs::write(&self.cache_path, script).await {
            tracing::warn!(%error, "failed to cache the PAC script");
        }
    }
}

#[async_trait::async_trait]
impl PacPort for HttpPacBackend {
    fn is_supported(&self) -> bool {
        Autoproxy::is_support()
    }

    async fn apply(&self, url: &url::Url, cancel: CancellationToken) -> Result<(), PacError> {
        let script = self.download(url, &cancel).await?;
        // Validated before it is installed: an OS pointed at a script without
        // an entry point resolves every request to no proxy at all.
        ensure!(
            script.contains("FindProxyForURL"),
            MissingEntryPointSnafu { url: url.as_str() }
        );
        self.cache(&script).await;
        // The restore is already on its way to putting the original settings
        // back, so installing this url now would outlive the app.
        ensure!(!cancel.is_cancelled(), DownloadCancelledSnafu);

        let url = url.to_string();
        crate::blocking::join(
            tokio::task::spawn_blocking(move || {
                Autoproxy {
                    enable: true,
                    url: url.clone(),
                }
                .set_auto_proxy()
                .boxed()
                .context(InstallPacSnafu { url })
            })
            .await,
        )
    }

    fn disable(&self) -> Result<(), PacError> {
        if !Autoproxy::is_support() {
            return Ok(());
        }
        Autoproxy {
            enable: false,
            url: String::new(),
        }
        .set_auto_proxy()
        .boxed()
        .context(ClearPacSnafu)
    }
}
