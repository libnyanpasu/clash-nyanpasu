use anyhow::{Context, Result, anyhow};
use axum::{
    Router,
    body::Body,
    extract::Query,
    http::{HeaderValue, Response, StatusCode},
    routing::get,
};
use base64::{Engine, prelude::BASE64_STANDARD};
use bytes::Bytes;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing_attributes::instrument;
use url::Url;

use std::{borrow::Cow, path::Path};

pub(crate) use crate::utils::candy::get_reqwest_client;

pub static SERVER_PORT: Lazy<u16> = Lazy::new(|| port_scanner::request_open_port().unwrap());

#[derive(Debug, Deserialize)]
struct CacheIcon {
    /// should be encoded as base64
    url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CacheFile<'n> {
    mime: Cow<'n, str>,
    bytes: Bytes,
}

impl TryFrom<CacheFile<'static>> for (HeaderValue, Bytes) {
    type Error = anyhow::Error;

    fn try_from(value: CacheFile<'static>) -> Result<Self, Self::Error> {
        Ok((
            value
                .mime
                .parse::<HeaderValue>()
                .context("failed to parse mime")?,
            value.bytes,
        ))
    }
}

// TODO: use Reader instead of Vec
async fn read_cache_file(path: &Path) -> Result<(HeaderValue, Bytes)> {
    let cache_file = tokio::fs::read(path).await?;
    let (cache_file, _): (CacheFile<'static>, _) =
        bincode::serde::decode_from_slice(&cache_file, bincode::config::standard())?;
    cache_file.try_into()
}

// TODO: use Writer instead of Vec
async fn write_cache_file(path: &Path, cache_file: &CacheFile<'_>) -> Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    let cache_file = bincode::serde::encode_to_vec(cache_file, bincode::config::standard())?;
    file.write_all(&cache_file).await?;
    Ok(())
}

async fn cache_icon_inner(url: &str) -> Result<(HeaderValue, Bytes)> {
    let url = BASE64_STANDARD.decode(url)?;
    let url = String::from_utf8_lossy(&url);
    let url = Url::parse(&url)?;
    // get filename
    let hash = Sha256::digest(url.as_str().as_bytes());
    let cache_dir = crate::utils::dirs::cache_dir()?.join("icons");
    if !cache_dir.exists() {
        std::fs::create_dir_all(&cache_dir)?;
    }
    let cache_file = cache_dir.join(format!("{:x}.bin", hash));
    if cache_file.exists() {
        let span = tracing::span!(tracing::Level::DEBUG, "read_cache_file", path = ?cache_file);
        let _enter = span.enter();
        match read_cache_file(&cache_file).await {
            Ok((mime, bytes)) => return Ok((mime, bytes)),
            Err(e) => {
                tracing::error!("failed to read cache file: {}", e);
                if let Err(e) = tokio::fs::remove_file(&cache_file).await {
                    tracing::error!("failed to remove cache file: {}", e);
                }
            }
        }
    }
    let client = get_reqwest_client()?;
    let response = client.get(url).send().await?.error_for_status()?;
    let mime = response
        .headers()
        .get("content-type")
        .ok_or(anyhow!("no content-type"))?
        .to_str()?
        .to_string();

    let bytes = response.bytes().await?;
    let data = CacheFile {
        mime: Cow::Owned(mime),
        bytes,
    };
    if let Err(e) = write_cache_file(&cache_file, &data).await {
        tracing::error!("failed to write cache file: {}", e);
    }
    Ok(data
        .try_into()
        .expect("It's impossible to fail, if failed, it must a bug, or memory corruption"))
}

#[tracing_attributes::instrument]
async fn cache_icon(query: Query<CacheIcon>) -> Response<Body> {
    match cache_icon_inner(&query.url).await {
        Ok((mime, bytes)) => {
            let mut response = Response::new(Body::from(bytes));
            response.headers_mut().insert("content-type", mime);
            response
        }
        Err(e) => {
            tracing::error!("{}", e);
            let mut response = Response::new(Body::from(e.to_string()));
            *response.status_mut() = StatusCode::BAD_REQUEST;
            response
        }
    }
}

#[derive(Deserialize)]
struct TrayIconReq {
    mode: crate::core::tray::icon::TrayIcon,
}

async fn tray_icon(query: Query<TrayIconReq>) -> Response<Body> {
    let mode = query.mode;
    let icon = crate::core::tray::icon::get_raw_icon(mode);
    let mut response = Response::new(Body::from(icon));
    response
        .headers_mut()
        .insert("content-type", "image/png".parse().unwrap());
    response
}

#[instrument]
pub async fn run(port: u16) -> std::io::Result<()> {
    let app = Router::new()
        .route("/cache/icon", get(cache_icon))
        .route("/tray/icon", get(tray_icon));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    tracing::debug!(
        "internal http server listening on {}",
        listener.local_addr()?
    );
    axum::serve(listener, app).await
}
