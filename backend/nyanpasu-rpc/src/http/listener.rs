//! The loopback listener every WebUI host serves through: a per-start access
//! credential and Host/Origin checks in front of whatever router it serves,
//! with or without frontend assets.

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use futures::StreamExt as _;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::SESSION_COOKIE;

const ACCESS_COOKIE: &str = "nyanpasu_http_access";

/// A loopback TCP listener with a fresh access credential. Nothing is served
/// until [`HttpListener::serve`].
pub struct HttpListener {
    listener: TcpListener,
    session: LocalSession,
}

impl HttpListener {
    /// Binds `127.0.0.1:port`; port 0 picks a free port.
    ///
    /// # Errors
    ///
    /// Fails when the port cannot be bound.
    pub async fn bind_loopback(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        let authority = listener.local_addr()?.to_string();
        Ok(Self {
            listener,
            session: LocalSession {
                authority,
                access_token: uuid::Uuid::new_v4().to_string(),
            },
        })
    }

    /// The URL that exchanges the credential for the access and session
    /// cookies. It carries the credential, so hand it only to the user.
    pub fn bootstrap_url(&self) -> String {
        format!(
            "http://{}/?access_token={}",
            self.session.authority, self.session.access_token
        )
    }

    /// Serves `app` behind the credential and Host/Origin checks until
    /// `shutdown` is cancelled. Open response bodies, such as event streams,
    /// end with the shutdown instead of holding it up.
    ///
    /// # Errors
    ///
    /// Fails when the server stops on an IO error.
    pub async fn serve(self, app: Router, shutdown: CancellationToken) -> std::io::Result<()> {
        let session = Session {
            session: self.session,
            shutdown: shutdown.clone(),
        };
        let app = app.layer(middleware::from_fn_with_state(session, local_session));
        axum::serve(self.listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    }
}

#[derive(Clone)]
struct LocalSession {
    authority: String,
    access_token: String,
}

#[derive(Clone)]
struct Session {
    session: LocalSession,
    shutdown: CancellationToken,
}

/// Reject foreign origins and DNS rebinding before allowing local RPC access.
async fn local_session(State(state): State<Session>, request: Request, next: Next) -> Response {
    let Session { session, shutdown } = state;
    let authority = &session.authority;
    let host = request.headers().get("host").and_then(|v| v.to_str().ok());
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    if host != Some(authority.as_str())
        || origin.is_some_and(|o| o != format!("http://{authority}"))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let authenticated = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .any(|c| {
            c.strip_prefix(ACCESS_COOKIE)
                .and_then(|c| c.strip_prefix('='))
                == Some(session.access_token.as_str())
        });
    let bootstrap = request.method() == Method::GET
        && request.uri().path() == "/"
        && url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
            .any(|(key, value)| key == "access_token" && value == session.access_token);
    if bootstrap {
        let mut response = axum::response::Redirect::to("/").into_response();
        for cookie in [
            format!(
                "{ACCESS_COOKIE}={}; HttpOnly; SameSite=Strict; Path=/",
                session.access_token
            ),
            format!(
                "{SESSION_COOKIE}={}; HttpOnly; SameSite=Strict; Path=/",
                uuid::Uuid::new_v4()
            ),
        ] {
            response.headers_mut().append(
                "set-cookie",
                cookie
                    .parse()
                    .expect("cookies built from a UUID are valid header values"),
            );
        }
        response
            .headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-store"));
        response
            .headers_mut()
            .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
        return response;
    }
    if !authenticated {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        Body::from_stream(
            body.into_data_stream()
                .take_until(shutdown.cancelled_owned()),
        ),
    )
}
