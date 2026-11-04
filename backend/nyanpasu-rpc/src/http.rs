//! The WebUI transport: an axum router over the registry and the event bus.
//! It serves only commands that opted into [`RpcTarget::Http`]. Access
//! control (the per-start credential and Host/Origin checks) belongs to the
//! listener that mounts this router.

use std::{convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    extract::{Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::{StreamExt as _, stream};
use nyanpasu_rpc_protocol::{EventName, RpcError, RpcOwner, RpcTarget};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{UnifiedRpc, application_error};

/// The browser session cookie that identifies an HTTP owner.
pub const SESSION_COOKIE: &str = "nyanpasu_http_session";

/// `POST /bridge/rpc`, `GET /bridge/events` (SSE) and
/// `GET /bridge/connection-details` (SSE).
pub fn router(rpc: UnifiedRpc) -> Router {
    Router::new()
        .route("/bridge/rpc", post(http_call))
        .route("/bridge/events", get(http_events))
        .route("/bridge/connection-details", get(http_connection_details))
        .with_state(rpc)
}

/// Answers a failed request with the envelope and the status its kind maps to.
pub fn error_response(error: RpcError) -> (StatusCode, Json<RpcError>) {
    let status = StatusCode::from_u16(error.http_status())
        .expect("every RPC error kind maps to a valid HTTP status");
    (status, Json(error))
}

fn session_owner(headers: &HeaderMap) -> Option<RpcOwner> {
    let cookie = headers.get("cookie")?.to_str().ok()?;
    let session = cookie
        .split(';')
        .map(str::trim)
        .find_map(|c| c.strip_prefix(SESSION_COOKIE)?.strip_prefix('='))?;
    uuid::Uuid::parse_str(session)
        .ok()
        .map(RpcOwner::http_session)
}

#[derive(Deserialize)]
struct Request {
    method: String,
    #[serde(default)]
    params: Value,
}

async fn http_call(
    State(rpc): State<UnifiedRpc>,
    headers: HeaderMap,
    request: Result<Json<Request>, JsonRejection>,
) -> Result<Json<Value>, (StatusCode, Json<RpcError>)> {
    let Json(request) =
        request.map_err(|error| error_response(RpcError::invalid_params(error.to_string())))?;
    // Sessions are required only for owner-scoped commands; ordinary calls may
    // use an anonymous owner. The listener authenticates every request before dispatch.
    let owner =
        session_owner(&headers).unwrap_or_else(|| RpcOwner::http_session(uuid::Uuid::new_v4()));
    rpc.call(RpcTarget::Http, &request.method, owner, request.params)
        .await
        .map(Json)
        .map_err(error_response)
}

#[derive(Deserialize)]
struct EventRequest {
    name: Option<String>,
}

async fn http_events(
    State(rpc): State<UnifiedRpc>,
    Query(request): Query<EventRequest>,
) -> Response {
    let name = match request.name.as_deref().map(str::parse::<EventName>) {
        None => None,
        Some(Ok(name)) => Some(name),
        Some(Err(_)) => {
            let name = request.name.as_deref().unwrap_or_default();
            return (StatusCode::NOT_FOUND, Json(RpcError::unknown_method(name))).into_response();
        }
    };
    let receiver = rpc.dependencies().events.subscribe();
    let events = stream::unfold((receiver, name), |(mut receiver, name)| async move {
        loop {
            match receiver.recv().await {
                Ok(event) if name.is_none_or(|name| event.name == name) => {
                    let payload = if name.is_some() {
                        event.payload
                    } else {
                        serde_json::json!({"name": event.name.as_str(), "payload": event.payload})
                    };
                    let data = serde_json::to_string(&payload).ok()?;
                    return Some((
                        Ok::<Event, Infallible>(Event::default().data(data)),
                        (receiver, name),
                    ));
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    return Some((
                        Ok(Event::default().event("resync").data("{}")),
                        (receiver, name),
                    ));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    let ready = stream::once(async { Ok::<Event, Infallible>(Event::default().comment("ready")) });
    Sse::new(ready.chain(events))
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

// Subscribe only while a page consumes full connection details. Dropping the
// SSE body releases the watch receiver, just like a native channel teardown.
async fn http_connection_details(
    State(rpc): State<UnifiedRpc>,
) -> Result<impl IntoResponse, (StatusCode, Json<RpcError>)> {
    let receiver = rpc
        .dependencies()
        .client
        .subscribe_clash_connection_details()
        .await
        .map_err(application_error)
        .map_err(error_response)?;
    Ok(Sse::new(connection_detail_events(receiver))
        .keep_alive(axum::response::sse::KeepAlive::default()))
}

fn connection_detail_events(
    receiver: tokio::sync::watch::Receiver<
        Option<Arc<nyanpasu_core::clash::ws::ClashConnectionDetails>>,
    >,
) -> impl futures::Stream<Item = Result<Event, Infallible>> {
    stream::unfold((receiver, true), |(mut receiver, mut first)| async move {
        loop {
            if !first && receiver.changed().await.is_err() {
                return None;
            }
            first = false;
            let frame = receiver.borrow_and_update().clone();
            if let Some(frame) = frame {
                let event = Event::default()
                    .json_data(frame.as_ref())
                    .expect("connection details serialize");
                return Some((Ok::<_, Infallible>(event), (receiver, false)));
            }
        }
    })
}

#[cfg(test)]
mod tests;
