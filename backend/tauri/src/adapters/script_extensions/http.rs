//! Desktop HTTP module fetching and the existing on-disk cache policy.
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_fs::create_dir_all;
use boa_utils::module::http::{CachedItem, HttpModuleError, HttpModuleSource};
use tokio::{runtime::Handle, sync::oneshot::channel as oneshot_channel};
use url::Url;

pub(crate) struct CachedHttpModuleSource {
    client: reqwest::Client,
    cache_dir: PathBuf,
    max_age: Duration,
    clock: Arc<dyn Fn() -> SystemTime + Send + Sync>,
    runtime: Handle,
}

impl CachedHttpModuleSource {
    pub(crate) fn new(
        client: reqwest::Client,
        cache_dir: PathBuf,
        max_age: Duration,
        clock: Arc<dyn Fn() -> SystemTime + Send + Sync>,
        runtime: Handle,
    ) -> Self {
        Self {
            client,
            cache_dir,
            max_age,
            clock,
            runtime,
        }
    }

    fn mapping_cache_dir(&self, url: &url::Url) -> PathBuf {
        let mut buf = self.cache_dir.clone();
        let host = match url.host() {
            Some(host) => host.to_string().replace('.', "--"),
            None => "unknown".to_string(),
        };
        let port = match url.port() {
            Some(port) => format!("__{port}"),
            None => "".to_string(),
        };
        buf.push(format!("{}_{}{}", url.scheme(), host, port));
        buf.push(url.path().replace('/', "_").replace(".", "--"));
        buf
    }
}

#[async_trait::async_trait]
impl HttpModuleSource for CachedHttpModuleSource {
    async fn load(&self, url: &Url) -> Result<CachedItem, HttpModuleError> {
        let cache_path = self.mapping_cache_dir(url);
        let parent_dir = cache_path
            .parent()
            .ok_or_else(|| {
                log::error!("failed to get parent directory for `{url}`");
                HttpModuleError::LoadModule {
                    source: std::io::Error::other(format!(
                        "failed to get cache parent directory for `{url}`; path: `{}`",
                        cache_path.display()
                    ))
                    .into(),
                }
            })?
            .to_path_buf();

        let max_age = self.max_age;

        log::debug!("checking cache for `{url}`...");

        let now = (self.clock)();
        let should_use_cached_content = match async_fs::metadata(&cache_path).await {
            Ok(metadata)
                if metadata
                    .modified()
                    .is_ok_and(|modified| modified > now - max_age) =>
            {
                true
            }
            Err(err) => {
                // create dir if not exists
                if err.kind() == std::io::ErrorKind::NotFound
                    && let Err(e) = async_fs::metadata(&parent_dir).await
                    && e.kind() == std::io::ErrorKind::NotFound
                    && let Err(err) = create_dir_all(parent_dir).await
                {
                    log::error!(
                        "failed to create cache directory for `{url}`; path: `{}`. error: `{}`",
                        cache_path.display(),
                        err
                    );
                }
                false
            }
            _ => false,
        };

        let item: anyhow::Result<CachedItem> = if should_use_cached_content {
            async {
                log::debug!("fetching `{url}` from cache...");
                let item = async_fs::read(&cache_path).await?;
                let item = postcard::from_bytes(&item)?;
                log::debug!("finished fetching `{url}` from cache");
                Ok(item)
            }
            .await
        } else {
            log::debug!("fetching `{url}`...");
            let (tx, rx) = oneshot_channel();
            let fetcher_url = url.clone();
            let client = self.client.clone();
            self.runtime.spawn(async move {
                let result = async {
                    let response = client.get(fetcher_url.as_str()).send().await?;

                    let mime = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .map(|v| v.to_string())
                        .unwrap_or(mime::TEXT_PLAIN.to_string());
                    let body = response.text().await?;

                    log::debug!("finished fetching `{fetcher_url}`");
                    Ok(CachedItem {
                        mime,
                        content: body,
                    })
                }
                .await;
                let _ = tx.send(result);
            });
            rx.await.expect("should never drop oneshot tx")
        };

        if let Ok(item) = &item {
            match postcard::to_stdvec(&item) {
                Ok(item) => {
                    if let Err(err) = async_fs::write(&cache_path, &item).await {
                        log::error!(
                            "failed to write cache for `{url}`; path: `{}`. error: `{}`",
                            cache_path.display(),
                            err
                        );
                    }
                }
                Err(err) => {
                    log::error!("failed to serialize content: {err}");
                }
            }
        }

        item.map_err(|source| HttpModuleError::LoadModule {
            source: source.into(),
        })
    }
}

#[cfg(test)]
mod tests;
