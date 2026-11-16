//! Embedded `/login` page and `/bridge/login` + `/bridge/logout` cookie endpoints.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::{ConnectInfo, Extension, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use nyanpasu_contracts::auth::{SecretString, parse_access_token};
use nyanpasu_rpc_protocol::RpcError;
use serde::Deserialize;

use super::auth::{HttpAuthError, HttpAuthenticator};

pub(crate) const LOGIN_PATH: &str = "/login";
pub(crate) const LOGIN_HTML: &str = include_str!("login.html");

pub(crate) const LOGIN_CSP: &str = concat!(
    "default-src 'none'; ",
    "script-src 'unsafe-inline'; ",
    "style-src 'unsafe-inline'; ",
    "connect-src 'self'; ",
    "img-src 'none'; ",
    "object-src 'none'; ",
    "base-uri 'none'; ",
    "frame-ancestors 'none'; ",
    "form-action 'none'"
);

pub(crate) const SESSION_COOKIE_NAME: &str = "nyanpasu_session";

#[derive(Clone)]
pub(crate) struct LoginState {
    pub(crate) authenticator: Arc<dyn HttpAuthenticator>,
}

#[derive(Deserialize)]
pub(crate) struct LoginRequest {
    token: String,
}

pub(crate) async fn login_page(method: Method) -> Response {
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(LOGIN_HTML)
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("content-security-policy", LOGIN_CSP)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
        .header("referrer-policy", "no-referrer")
        .body(body)
        .unwrap()
}

pub(crate) fn extract_cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    for header in headers.get_all("cookie") {
        let Ok(raw) = header.to_str() else {
            continue;
        };
        for part in raw.split(';').map(str::trim) {
            if let Some(value) = part.strip_prefix(&prefix) {
                return Some(value);
            }
        }
    }
    None
}

pub(crate) fn session_set_cookie(cookie: &SecretString) -> String {
    format!(
        "{SESSION_COOKIE_NAME}={}; HttpOnly; SameSite=Strict; Path=/",
        cookie.expose_secret()
    )
}

pub(crate) fn clear_session_set_cookie() -> String {
    format!("{SESSION_COOKIE_NAME}=; Max-Age=0; HttpOnly; SameSite=Strict; Path=/")
}

pub(crate) fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

pub(crate) async fn login_submit(
    State(state): State<LoginState>,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Response {
    if !is_json_content_type(&headers) {
        return no_store_json(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            RpcError::invalid_params("content-type must be application/json"),
        );
    }
    let peer = connect_info.map(|Extension(ConnectInfo(addr))| addr);
    let request = match payload {
        Ok(Json(request)) => request,
        Err(_) => {
            return no_store_json(
                StatusCode::BAD_REQUEST,
                RpcError::invalid_params("invalid login payload"),
            );
        }
    };
    let token_id = parse_access_token(&request.token).map(|parsed| parsed.id.to_string());
    match state
        .authenticator
        .login_with_token(SecretString::new(request.token))
        .await
    {
        Ok(established) => {
            let mut response = (
                StatusCode::OK,
                Json(serde_json::json!({ "principal": established.auth.principal })),
            )
                .into_response();
            response.headers_mut().append(
                "set-cookie",
                session_set_cookie(&established.cookie).parse().unwrap(),
            );
            response
                .headers_mut()
                .insert("cache-control", HeaderValue::from_static("no-store"));
            response
                .headers_mut()
                .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
            response
        }
        Err(HttpAuthError::Unauthorized) => {
            tracing::warn!(
                target: "nyanpasu::remote_access",
                peer = ?peer,
                token_id = ?token_id,
                "HTTP token login rejected"
            );
            no_store_json(
                StatusCode::UNAUTHORIZED,
                RpcError::unauthorized("authentication required"),
            )
        }
        Err(HttpAuthError::Unavailable) => {
            tracing::warn!(
                target: "nyanpasu::remote_access",
                peer = ?peer,
                token_id = ?token_id,
                "HTTP token login unavailable"
            );
            no_store_json(
                StatusCode::SERVICE_UNAVAILABLE,
                RpcError::new("unavailable", "authentication service unavailable"),
            )
        }
    }
}

pub(crate) async fn login_logout(State(state): State<LoginState>, headers: HeaderMap) -> Response {
    if let Some(cookie) = extract_cookie_value(&headers, SESSION_COOKIE_NAME)
        && !cookie.is_empty()
    {
        let _ = state
            .authenticator
            .logout(SecretString::new(cookie.to_owned()))
            .await;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append("set-cookie", clear_session_set_cookie().parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}

fn no_store_json(status: StatusCode, error: RpcError) -> Response {
    let mut response = (status, Json(error)).into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}
