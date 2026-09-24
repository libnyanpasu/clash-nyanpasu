use std::sync::Arc;
use std::{collections::BTreeMap, str::FromStr};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderName, HeaderValue, Method, Request, StatusCode, Uri},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower::ServiceExt;
use utoipa::{
    ToSchema,
    openapi::{OpenApi, path::Paths},
};
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Clone)]
pub struct FakeApplication(Arc<RwLock<Vec<Profile>>>);

impl Default for FakeApplication {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(vec![
            Profile {
                id: "profile-1".into(),
                name: "Primary".into(),
                active: true,
            },
            Profile {
                id: "profile-2".into(),
                name: "Backup".into(),
                active: false,
            },
        ])))
    }
}

impl FakeApplication {
    async fn profiles(&self) -> Vec<Profile> {
        self.0.read().await.clone()
    }

    async fn activate_profile(&self, profile_id: &str) -> Result<(), ()> {
        let mut profiles = self.0.write().await;
        if !profiles.iter().any(|profile| profile.id == profile_id) {
            return Err(());
        }

        for profile in profiles.iter_mut() {
            profile.active = profile.id == profile_id;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProfileActivation {
    pub profile_id: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProfileActivationRequest {
    pub profile_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub method: String,
    pub uri: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/profiles",
    tag = "profiles",
    operation_id = "listProfiles",
    responses(
        (status = 200, description = "List profiles", body = [Profile])
    )
)]
async fn list_profiles(State(app): State<FakeApplication>) -> Json<Vec<Profile>> {
    Json(app.profiles().await)
}

#[utoipa::path(
    post,
    path = "/api/v1/profiles/active",
    tag = "profiles",
    operation_id = "activateProfile",
    request_body = ProfileActivationRequest,
    responses(
        (status = 200, description = "Profile activation result", body = ProfileActivation),
        (status = 404, description = "Profile does not exist", body = ApiError)
    )
)]
async fn activate_profile(
    State(app): State<FakeApplication>,
    Json(request): Json<ProfileActivationRequest>,
) -> Result<Json<ProfileActivation>, (StatusCode, Json<ApiError>)> {
    app.activate_profile(&request.profile_id)
        .await
        .map_err(|()| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    code: "profile_not_found".into(),
                    message: format!("No profile exists with id `{}`", request.profile_id),
                }),
            )
        })?;

    Ok(Json(ProfileActivation {
        profile_id: request.profile_id,
        active: true,
    }))
}

fn openapi_router() -> OpenApiRouter<FakeApplication> {
    OpenApiRouter::with_openapi(OpenApi::new(
        utoipa::openapi::Info::new("Nyanpasu application API probe", "0.1.0"),
        Paths::new(),
    ))
    .routes(routes!(list_profiles))
    .routes(routes!(activate_profile))
}

pub fn application_api(app: FakeApplication) -> (Router, OpenApi) {
    let (router, openapi) = openapi_router().split_for_parts();
    (router.with_state(app), openapi)
}

/// JSON-only in-process HTTP adapter, shaped like the body of a Tauri bridge command.
pub async fn dispatch(router: Router, request: BridgeRequest) -> Result<BridgeResponse, String> {
    let method = Method::from_bytes(request.method.as_bytes())
        .map_err(|error| format!("invalid HTTP method: {error}"))?;
    let uri = Uri::from_str(&request.uri).map_err(|error| format!("invalid URI: {error}"))?;
    let mut builder = Request::builder().method(method).uri(uri);

    for (name, value) in request.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| format!("invalid header name: {error}"))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|error| format!("invalid header value: {error}"))?;
        builder = builder.header(name, value);
    }

    let request = builder
        .body(Body::from(request.body.unwrap_or_default()))
        .map_err(|error| format!("invalid HTTP request: {error}"))?;
    let response = router
        .oneshot(request)
        .await
        .map_err(|error| format!("route dispatch failed: {error}"))?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .map_err(|error| format!("response exceeds JSON bridge limit: {error}"))?;
    let body = String::from_utf8(body.to_vec())
        .map_err(|error| format!("response body is not UTF-8 JSON: {error}"))?;

    Ok(BridgeResponse {
        status,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::{FakeApplication, application_api};
    use axum::{
        body::Body,
        body::to_bytes,
        http::{Request, StatusCode},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    #[test]
    fn registered_routes_produce_a_complete_openapi_document() {
        let (_, openapi) = application_api(FakeApplication::default());
        let json: Value = serde_json::from_str(&openapi.to_json().unwrap()).unwrap();

        assert_eq!(json["openapi"], "3.1.0");
        assert!(json["paths"]["/api/v1/profiles"]["get"].is_object());
        assert!(json["paths"]["/api/v1/profiles/active"]["post"].is_object());
        assert!(json["components"]["schemas"]["Profile"].is_object());
    }

    #[tokio::test]
    async fn axum_router_can_be_dispatched_in_process() {
        let (router, _) = application_api(FakeApplication::default());
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/profiles/active")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"profile_id":"profile-2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, json!({ "profile_id": "profile-2", "active": true }));
    }

    #[tokio::test]
    async fn tauri_shaped_request_response_can_dispatch_without_a_listener() {
        let (router, _) = application_api(FakeApplication::default());
        let response = super::dispatch(
            router,
            super::BridgeRequest {
                method: "POST".into(),
                uri: "/api/v1/profiles/active".into(),
                headers: [("content-type".into(), "application/json".into())].into(),
                body: Some(r#"{"profile_id":"profile-2"}"#.into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(response.status, 200);
        assert!(response.headers.contains_key("content-type"));
        assert_eq!(
            serde_json::from_str::<Value>(&response.body).unwrap(),
            json!({ "profile_id": "profile-2", "active": true })
        );
    }

    #[tokio::test]
    async fn route_errors_are_structured_and_reported_by_status() {
        let (router, _) = application_api(FakeApplication::default());
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/profiles/active")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"profile_id":"missing"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "profile_not_found");
    }
}
