//! Icon entry security checks through the listener's authentication middleware.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    middleware,
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;

use super::{LocalSession, local_session};
use crate::{
    icons::{
        self,
        tests::{failing_fixture, fixture},
    },
    test_support::{Fixture, with_http_auth},
};

fn listener(fixture: &Fixture) -> Router {
    fixture
        .rpc
        .clone()
        .router()
        .layer(middleware::from_fn_with_state(
            LocalSession {
                authority: "127.0.0.1:1234".into(),
                access_token: "test-access".into(),
                session_id: "sess_test_icon".into(),
                shutdown: CancellationToken::new(),
            },
            local_session,
        ))
}

async fn get(
    app: &Router,
    path: &str,
    cookie: &str,
    origin: Option<&str>,
    host: &str,
) -> axum::response::Response {
    let mut request = Request::get(path)
        .header("host", host)
        .header("cookie", cookie);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

const COOKIES: &str =
    "nyanpasu_http_access=test-access; nyanpasu_http_session=00000000-0000-4000-8000-000000000001";
const CACHED: &str = "/bridge/icons/cached?url=https%3A%2F%2Ficons.example%2Ficon.svg";

#[tokio::test]
async fn icon_get_without_origin_accepts_valid_session_cookies() {
    let fixture = fixture("image/svg+xml", 1).await;
    let response = get(&listener(&fixture), CACHED, COOKIES, None, "127.0.0.1:1234").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/svg+xml");
    assert_eq!(response.headers()["cache-control"], "private, max-age=3600");
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 3);
        assert!(
            records
                .iter()
                .all(|record| record.header.method == "icon_fetch")
        );
        assert!(!format!("{records:?}").contains("icons.example"));
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn unauthenticated_icon_get_does_not_fetch_remote_content() {
    let fixture = fixture("image/png", 0).await;
    let app = listener(&fixture);
    for (cookie, origin, host, status) in [
        ("", None, "127.0.0.1:1234", StatusCode::UNAUTHORIZED),
        (
            "nyanpasu_http_session=00000000-0000-4000-8000-000000000001",
            None,
            "127.0.0.1:1234",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "nyanpasu_http_access=test-access",
            None,
            "127.0.0.1:1234",
            StatusCode::BAD_REQUEST,
        ),
        (
            COOKIES,
            Some("https://foreign.example"),
            "127.0.0.1:1234",
            StatusCode::FORBIDDEN,
        ),
        (COOKIES, None, "foreign.example", StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            get(&app, CACHED, cookie, origin, host).await.status(),
            status
        );
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn icon_responses_include_sandbox_and_nosniff() {
    for mime in ["image/svg+xml", "text/html"] {
        let fixture = fixture(mime, 1).await;
        let app = listener(&fixture);
        for path in [
            CACHED,
            CACHED,
            "/bridge/icons/tray?mode=normal",
            "/bridge/icons/unknown",
        ] {
            let response = get(&app, path, COOKIES, None, "127.0.0.1:1234").await;
            for (name, value) in icons::SECURITY_HEADERS {
                assert_eq!(response.headers()[name], value);
            }
            if mime == "text/html" && path == CACHED {
                assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
                let body = axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap();
                assert!(!String::from_utf8_lossy(&body).contains("untrusted icon bytes"));
            }
        }
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn icon_cache_policy_does_not_relax_other_routes() {
    let fixture = fixture("image/png", 1).await;
    let mut files = crate::ports::MockHostIconFiles::new();
    files.expect_tray_icon().returning(|_, _| vec![1, 2, 3]);
    let mut fixture = fixture;
    std::sync::Arc::get_mut(&mut fixture.rpc.dependencies)
        .unwrap()
        .icon_files = Some(std::sync::Arc::new(files));
    let app = listener(&fixture);
    for (path, cache) in [
        (CACHED, "private, max-age=3600"),
        (
            "/bridge/icons/tray?mode=normal&version=1",
            "private, no-cache",
        ),
        ("/bridge/icons/tray?mode=invalid", "no-store"),
        ("/bridge/missing", "no-store"),
        ("/", "no-store"),
    ] {
        assert_eq!(
            get(&app, path, COOKIES, None, "127.0.0.1:1234")
                .await
                .headers()["cache-control"],
            cache,
            "{path}"
        );
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn http_icon_route_enforces_visibility_privacy_across_read_and_sensitive_scopes() {
    use nyanpasu_rpc_protocol::{AuthContext, Principal, Scope};

    let fixture = failing_fixture(2).await;
    let router = fixture.rpc.clone().router();
    let path = "/bridge/icons/cached?url=https%3A%2F%2Fuser%3Apass%40secret-icon-canary.invalid%2Ficon.png";

    for (scopes, is_full) in [
        (vec![Scope::Read], false),
        (vec![Scope::Read, Scope::SensitiveRead], true),
    ] {
        let auth = AuthContext::new(
            Principal::Token {
                id: "tok_icon_01".to_owned(),
                name: "icon-token".to_owned(),
            },
            scopes.into_iter().collect(),
        );
        let response = with_http_auth(router.clone(), auth)
            .oneshot(
                Request::get(path)
                    .header("cookie", COOKIES)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()["cache-control"], "no-store");
        for (name, value) in icons::SECURITY_HEADERS {
            assert_eq!(response.headers()[name], value);
        }
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body_text = String::from_utf8_lossy(&body);
        let wire: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(wire["kind"], "application_error");
        if is_full {
            assert!(
                body_text.contains("secret-icon-canary"),
                "SensitiveRead icon error must retain canary diagnostic: {body_text}"
            );
            assert!(
                body_text.contains("user:pass"),
                "SensitiveRead icon error must retain URL credentials in diagnostic: {body_text}"
            );
        } else {
            assert!(
                !body_text.contains("secret-icon-canary")
                    && !body_text.contains("user:pass")
                    && !body_text.contains("upstream connect error"),
                "Read-only icon error must exclude secret URL and upstream text: {body_text}"
            );
            assert!(wire.pointer("/domain_error/detail").is_none());
            assert_eq!(wire["message"], "internal error");
            assert_eq!(wire["domain_error"]["kind"]["domain"], "unknown");
        }
    }

    fixture.shutdown().await;
}
