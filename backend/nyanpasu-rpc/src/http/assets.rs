//! Static frontend assets for a WebUI host: files by path with the SPA
//! fallback, behind whichever listener serves the router. The shell supplies
//! the bytes (a Tauri bundle, a directory), so no host is assumed here.

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};

/// Where a shell's frontend files come from.
pub trait FrontendAssets: Send + Sync + 'static {
    /// The MIME type and bytes of the asset at `path` (no leading slash), or
    /// `None` when there is none. It may block: the router calls it off the
    /// async workers.
    fn get(&self, path: &str) -> Option<(String, Vec<u8>)>;
}

/// A router that answers every path from `assets`: the asset itself, or the
/// SPA's `index.html` for a navigation to a route the app handles, or 404.
pub fn assets_router(assets: Arc<dyn FrontendAssets>) -> Router {
    Router::new().fallback(serve).with_state(assets)
}

async fn serve(State(assets): State<Arc<dyn FrontendAssets>>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let decoded = percent_encoding::percent_decode_str(request.uri().path()).decode_utf8_lossy();
    if decoded.split('/').any(|part| part == "..") || decoded.contains('\\') {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let path = decoded.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path }.to_owned();
    let wants_page = request
        .headers()
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/html"));
    let head = request.method() == Method::HEAD;
    let asset = tokio::task::spawn_blocking(move || {
        // Assets and API typos must return 404 rather than the SPA HTML.
        assets.get(&path).or_else(|| {
            (!path.starts_with("bridge/")
                && !path.rsplit('/').next().unwrap_or("").contains('.')
                && wants_page)
                .then(|| assets.get("index.html"))
                .flatten()
        })
    })
    .await
    // The blocking task only moves the lookup off the async workers; a panic in
    // the shell's asset source is a bug and keeps propagating.
    .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()));
    match asset {
        Some((mime, bytes)) => {
            let length = bytes.len();
            let body = if head {
                Body::empty()
            } else {
                Body::from(bytes)
            };
            Response::builder()
                .header("content-type", mime)
                .header("content-length", length)
                .header("cache-control", "no-cache")
                .body(body)
                .expect("a MIME type from an asset source is a valid header value")
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::Request as HttpRequest};
    use tower::ServiceExt as _;

    use super::*;

    struct Assets;

    impl FrontendAssets for Assets {
        fn get(&self, path: &str) -> Option<(String, Vec<u8>)> {
            match path {
                "index.html" => Some(("text/html".into(), b"<html>app</html>".to_vec())),
                "assets/app.js" => Some(("text/javascript".into(), b"console.log('app')".to_vec())),
                _ => None,
            }
        }
    }

    #[tokio::test]
    async fn serves_assets_the_spa_and_real_404s() {
        let router = assets_router(Arc::new(Assets));
        for (path, accept, expected, mime) in [
            ("/", "*/*", StatusCode::OK, Some("text/html")),
            (
                "/assets/app.js",
                "*/*",
                StatusCode::OK,
                Some("text/javascript"),
            ),
            (
                "/main/settings/debug",
                "text/html",
                StatusCode::OK,
                Some("text/html"),
            ),
            (
                "/assets/missing.js",
                "text/html",
                StatusCode::NOT_FOUND,
                None,
            ),
            ("/bridge/missing", "text/html", StatusCode::NOT_FOUND, None),
            ("/%2e%2e/secret", "text/html", StatusCode::BAD_REQUEST, None),
        ] {
            let response = router
                .clone()
                .oneshot(
                    HttpRequest::get(path)
                        .header("accept", accept)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}");
            if let Some(mime) = mime {
                assert_eq!(response.headers()["content-type"], mime);
            }
        }
        let head = router
            .oneshot(
                HttpRequest::head("/assets/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(to_bytes(head.into_body(), 1024).await.unwrap().is_empty());
    }
}
