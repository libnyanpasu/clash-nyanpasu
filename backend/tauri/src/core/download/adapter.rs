//! A [`BoltLoadAdapter`] implemented on top of the project's existing reqwest 0.12
//! client.
//!
//! We deliberately do **not** use bolt-load's bundled reqwest adapter: it targets
//! reqwest 0.13 while this workspace pins 0.12. Implementing the adapter ourselves
//! keeps a single reqwest / TLS dependency tree and lets us reuse the already
//! configured proxy, user-agent and rustls client.

use std::sync::Arc;

use async_trait::async_trait;
use bolt_load::adapter::{
    AdapterError, AnyBytesStream, BoltLoadAdapter, BoltLoadAdapterMeta, RetryableError,
    UnretryableError,
};
use futures::{StreamExt, TryStreamExt};
use reqwest::{Client, StatusCode, header};
use url::Url;

pub struct NyanpasuReqwestAdapter {
    client: Client,
    url: Url,
}

struct HeadInfo {
    content_size: u64,
    range_supported: bool,
    filename: Option<String>,
}

impl NyanpasuReqwestAdapter {
    pub fn new(client: Client, url: Url) -> Self {
        Self { client, url }
    }

    /// Probe the resource with a HEAD request: content length, range support and a
    /// suggested filename.
    async fn head(&self) -> Result<HeadInfo, AdapterError> {
        let resp = self
            .client
            .head(self.url.clone())
            .send()
            .await
            .map_err(map_reqwest_err)?
            .error_for_status()
            .map_err(map_reqwest_err)?;
        let headers = resp.headers();
        let content_size = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let range_supported = headers
            .get(header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none"));
        let filename = parse_filename(headers).or_else(|| filename_from_url(&self.url));
        Ok(HeadInfo {
            content_size,
            range_supported,
            filename,
        })
    }
}

#[async_trait]
impl BoltLoadAdapter for NyanpasuReqwestAdapter {
    async fn is_range_stream_available(&self) -> bool {
        match self.head().await {
            Ok(info) => info.range_supported && info.content_size > 0,
            Err(_) => false,
        }
    }

    async fn retrieve_meta(&self) -> Result<BoltLoadAdapterMeta, AdapterError> {
        let info = self.head().await?;
        Ok(BoltLoadAdapterMeta {
            content_size: info.content_size,
            filename: info.filename,
        })
    }

    async fn full_stream(&self) -> Result<AnyBytesStream, AdapterError> {
        let resp = self
            .client
            .get(self.url.clone())
            .send()
            .await
            .map_err(map_reqwest_err)?
            .error_for_status()
            .map_err(map_reqwest_err)?;
        Ok(resp.bytes_stream().map_err(map_reqwest_err).boxed())
    }

    async fn range_stream(&self, start: u64, end: u64) -> Result<AnyBytesStream, AdapterError> {
        // bolt-load uses half-open `[start, end)`; HTTP Range is inclusive.
        let range = format!("bytes={}-{}", start, end.saturating_sub(1));
        let resp = self
            .client
            .get(self.url.clone())
            .header(header::RANGE, range)
            .send()
            .await
            .map_err(map_reqwest_err)?
            .error_for_status()
            .map_err(map_reqwest_err)?;
        Ok(resp.bytes_stream().map_err(map_reqwest_err).boxed())
    }
}

/// Classify a reqwest error into a retryable / unretryable [`AdapterError`].
fn map_reqwest_err(e: reqwest::Error) -> AdapterError {
    match e.status() {
        Some(StatusCode::NOT_FOUND) => UnretryableError::NotFound.into(),
        Some(status @ (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)) => {
            UnretryableError::Unauthorized {
                message: status.to_string(),
            }
            .into()
        }
        Some(status) if status.is_server_error() => retryable_io(e),
        Some(status) => UnretryableError::ServiceUnavailable {
            status: status.to_string(),
            description: status.canonical_reason().unwrap_or_default().to_string(),
        }
        .into(),
        None => retryable_io(e),
    }
}

fn retryable_io(e: reqwest::Error) -> AdapterError {
    RetryableError::Io {
        source: Arc::new(std::io::Error::other(e)),
    }
    .into()
}

fn parse_filename(headers: &header::HeaderMap) -> Option<String> {
    let value = headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()?;
    value.split(';').find_map(|part| {
        part.trim()
            .strip_prefix("filename=")
            .map(|f| f.trim_matches(|c| c == '"' || c == '\'').to_string())
            .filter(|f| !f.is_empty())
    })
}

fn filename_from_url(url: &Url) -> Option<String> {
    url.path_segments()?
        .next_back()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, routing::get};
    use tokio::net::TcpListener;

    const TEST_CONTENT: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!@"; // 64 bytes

    async fn spawn(router: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{addr}")
    }

    fn test_client() -> Client {
        Client::builder().no_proxy().build().unwrap()
    }

    fn respond(
        status: axum::http::StatusCode,
        body: Vec<u8>,
        cd: bool,
    ) -> axum::http::Response<Body> {
        let len = body.len();
        let mut r = axum::http::Response::new(Body::from(body));
        let h = r.headers_mut();
        h.insert(
            axum::http::header::ACCEPT_RANGES,
            axum::http::HeaderValue::from_static("bytes"),
        );
        h.insert(
            axum::http::header::CONTENT_LENGTH,
            axum::http::HeaderValue::from_str(&len.to_string()).unwrap(),
        );
        if cd {
            h.insert(
                axum::http::header::CONTENT_DISPOSITION,
                axum::http::HeaderValue::from_static(r#"attachment; filename="testfile.bin""#),
            );
        }
        *r.status_mut() = status;
        r
    }

    /// Router: HEAD/GET with full range support and Content-Disposition.
    fn range_router() -> Router {
        async fn h(req: axum::http::Request<Body>) -> axum::http::Response<Body> {
            let content = TEST_CONTENT.as_bytes();
            let total = content.len();
            let range = req
                .headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("bytes=").map(|v| v.to_owned()));
            let (status, body) = match range {
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
                        content[start..end_excl].to_vec(),
                    )
                }
                None => (axum::http::StatusCode::OK, content.to_vec()),
            };
            respond(
                status,
                body,
                status != axum::http::StatusCode::PARTIAL_CONTENT,
            )
        }
        Router::new().route("/content", get(h))
    }

    /// Router without Accept-Ranges header.
    fn no_range_router() -> Router {
        async fn h() -> axum::http::Response<Body> {
            let mut r = axum::http::Response::new(Body::from(TEST_CONTENT.as_bytes().to_vec()));
            r.headers_mut().insert(
                axum::http::header::CONTENT_LENGTH,
                axum::http::HeaderValue::from_static("64"),
            );
            r
        }
        Router::new().route("/content", get(h))
    }

    #[tokio::test]
    async fn range_detection() {
        let a = spawn(range_router()).await;
        let ad = NyanpasuReqwestAdapter::new(
            test_client(),
            Url::parse(&format!("{a}/content")).unwrap(),
        );
        assert!(ad.is_range_stream_available().await);

        let a = spawn(no_range_router()).await;
        let ad = NyanpasuReqwestAdapter::new(
            test_client(),
            Url::parse(&format!("{a}/content")).unwrap(),
        );
        assert!(!ad.is_range_stream_available().await);
    }

    #[tokio::test]
    async fn metadata_has_size_and_filename() {
        let a = spawn(range_router()).await;
        let ad = NyanpasuReqwestAdapter::new(
            test_client(),
            Url::parse(&format!("{a}/content")).unwrap(),
        );
        let m = ad.retrieve_meta().await.unwrap();
        assert_eq!(m.content_size, 64);
        assert_eq!(m.filename.as_deref(), Some("testfile.bin"));
    }

    #[tokio::test]
    async fn full_stream_completes() {
        let a = spawn(range_router()).await;
        let ad = NyanpasuReqwestAdapter::new(
            test_client(),
            Url::parse(&format!("{a}/content")).unwrap(),
        );
        let bytes: Vec<u8> = ad
            .full_stream()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(bytes, TEST_CONTENT.as_bytes());
    }

    #[tokio::test]
    async fn range_stream_correct() {
        let a = spawn(range_router()).await;
        let ad = NyanpasuReqwestAdapter::new(
            test_client(),
            Url::parse(&format!("{a}/content")).unwrap(),
        );
        let bytes: Vec<u8> = ad
            .range_stream(8, 24)
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(bytes, &TEST_CONTENT.as_bytes()[8..24]);
    }

    #[tokio::test]
    async fn error_classification() {
        let a = spawn(range_router()).await;
        // 404 → unretryable
        let ad =
            NyanpasuReqwestAdapter::new(test_client(), Url::parse(&format!("{a}/nope")).unwrap());
        assert!(matches!(
            ad.retrieve_meta().await.unwrap_err(),
            AdapterError::Unretryable { .. }
        ));

        // 500 → retryable
        async fn fail() -> axum::http::StatusCode {
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        }
        let a = spawn(Router::new().route("/fail", get(fail))).await;
        let ad =
            NyanpasuReqwestAdapter::new(test_client(), Url::parse(&format!("{a}/fail")).unwrap());
        assert!(matches!(
            ad.retrieve_meta().await.unwrap_err(),
            AdapterError::Retryable { .. }
        ));

        // connection refused → retryable
        let ad = NyanpasuReqwestAdapter::new(
            test_client(),
            Url::parse("http://127.0.0.1:1/nope").unwrap(),
        );
        assert!(matches!(
            ad.retrieve_meta().await.unwrap_err(),
            AdapterError::Retryable { .. }
        ));
    }
}
