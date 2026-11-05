use axum::http::StatusCode;
use nyanpasu_rpc_protocol::ShellCapability;
use serde_json::Value;

use super::*;
use crate::EventBus;

struct Served {
    base: String,
    tickets: WebTickets,
    http: reqwest::Client,
    shutdown: CancellationToken,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    // Keeps the registry's database and paths alive for the test.
    _directory: tempfile::TempDir,
}

impl Served {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let rpc = crate::registry::tests::test_rpc(
            &directory,
            EventBus::new(Vec::new()),
            &ShellCapability::ALL,
        )
        .await;
        let (listener, tickets) = RouterListener::bind(([127, 0, 0, 1], 0).into())
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let server = tokio::spawn(listener.serve(super::super::router(rpc), shutdown.clone()));
        Self {
            base,
            tickets,
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            shutdown,
            server,
            _directory: directory,
        }
    }

    fn ticket_url(&self) -> String {
        format!("{}{}", self.base, self.tickets.issue().unwrap().path)
    }

    /// Exchanges a fresh ticket and returns the session cookie pair.
    async fn sign_in(&self) -> String {
        let response = self.http.get(self.ticket_url()).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        cookie_pair(&response)
    }

    fn rpc(&self, cookie: Option<&str>) -> reqwest::RequestBuilder {
        let request = self
            .http
            .post(format!("{}/bridge/rpc", self.base))
            .json(&serde_json::json!({"method": "get_profiles", "params": {}}));
        match cookie {
            Some(cookie) => request.header("cookie", cookie),
            None => request,
        }
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.server.await.unwrap().unwrap();
    }
}

fn cookie_pair(response: &reqwest::Response) -> String {
    response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_ticket_signs_in_once_and_the_cookie_opens_the_api() {
    let served = Served::start().await;

    assert_eq!(
        served.rpc(None).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED,
        "no cookie"
    );
    let ticket_url = served.ticket_url();
    let exchanged = served.http.get(&ticket_url).send().await.unwrap();
    assert_eq!(exchanged.status(), StatusCode::SEE_OTHER);
    assert_eq!(exchanged.headers()["location"], "/");
    assert_eq!(exchanged.headers()["cache-control"], "no-store");
    assert_eq!(exchanged.headers()["referrer-policy"], "no-referrer");
    let set_cookie = exchanged.headers()["set-cookie"].to_str().unwrap();
    assert!(
        set_cookie.starts_with(&format!("{SESSION_COOKIE}=")),
        "{set_cookie}"
    );
    assert!(
        set_cookie.ends_with("; HttpOnly; SameSite=Strict; Path=/"),
        "{set_cookie}"
    );
    let cookie = cookie_pair(&exchanged);

    let profiles = served.rpc(Some(&cookie)).send().await.unwrap();
    assert_eq!(profiles.status(), StatusCode::OK);
    assert!(
        profiles
            .json::<Value>()
            .await
            .unwrap()
            .get("items")
            .is_some()
    );

    let reused = served.http.get(&ticket_url).send().await.unwrap();
    assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);
    assert!(reused.headers().get("set-cookie").is_none());
    // The earlier session is unaffected by the failed reuse.
    assert_eq!(
        served.rpc(Some(&cookie)).send().await.unwrap().status(),
        StatusCode::OK
    );
    served.stop().await;
}

#[tokio::test]
async fn an_unknown_ticket_or_cookie_is_refused() {
    let served = Served::start().await;

    let bad = served
        .http
        .get(format!(
            "{}/?ticket=00000000000000000000000000000000",
            served.base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);
    assert!(bad.headers().get("set-cookie").is_none());
    let guessed = format!("{SESSION_COOKIE}={}", uuid::Uuid::new_v4());
    assert_eq!(
        served.rpc(Some(&guessed)).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    // A ticket is only exchanged at the root.
    let elsewhere = served
        .http
        .post(format!("{}/bridge/rpc?ticket={}", served.base, "x"))
        .send()
        .await
        .unwrap();
    assert_eq!(elsewhere.status(), StatusCode::UNAUTHORIZED);
    served.stop().await;
}

#[tokio::test]
async fn a_foreign_origin_is_refused_but_any_host_name_is_served() {
    let served = Served::start().await;
    let cookie = served.sign_in().await;
    let authority = served.base.trim_start_matches("http://").to_owned();

    let foreign = served
        .rpc(Some(&cookie))
        .header("origin", "http://attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    let other_port = served
        .rpc(Some(&cookie))
        .header("origin", "http://127.0.0.1:1")
        .send()
        .await
        .unwrap();
    assert_eq!(other_port.status(), StatusCode::FORBIDDEN);
    // Even a valid ticket does not bypass the Origin check.
    let forged_exchange = served
        .http
        .get(served.ticket_url())
        .header("origin", "http://attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(forged_exchange.status(), StatusCode::FORBIDDEN);

    let own = served
        .rpc(Some(&cookie))
        .header("origin", format!("http://{authority}"))
        .send()
        .await
        .unwrap();
    assert_eq!(own.status(), StatusCode::OK);
    // A browser reaches a router by whatever name it has; there is no fixed authority.
    let named = served
        .rpc(Some(&cookie))
        .header("host", "router.lan:7899")
        .header("origin", "http://router.lan:7899")
        .send()
        .await
        .unwrap();
    assert_eq!(named.status(), StatusCode::OK);
    served.stop().await;
}

#[tokio::test]
async fn pages_are_never_framed_by_anyone() {
    let served = Served::start().await;
    let cookie = served.sign_in().await;

    let response = served
        .rpc(Some(&cookie))
        .header("host", "router.lan:7899")
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'none'"
    );
    // The refusal of an unauthenticated request and the ticket redirect carry
    // the policy too.
    let refused = served.rpc(None).send().await.unwrap();
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(refused.headers()["x-frame-options"], "DENY");
    let signed_in = served.http.get(served.ticket_url()).send().await.unwrap();
    assert_eq!(signed_in.status(), StatusCode::SEE_OTHER);
    assert_eq!(signed_in.headers()["x-frame-options"], "DENY");
    served.stop().await;
}

#[tokio::test]
async fn the_issued_port_is_the_bound_port() {
    let (listener, tickets) = RouterListener::bind(([127, 0, 0, 1], 0).into())
        .await
        .unwrap();

    assert_eq!(
        tickets.issue().unwrap().port,
        listener.local_addr().unwrap().port()
    );
}
