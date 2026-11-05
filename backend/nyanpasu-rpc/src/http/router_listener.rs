//! The listener a router (or any LAN) shell serves the WebUI through. Unlike
//! the desktop's [`HttpListener`](super::HttpListener) it binds a given
//! address, so a browser on another machine reaches it by whatever name the
//! machine has; there is no fixed authority to check the Host header against.
//!
//! Access is a session cookie that only a one-time ticket can create. A ticket
//! comes from the shell's `issue_web_ticket` command, so whoever may run that
//! command (on a router: root, through LuCI) decides who gets in. With a cookie
//! as the credential a rebinding page has nothing to ride on: the browser sends
//! no cookie to another origin, and a request that carries an `Origin` must
//! have come from this server's own pages.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::{SESSION_COOKIE, auth::WebAuth, listener::end_with_shutdown, session_id};
use crate::web_ticket::WebTickets;

/// A TCP listener that serves behind ticket-gated sessions. Nothing is served
/// until [`RouterListener::serve`].
pub struct RouterListener {
    listener: TcpListener,
    auth: Arc<WebAuth>,
}

impl RouterListener {
    /// Binds `address` and returns the listener with the issuer of its tickets,
    /// for the composition root to inject into the registry. Port 0 picks a
    /// free port.
    ///
    /// # Errors
    ///
    /// Fails when the address cannot be bound.
    pub async fn bind(address: SocketAddr) -> std::io::Result<(Self, WebTickets)> {
        let listener = TcpListener::bind(address).await?;
        let auth = Arc::new(WebAuth::new(listener.local_addr()?.port()));
        let tickets = WebTickets::new(auth.clone());
        Ok((Self { listener, auth }, tickets))
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves `app` behind the ticket and session checks until `shutdown` is
    /// cancelled. Open response bodies, such as event streams, end with the
    /// shutdown instead of holding it up.
    ///
    /// # Errors
    ///
    /// Fails when the server stops on an IO error.
    pub async fn serve(self, app: Router, shutdown: CancellationToken) -> std::io::Result<()> {
        let state = Access {
            auth: self.auth,
            shutdown: shutdown.clone(),
        };
        let app = app.layer(middleware::from_fn_with_state(state, access));
        axum::serve(self.listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    }
}

#[derive(Clone)]
struct Access {
    auth: Arc<WebAuth>,
    shutdown: CancellationToken,
}

async fn access(State(state): State<Access>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    // A browser sends Origin on cross-site and on fetch/XHR requests; only this
    // server's own pages may have one.
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin.is_some_and(|origin| {
        host.as_ref()
            .is_none_or(|host| origin != format!("http://{host}"))
    }) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = respond(&state, request, next).await;
    // Only this server's own pages are shown, never inside another page's frame.
    let headers = response.headers_mut();
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    response
}

async fn respond(state: &Access, request: Request, next: Next) -> Response {
    if request.method() == Method::GET
        && request.uri().path() == "/"
        && let Some(ticket) =
            url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
                .find_map(|(key, value)| (key == "ticket").then(|| value.into_owned()))
    {
        return exchange(&state.auth, &ticket);
    }
    let authenticated =
        session_id(request.headers()).is_some_and(|session| state.auth.is_session(session));
    if !authenticated {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let api = request.uri().path().starts_with("/bridge/");
    let mut response = next.run(request).await;
    if api {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    end_with_shutdown(response, state.shutdown.clone())
}

/// Turns a ticket into a session cookie and sends the browser to the app.
fn exchange(auth: &WebAuth, ticket: &str) -> Response {
    let Some(session) = auth.redeem(ticket) else {
        return (
            StatusCode::UNAUTHORIZED,
            "This sign-in link is invalid, expired or already used. \
             Open the WebUI from the router's admin page again.",
        )
            .into_response();
    };
    let mut response = Redirect::to("/").into_response();
    let cookie = format!("{SESSION_COOKIE}={session}; HttpOnly; SameSite=Strict; Path=/");
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie
            .parse()
            .expect("a cookie built from a UUID is a valid header value"),
    );
    // The ticket is in the address bar and must not leak through a Referer.
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(test)]
mod tests;
