//! Audited HTTP command, archive and subscription routes.
use std::{convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{
        Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::{StreamExt, stream};
use nyanpasu_rpc_protocol::{EventName, Request, RpcError, RpcOwner};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    error::application_error,
    registry::{
        CommandHandlers, RpcDependencies, RpcTransport, SetupError, UnifiedRpc, dispatch_audited,
    },
};

fn http_owner(headers: &HeaderMap) -> Result<RpcOwner, RpcError> {
    let cookie = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let session = cookie
        .split(';')
        .map(str::trim)
        .find_map(|c| c.strip_prefix("nyanpasu_http_session="))
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or_else(|| RpcError::invalid_params("HTTP session cookie is missing"))?;
    Ok(RpcOwner::new(format!("http:{session}")))
}

fn http_owner_or_anonymous(headers: &HeaderMap) -> RpcOwner {
    // A session identifies an owner; authentication belongs to the listener.
    http_owner(headers).unwrap_or_else(|_| RpcOwner::new(format!("http:{}", uuid::Uuid::new_v4())))
}

/// Filled once by the composition root after the application facade exists.
/// A weak dependency graph avoids an idle HTTP actor retaining its own client.
#[derive(Default)]
pub struct RpcHttpRoutes(std::sync::OnceLock<(std::sync::Weak<RpcDependencies>, CommandHandlers)>);

impl RpcHttpRoutes {
    pub fn install(&self, rpc: &UnifiedRpc) -> Result<(), SetupError> {
        self.0
            .set((Arc::downgrade(&rpc.dependencies), rpc.commands.clone()))
            .map_err(|_| SetupError::InstallRoutes)
    }
}
impl super::HttpRoutes for RpcHttpRoutes {
    fn build(&self) -> anyhow::Result<Router> {
        let (dependencies, commands) = self
            .0
            .get()
            .ok_or_else(|| anyhow::anyhow!("HTTP routes not installed"))?;
        Ok(UnifiedRpc {
            dependencies: dependencies
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("application has shut down"))?,
            commands: commands.clone(),
        }
        .router())
    }
}

impl UnifiedRpc {
    pub fn router(self) -> Router {
        Router::new()
            .route("/bridge/rpc", post(http_call))
            .route("/bridge/logs/archive", get(http_logs_archive))
            .route("/bridge/events", get(http_events))
            .route("/bridge/connection-details", get(http_connection_details))
            .with_state(self)
    }
}

async fn http_logs_archive(
    State(rpc): State<UnifiedRpc>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<RpcError>)> {
    dispatch_audited(
        &rpc.dependencies.audit,
        RpcTransport::Http,
        http_owner_or_anonymous(&headers),
        || "logs_archive",
        async {
            let paths = rpc.dependencies.paths.clone();
            let archive =
                crate::commands::archive::logs_archive(paths, rpc.dependencies.client.clone())
                    .await
                    .map_err(|error| RpcError::new("application_error", error.to_string()))?;
            let file = tokio::fs::File::open(&archive)
                .await
                .map_err(application_error)?;
            let body = stream::try_unfold((file, archive), |(mut file, _archive)| async move {
                use tokio::io::AsyncReadExt;

                let mut bytes = vec![0; 64 * 1024];
                let count = file.read(&mut bytes).await?;
                if count == 0 {
                    return Ok::<_, std::io::Error>(None);
                }
                bytes.truncate(count);
                Ok(Some((bytes, (file, _archive))))
            });
            let file_name = format!("{}-log.zip", chrono::Local::now().format("%Y-%m-%d"));
            Response::builder()
                .header("content-type", "application/zip")
                .header(
                    "content-disposition",
                    format!("attachment; filename=\"{file_name}\""),
                )
                .body(Body::from_stream(body))
                .map_err(application_error)
        },
    )
    .await
    .map_err(http_rpc_error)
}

fn http_status(error: &RpcError) -> StatusCode {
    match error.kind {
        "invalid_params" => StatusCode::BAD_REQUEST,
        "unknown_method" => StatusCode::NOT_FOUND,
        "unsupported" => StatusCode::NOT_IMPLEMENTED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn http_rpc_error(error: RpcError) -> (StatusCode, Json<RpcError>) {
    (http_status(&error), Json(error))
}

// Subscribe only while a page consumes full connection details. Dropping the
// SSE body releases the watch receiver, just like a native channel teardown.
async fn http_connection_details(
    State(rpc): State<UnifiedRpc>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<RpcError>)> {
    dispatch_audited(
        &rpc.dependencies.audit,
        RpcTransport::Http,
        http_owner_or_anonymous(&headers),
        || "connection_details_subscribe",
        async {
            let receiver = rpc
                .dependencies
                .client
                .subscribe_clash_connection_details()
                .await
                .map_err(application_error)?;
            Ok(Sse::new(connection_detail_events(receiver))
                .keep_alive(axum::response::sse::KeepAlive::default()))
        },
    )
    .await
    .map_err(http_rpc_error)
}

pub(crate) fn connection_detail_events(
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

#[derive(Deserialize)]
struct EventRequest {
    name: Option<String>,
}

async fn http_events(
    State(rpc): State<UnifiedRpc>,
    headers: HeaderMap,
    request: Result<Query<EventRequest>, QueryRejection>,
) -> Response {
    let owner = http_owner_or_anonymous(&headers);
    let request = match request {
        Ok(Query(request)) => request,
        Err(error) => {
            let _ = dispatch_audited(
                &rpc.dependencies.audit,
                RpcTransport::Http,
                owner,
                || "events_subscribe",
                async { Err::<(), _>(RpcError::invalid_params(error.to_string())) },
            )
            .await;
            return error.into_response();
        }
    };
    dispatch_audited(
        &rpc.dependencies.audit,
        RpcTransport::Http,
        owner,
        || "events_subscribe",
        async {
            if request
                .name
                .as_ref()
                .is_some_and(|name| !EventName::ALL.iter().any(|event| event.as_str() == name))
            {
                return Err(RpcError::unknown_method(request.name.as_deref().unwrap()));
            }
            let receiver = rpc.dependencies.events.subscribe();
            let name = request.name;
            let events = stream::unfold((receiver, name), |(mut receiver, name)| async move {
                loop {
                    match receiver.recv().await {
                        Ok((event_name, payload))
                            if name.as_ref().is_none_or(|name| event_name == name) =>
                        {
                            let payload = if name.is_some() {
                                payload
                            } else {
                                serde_json::json!({"name": event_name, "payload": payload})
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
            let ready =
                stream::once(async { Ok::<Event, Infallible>(Event::default().comment("ready")) });
            Ok(Sse::new(ready.chain(events))
                .keep_alive(axum::response::sse::KeepAlive::default())
                .into_response())
        },
    )
    .await
    .unwrap_or_else(|error| http_rpc_error(error).into_response())
}

async fn http_call(
    State(rpc): State<UnifiedRpc>,
    headers: HeaderMap,
    request: Result<Json<Request>, JsonRejection>,
) -> Result<Json<Value>, (StatusCode, Json<RpcError>)> {
    let owner = http_owner_or_anonymous(&headers);
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return dispatch_audited(
                &rpc.dependencies.audit,
                RpcTransport::Http,
                owner,
                || "invalid_envelope",
                async { Err(RpcError::invalid_params(error.to_string())) },
            )
            .await
            .map_err(http_rpc_error);
        }
    };
    rpc.call(RpcTransport::Http, &request.method, owner, request.params)
        .await
        .map(Json)
        .map_err(|error| (http_status(&error), Json(error)))
}
