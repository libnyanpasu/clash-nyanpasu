use anyhow::{Context, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, path::Path, time::Duration};
use tokio::io::AsyncWriteExt;
use url::Url;

const CACHE_TIMEOUT: Duration = Duration::from_secs(60 * 60 * 24 * 7);

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CacheFile<'n> {
    mime: Cow<'n, str>,
    bytes: Bytes,
}

async fn read_cache_file(path: &Path) -> Result<CacheFile<'static>> {
    let bytes = tokio::fs::read(path).await?;
    Ok(postcard::from_bytes(&bytes)?)
}

async fn write_cache_file(path: &Path, cache_file: &CacheFile<'_>) -> Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    file.write_all(&postcard::to_allocvec(cache_file)?).await?;
    Ok(())
}

async fn remove_cache_file(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await {
        tracing::error!("failed to remove icon cache file: {error}");
    }
}

pub async fn get_cached_icon(
    url: &str,
    self_proxy_port: u16,
    paths: &nyanpasu_paths::PathResolver,
) -> Result<(String, Bytes)> {
    let url = Url::parse(&url)?;
    let hash = Sha256::digest(url.as_str().as_bytes());
    let cache_dir = paths.cache_dir()?.join("icons");
    tokio::fs::create_dir_all(&cache_dir).await?;
    let outdated_time = std::time::SystemTime::now()
        .checked_sub(CACHE_TIMEOUT)
        .expect("icon cache timeout is valid");
    let cache_file = cache_dir.join(format!("{}.bin", hex::encode(hash)));

    if let Ok(meta) = tokio::fs::metadata(&cache_file).await {
        if meta
            .modified()
            .is_ok_and(|modified| modified < outdated_time)
        {
            remove_cache_file(&cache_file).await;
        } else {
            match read_cache_file(&cache_file).await {
                Ok(data) => return Ok((data.mime.into_owned(), data.bytes)),
                Err(error) => {
                    tracing::error!("failed to read icon cache file: {error}");
                    remove_cache_file(&cache_file).await;
                }
            }
        }
    }

    let client = crate::utils::candy::get_reqwest_client(self_proxy_port)?;
    let response = client.get(url).send().await?.error_for_status()?;
    let mime = response
        .headers()
        .get("content-type")
        .context("icon response has no content-type")?
        .to_str()?
        .to_owned();
    let data = CacheFile {
        mime: Cow::Owned(mime),
        bytes: response.bytes().await?,
    };
    if let Err(error) = write_cache_file(&cache_file, &data).await {
        tracing::error!("failed to write icon cache file: {error}");
    }

    Ok((data.mime.into_owned(), data.bytes))
}
