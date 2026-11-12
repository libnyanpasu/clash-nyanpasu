//! Desktop URI adapter for the neutral icon entry; tasks join root shutdown.

use std::sync::Arc;

use nyanpasu_rpc::{
    audit::RpcTransport,
    icons,
    registry::{RpcDependencies, dispatch_audited, unreported},
};
use nyanpasu_rpc_protocol::{RpcError, RpcOwner};
use tauri::{
    Manager as _,
    http::{HeaderValue, Method, Request, Response, StatusCode},
};
use tokio_util::{
    sync::CancellationToken,
    task::{TaskTracker, task_tracker::TaskTrackerToken},
};

use super::native::DesktopRpc;

pub(crate) fn handle(
    context: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let app = context.app_handle();
    let Some(desktop) = app.try_state::<DesktopRpc>() else {
        responder.respond(unavailable());
        return;
    };
    // Acquire tracking before checking admission, so shutdown cannot miss an
    // accepted request between the check and spawn. Closed trackers still track.
    let Some(admission) = admit(&desktop.tasks, &desktop.shutdown) else {
        responder.respond(unavailable());
        return;
    };
    let dependencies = desktop.icon_dependencies();
    let owner = RpcOwner::desktop(context.webview_label());
    let dev_origin = if tauri::is_dev() {
        app.config()
            .build
            .dev_url
            .as_ref()
            .map(|url| url.origin().ascii_serialization())
    } else {
        None
    };
    desktop.tasks.spawn_on(
        async move {
            responder.respond(response(dependencies, owner, request, dev_origin.as_deref()).await);
        },
        tauri::async_runtime::handle().inner(),
    );
    drop(admission);
}

fn admit(tasks: &TaskTracker, shutdown: &CancellationToken) -> Option<TaskTrackerToken> {
    let token = tasks.token();
    (!shutdown.is_cancelled()).then_some(token)
}

fn unavailable() -> Response<Vec<u8>> {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        RpcError::new("unavailable", "icon entry is not accepting requests"),
    )
}

fn cors_origin(
    request: &Request<Vec<u8>>,
    dev_origin: Option<&str>,
) -> Result<Option<HeaderValue>, RpcError> {
    let mut origins = request.headers().get_all("origin").iter();
    let Some(origin) = origins.next() else {
        return Ok(None);
    };
    let value = origin
        .to_str()
        .map_err(|_| RpcError::new("forbidden", "invalid icon origin"))?;
    let packaged = if cfg!(windows) {
        matches!(value, "http://tauri.localhost" | "https://tauri.localhost")
    } else {
        value == "tauri://localhost"
    };
    if origins.next().is_some() || !(packaged || dev_origin == Some(value)) {
        return Err(RpcError::new("forbidden", "invalid icon origin"));
    }
    Ok(Some(origin.clone()))
}

async fn response(
    dependencies: Arc<RpcDependencies>,
    owner: RpcOwner,
    request: Request<Vec<u8>>,
    dev_origin: Option<&str>,
) -> Response<Vec<u8>> {
    let mut origin = None;
    let result = dispatch_audited(
        &dependencies.audit,
        RpcTransport::TauriProtocol,
        owner,
        || "icon_fetch",
        None,
        || {
            if request.method() != Method::GET {
                return Err(RpcError::new(
                    "method_not_allowed",
                    "icon entry requires GET",
                ));
            }
            origin = cors_origin(&request, dev_origin)?;
            let input = icons::parse(request.uri().path(), request.uri().query())?;
            Ok(unreported(icons::read(&dependencies, input)))
        },
    )
    .await;
    let mut response = match result {
        Ok(body) => {
            let mut response = Response::new(body.bytes);
            response
                .headers_mut()
                .insert("content-type", HeaderValue::from_static(body.content_type));
            response.headers_mut().insert(
                "cache-control",
                HeaderValue::from_static(body.cache_policy.header()),
            );
            secure(response)
        }
        Err(error) => {
            let status = match error.kind {
                "invalid_params" => StatusCode::BAD_REQUEST,
                "invalid_icon" => StatusCode::UNPROCESSABLE_ENTITY,
                "unavailable" => StatusCode::NOT_IMPLEMENTED,
                "forbidden" => StatusCode::FORBIDDEN,
                "method_not_allowed" => StatusCode::METHOD_NOT_ALLOWED,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, error)
        }
    };
    response
        .headers_mut()
        .insert("vary", HeaderValue::from_static("Origin"));
    if let Some(origin) = origin {
        response
            .headers_mut()
            .insert("access-control-allow-origin", origin);
    }
    response
}

fn error_response(status: StatusCode, error: RpcError) -> Response<Vec<u8>> {
    let mut response = Response::new(serde_json::to_vec(&error).expect("RPC errors serialize"));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    secure(response)
}

fn secure(mut response: Response<Vec<u8>>) -> Response<Vec<u8>> {
    for (name, value) in icons::SECURITY_HEADERS {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

#[cfg(test)]
mod tests {
    use nyanpasu_rpc::{
        audit::{AuditPhase, RpcTransport},
        test_support::Fixture,
    };
    use nyanpasu_rpc_protocol::RpcOwner;
    use std::future::Future as _;
    use tauri::http::{Request, StatusCode};

    #[tokio::test]
    async fn desktop_icon_requests_join_shutdown_and_refuse_late_admission() {
        let tasks = tokio_util::task::TaskTracker::new();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let accepted = super::admit(&tasks, &shutdown).unwrap();
        shutdown.cancel();
        tasks.close();
        assert!(super::admit(&tasks, &shutdown).is_none());
        assert!(
            std::pin::pin!(tasks.wait())
                .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
                .is_pending()
        );
        drop(accepted);
        tasks.wait().await;
    }

    #[tokio::test]
    async fn desktop_icon_protocol_uses_the_shared_parser() {
        let fixture = Fixture::new(false).await;
        for (path, status) in [
            ("/tray?mode=tun&mode=normal", StatusCode::BAD_REQUEST),
            (
                "/cached?url=file%3A%2F%2F%2Fsecret",
                StatusCode::BAD_REQUEST,
            ),
            ("/widget?mode=normal&version=4", StatusCode::NOT_IMPLEMENTED),
        ] {
            for base in [
                "nyanpasu-icons://localhost",
                "http://nyanpasu-icons.localhost",
            ] {
                let request = Request::get(format!("{base}{path}")).body(vec![]).unwrap();
                let response = super::response(
                    fixture.rpc.dependencies(),
                    RpcOwner::desktop("main"),
                    request,
                    None,
                )
                .await;
                assert_eq!(response.status(), status);
            }
        }
        {
            let records = fixture.sink.records.lock();
            assert!(records.iter().all(|record| record.header.transport
                == RpcTransport::TauriProtocol
                && record.header.method == "icon_fetch"
                && record.header.owner.as_str() == "main"));
            assert_eq!(
                records
                    .iter()
                    .filter(|record| matches!(record.phase, AuditPhase::Admitted))
                    .count(),
                2
            );
            assert!(!format!("{records:?}").contains("secret"));
        }
        fixture.shutdown().await;
    }

    #[test]
    fn icon_document_responses_require_script_and_origin_isolation() {
        let response = super::unavailable();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            response.headers()["content-security-policy"],
            "sandbox; default-src 'none'"
        );
        for (origin, allowed) in [
            (None, true),
            (Some("http://localhost:3000"), true),
            (Some("http://localhost:3001"), false),
            (Some("null"), false),
            (Some("http://nyanpasu-icons.localhost"), false),
            (Some("https://foreign.example"), false),
        ] {
            let mut request = Request::get("nyanpasu-icons://localhost/tray?mode=normal");
            if let Some(origin) = origin {
                request = request.header("origin", origin);
            }
            assert_eq!(
                super::cors_origin(
                    &request.body(vec![]).unwrap(),
                    Some("http://localhost:3000")
                )
                .is_ok(),
                allowed
            );
        }
    }
}
