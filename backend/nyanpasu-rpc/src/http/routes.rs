//! Audited HTTP command, archive and subscription routes.
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    Extension, Json, Router,
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
use nyanpasu_rpc_protocol::{
    AuthContext, EventName, Request, RpcError, RpcOwner, RpcTarget, Scope, ServerInfo,
    ShellCapability,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::{
    audit::{AuditInvocation, AuditPhase, DeliveryOutcome, RpcTransport, SubscriptionOutcome},
    error::application_error,
    http::HttpSessionRevocation,
    registry::{
        CommandHandlers, RpcCaller, RpcDependencies, RpcFuture, SetupError, UnifiedRpc,
        dispatch_with_audit, unreported,
    },
};

fn response_prepared(audit: &AuditInvocation, response: Response) -> Response {
    audit.record(AuditPhase::Delivery {
        outcome: DeliveryOutcome::ResponsePrepared,
    });
    response
}

/// The inner stream releases its receiver before recording termination, even if
/// its body is dropped without ever being polled. No frame delivery is audited.
struct AuditedSubscription<S> {
    stream: Option<Pin<Box<S>>>,
    audit: AuditInvocation,
}

impl<S> AuditedSubscription<S> {
    fn new(stream: S, audit: &AuditInvocation) -> Self {
        audit.record(AuditPhase::Subscription {
            outcome: SubscriptionOutcome::Established,
        });
        Self {
            stream: Some(Box::pin(stream)),
            audit: audit.clone(),
        }
    }
}

impl<S: futures::Stream> futures::Stream for AuditedSubscription<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let Some(stream) = &mut this.stream else {
            return Poll::Ready(None);
        };
        let result = stream.as_mut().poll_next(cx);
        if matches!(result, Poll::Ready(None)) {
            drop(this.stream.take());
            this.audit.record(AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Failed,
            });
        }
        result
    }
}

impl<S> Drop for AuditedSubscription<S> {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            drop(stream);
            self.audit.record(AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Disconnected,
            });
        }
    }
}

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
pub struct RpcHttpRoutes(
    std::sync::OnceLock<(
        std::sync::Weak<RpcDependencies>,
        ServerInfo,
        Vec<ShellCapability>,
        CommandHandlers,
    )>,
);

impl RpcHttpRoutes {
    pub fn install(&self, rpc: &UnifiedRpc) -> Result<(), SetupError> {
        self.0
            .set((
                Arc::downgrade(&rpc.dependencies),
                rpc.server.clone(),
                rpc.provided.clone(),
                rpc.commands.clone(),
            ))
            .map_err(|_| SetupError::InstallRoutes)
    }
}
impl super::HttpRoutes for RpcHttpRoutes {
    fn build(&self) -> anyhow::Result<Router> {
        let (dependencies, server, provided, commands) = self
            .0
            .get()
            .ok_or_else(|| anyhow::anyhow!("HTTP routes not installed"))?;
        Ok(UnifiedRpc {
            dependencies: dependencies
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("application has shut down"))?,
            server: server.clone(),
            provided: provided.clone(),
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
            .route("/bridge/icons/{*path}", get(http_icon))
            .route("/bridge/events", get(http_events))
            .route("/bridge/connection-details", get(http_connection_details))
            .with_state(self)
    }
}

fn until_cancelled<S>(
    stream: S,
    revocation: Option<CancellationToken>,
) -> impl futures::Stream<Item = S::Item>
where
    S: futures::Stream + Unpin,
{
    stream::unfold(
        (stream, revocation),
        |(mut stream, revocation)| async move {
            match revocation {
                Some(token) => tokio::select! {
                    biased;
                    _ = token.cancelled() => None,
                    item = stream.next() => item.map(|item| (item, (stream, Some(token)))),
                },
                None => stream.next().await.map(|item| (item, (stream, None))),
            }
        },
    )
}

async fn http_icon(
    State(rpc): State<UnifiedRpc>,
    auth: Option<Extension<AuthContext>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    let auth = auth.map(|Extension(auth)| auth);
    let audit = rpc.dependencies.audit.invocation(
        RpcTransport::Http,
        &http_owner_or_anonymous(&headers),
        auth.as_ref().map(|auth| &auth.principal),
        "icon_fetch",
        None,
    );
    let result = dispatch_with_audit(&audit, || {
        crate::policy::admit_route(Scope::Read, auth.as_ref())?;
        http_owner(&headers)?;
        let path = uri
            .path()
            .strip_prefix("/bridge/icons")
            .ok_or_else(|| RpcError::invalid_params("invalid icon path"))?;
        let request = crate::icons::parse(path, uri.query())?;
        Ok(unreported(async {
            let body = crate::icons::read(&rpc.dependencies, request).await?;
            let mut response = Response::builder()
                .header("content-type", body.content_type)
                .body(Body::from(body.bytes))
                .map_err(application_error)?;
            response.extensions_mut().insert(body.cache_policy);
            Ok(response)
        }))
    })
    .await;
    let mut response = result.unwrap_or_else(|error| {
        let status = match error.kind {
            "invalid_icon" => StatusCode::UNPROCESSABLE_ENTITY,
            "unavailable" => StatusCode::NOT_IMPLEMENTED,
            _ => http_status(&error),
        };
        (status, Json(error)).into_response()
    });
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    for (name, value) in crate::icons::SECURITY_HEADERS {
        response
            .headers_mut()
            .insert(name, axum::http::HeaderValue::from_static(value));
    }
    response_prepared(&audit, response)
}

/// Exports the diagnostic log archive for HTTP callers.
///
/// The text redactor in `nyanpasu_config::redact` leaves opaque URL host/path tokens intact, so
/// shared authorization must gate this route on `Scope::SensitiveRead` until a token-safe archive
/// view exists.
async fn http_logs_archive(
    State(rpc): State<UnifiedRpc>,
    auth: Option<Extension<AuthContext>>,
    headers: HeaderMap,
) -> Response {
    let auth = auth.map(|Extension(auth)| auth);
    let audit = rpc.dependencies.audit.invocation(
        RpcTransport::Http,
        &http_owner_or_anonymous(&headers),
        auth.as_ref().map(|auth| &auth.principal),
        "logs_archive",
        None,
    );
    let result = dispatch_with_audit(&audit, || {
        crate::policy::admit_route(Scope::SensitiveRead, auth.as_ref())?;
        Ok(unreported(async {
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
        }))
    })
    .await
    .unwrap_or_else(|error| http_rpc_error(error).into_response());
    response_prepared(&audit, result)
}

fn http_status(error: &RpcError) -> StatusCode {
    match error.kind {
        "unauthorized" => StatusCode::UNAUTHORIZED,
        "forbidden" => StatusCode::FORBIDDEN,
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
    auth: Option<Extension<AuthContext>>,
    revocation: Option<Extension<HttpSessionRevocation>>,
    headers: HeaderMap,
) -> Response {
    let auth = auth.map(|Extension(auth)| auth);
    let revocation = revocation.map(|Extension(revocation)| revocation.0);
    let audit = rpc.dependencies.audit.invocation(
        RpcTransport::Http,
        &http_owner_or_anonymous(&headers),
        auth.as_ref().map(|auth| &auth.principal),
        "connection_details_subscribe",
        None,
    );
    let result = dispatch_with_audit(&audit, || {
        crate::policy::admit_route(Scope::Read, auth.as_ref())?;
        Ok(unreported(async {
            let receiver = rpc
                .dependencies
                .client
                .subscribe_clash_connection_details()
                .await
                .map_err(|error| application_error(crate::error::IpcError::from(error)))?;
            let stream = until_cancelled(Box::pin(connection_detail_events(receiver)), revocation);
            Ok(Sse::new(AuditedSubscription::new(stream, &audit))
                .keep_alive(axum::response::sse::KeepAlive::default())
                .into_response())
        }))
    })
    .await
    .unwrap_or_else(|error| {
        audit.record(AuditPhase::Subscription {
            outcome: SubscriptionOutcome::Failed,
        });
        http_rpc_error(error).into_response()
    });
    response_prepared(&audit, result)
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
    auth: Option<Extension<AuthContext>>,
    revocation: Option<Extension<HttpSessionRevocation>>,
    headers: HeaderMap,
    request: Result<Query<EventRequest>, QueryRejection>,
) -> Response {
    let auth = auth.map(|Extension(auth)| auth);
    let revocation = revocation.map(|Extension(revocation)| revocation.0);
    let owner = http_owner_or_anonymous(&headers);
    let audit = rpc.dependencies.audit.invocation(
        RpcTransport::Http,
        &owner,
        auth.as_ref().map(|auth| &auth.principal),
        "events_subscribe",
        None,
    );
    let request = match request {
        Ok(Query(request)) => request,
        Err(error) => {
            let rejection = if auth.is_none() {
                let _ = dispatch_with_audit(&audit, || {
                    Err::<RpcFuture, _>(RpcError::unauthorized("authentication required"))
                })
                .await;
                http_rpc_error(RpcError::unauthorized("authentication required")).into_response()
            } else {
                let _ = dispatch_with_audit(&audit, || {
                    Err::<RpcFuture, _>(RpcError::invalid_params(error.to_string()))
                })
                .await;
                error.into_response()
            };
            audit.record(AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Failed,
            });
            return response_prepared(&audit, rejection);
        }
    };
    let audit_stream = audit.clone();
    let result = dispatch_with_audit(&audit, || {
        let caller = auth.ok_or_else(|| RpcError::unauthorized("authentication required"))?;
        if let Some(name) = request.name.as_deref() {
            let event = EventName::from_name(name).ok_or_else(|| RpcError::unknown_method(name))?;
            crate::policy::admit_event(event, &rpc.provided, RpcTarget::Http, Some(&caller))?;
        }
        Ok(unreported(async move {
            let receiver = rpc.dependencies.events.subscribe();
            let name = request.name;
            let provided = rpc.provided.clone();
            let events = stream::unfold(
                (receiver, name, provided, caller),
                |(mut receiver, name, provided, caller)| async move {
                    loop {
                        match receiver.recv().await {
                            Ok((event_name, payload))
                                if name.as_ref().is_none_or(|name| event_name == name) =>
                            {
                                let visible =
                                    EventName::from_name(event_name).is_some_and(|event| {
                                        crate::policy::event_visible_to(
                                            event,
                                            &provided,
                                            RpcTarget::Http,
                                            &caller,
                                        )
                                    });
                                if !visible {
                                    continue;
                                }
                                let payload = if name.is_some() {
                                    payload
                                } else {
                                    serde_json::json!({"name": event_name, "payload": payload})
                                };
                                let data = serde_json::to_string(&payload).ok()?;
                                return Some((
                                    Ok::<Event, Infallible>(Event::default().data(data)),
                                    (receiver, name, provided, caller),
                                ));
                            }
                            Ok(_) => continue,
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                return Some((
                                    Ok(Event::default().event("resync").data("{}")),
                                    (receiver, name, provided, caller),
                                ));
                            }
                            Err(broadcast::error::RecvError::Closed) => return None,
                        }
                    }
                },
            );
            let ready =
                stream::once(async { Ok::<Event, Infallible>(Event::default().comment("ready")) });
            let stream = until_cancelled(Box::pin(ready.chain(events)), revocation);
            Ok(Sse::new(AuditedSubscription::new(stream, &audit_stream))
                .keep_alive(axum::response::sse::KeepAlive::default())
                .into_response())
        }))
    })
    .await
    .unwrap_or_else(|error| {
        audit.record(AuditPhase::Subscription {
            outcome: SubscriptionOutcome::Failed,
        });
        http_rpc_error(error).into_response()
    });
    response_prepared(&audit, result)
}

async fn http_call(
    State(rpc): State<UnifiedRpc>,
    auth: Option<Extension<AuthContext>>,
    headers: HeaderMap,
    request: Result<Json<Request>, JsonRejection>,
) -> Response {
    let auth = auth.map(|Extension(auth)| auth);
    let owner = http_owner_or_anonymous(&headers);
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            let audit = rpc.dependencies.audit.invocation(
                RpcTransport::Http,
                &owner,
                auth.as_ref().map(|auth| &auth.principal),
                "invalid_envelope",
                None,
            );
            let rpc_error = if auth.is_none() {
                RpcError::unauthorized("authentication required")
            } else {
                RpcError::invalid_params(error.to_string())
            };
            let result = dispatch_with_audit(&audit, || Err::<RpcFuture, _>(rpc_error))
                .await
                .map(Json::<Value>)
                .map_err(http_rpc_error)
                .into_response();
            return response_prepared(&audit, result);
        }
    };
    let audit = rpc.audit_invocation(
        RpcTransport::Http,
        &request.method,
        &owner,
        auth.as_ref().map(|auth| &auth.principal),
        None,
    );
    let caller = auth.map(|auth| RpcCaller::new(auth, owner));
    let result = rpc
        .call_with_admission(
            &audit,
            RpcTransport::Http,
            &request.method,
            caller,
            request.params,
            || Ok(()),
        )
        .await
        .map(Json)
        .map_err(|error| (http_status(&error), Json(error)))
        .into_response();
    response_prepared(&audit, result)
}

#[cfg(test)]
mod tests {
    use std::{
        pin::Pin,
        sync::Arc,
        task::{Context, Poll},
    };

    use futures::{Stream, StreamExt as _};
    use nyanpasu_rpc_protocol::RpcOwner;
    use tokio::sync::watch;

    use super::{AuditedSubscription, connection_detail_events};
    use crate::audit::{
        AuditPhase, RpcAudit, RpcAuditRecord, RpcAuditSink, RpcTransport, SubscriptionOutcome,
    };

    struct TeardownSink(
        watch::Sender<Option<Arc<nyanpasu_core::clash::ws::ClashConnectionDetails>>>,
    );

    impl RpcAuditSink for TeardownSink {
        fn record(&self, record: &RpcAuditRecord) {
            if matches!(
                record.phase,
                AuditPhase::Subscription {
                    outcome: SubscriptionOutcome::Disconnected | SubscriptionOutcome::Failed
                }
            ) {
                assert_eq!(
                    self.0.receiver_count(),
                    0,
                    "HTTP receiver is released before its audit fact"
                );
            }
        }
    }

    #[tokio::test]
    async fn http_subscription_audit_follows_receiver_teardown() {
        let (sender, receiver) = watch::channel(None);
        let sink = Arc::new(TeardownSink(sender));
        let audit = RpcAudit::new(sink.clone()).invocation(
            RpcTransport::Http,
            &RpcOwner::desktop("test"),
            None,
            "connection_details_subscribe",
            None,
        );
        let events = AuditedSubscription::new(connection_detail_events(receiver), &audit);
        assert_eq!(sink.0.receiver_count(), 1);
        drop(events);
        assert_eq!(
            sink.0.receiver_count(),
            0,
            "unpolled stream still tears down"
        );

        struct Ended(
            watch::Receiver<Option<Arc<nyanpasu_core::clash::ws::ClashConnectionDetails>>>,
        );
        impl Stream for Ended {
            type Item = ();
            fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<()>> {
                assert!(!self.0.has_changed().unwrap());
                Poll::Ready(None)
            }
        }
        let mut events = AuditedSubscription::new(Ended(sink.0.subscribe()), &audit);
        assert_eq!(events.next().await, None);
        assert_eq!(
            sink.0.receiver_count(),
            0,
            "upstream end also releases demand before audit"
        );
    }
}
