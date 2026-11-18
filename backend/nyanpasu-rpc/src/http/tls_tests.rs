//! HTTPS listener behavior: the certificate it presents, secure cookies, the network scopes'
//! peer, credential and Host rules, and failing closed when no certificate is available.

use std::{net::IpAddr, sync::Arc};

use nyanpasu_config::application::{HttpAccessConfig, HttpListen, HttpTls};
use nyanpasu_contracts::auth::{
    CreateAccessTokenRequest, Grant, Role, TlsFingerprint, TlsIdentity, TokenName,
};
use nyanpasu_core::client::NyanpasuClient;
use reqwest::StatusCode;
use serde_json::json;

use super::{
    HttpEndpoint, HttpServerClient, HttpServerFault, HttpServerStatus, PeerPolicy, auth, routes,
    test_support::{StaticTlsIdentity, self_signed_identity},
    tls::HttpTlsIdentitySource,
};
use crate::test_support::Fixture;

struct Harness {
    fixture: Fixture,
    server: HttpServerClient,
    identity: Arc<StaticTlsIdentity>,
}

impl Harness {
    async fn start() -> Self {
        let fixture = Fixture::new(false).await;
        let routes = Arc::new(routes::RpcHttpRoutes::default());
        routes.install(&fixture.rpc).unwrap();
        let authenticator = Arc::new(auth::NyanpasuClientHttpAuthenticator::new(
            &fixture.rpc.dependencies.client,
        ));
        let identity = Arc::new(StaticTlsIdentity::new(self_signed_identity(&[])));
        let server = HttpServerClient::spawn_with_tls(
            None,
            routes,
            authenticator,
            Some(identity.clone() as Arc<dyn HttpTlsIdentitySource>),
        )
        .await
        .unwrap();
        Self {
            fixture,
            server,
            identity,
        }
    }

    fn client(&self) -> &NyanpasuClient {
        &self.fixture.rpc.dependencies.client
    }

    async fn configure(&self, config: HttpAccessConfig) -> HttpServerStatus {
        self.server.configure(&config).await.unwrap()
    }

    async fn token(&self) -> String {
        self.client()
            .create_remote_access_token(CreateAccessTokenRequest {
                name: TokenName::parse("lan-test").unwrap(),
                grant: Grant::Role(Role::Viewer),
                expires_at_ms: None,
            })
            .await
            .unwrap()
            .token
            .expose_secret()
            .to_owned()
    }

    async fn finish(self) {
        self.server.shutdown().await.unwrap();
        self.fixture.shutdown().await;
    }
}

fn tls_config() -> HttpAccessConfig {
    HttpAccessConfig {
        enabled: true,
        tls: HttpTls::SelfSigned,
        ..Default::default()
    }
}

/// A LAN listener whose allow list leaves this computer's loopback peer to the loopback rule.
fn lan_config() -> HttpAccessConfig {
    HttpAccessConfig {
        listen: HttpListen::Lan,
        lan_allow: vec!["10.0.0.0/8".parse().unwrap()],
        ..tls_config()
    }
}

/// A client that accepts the self-signed certificate and reports what the server presented.
fn insecure_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .tls_danger_accept_invalid_certs(true)
        .tls_info(true)
        .build()
        .unwrap()
}

/// HTTP/1.1 sends the name in `Host`, so a test can address any name without resolving it.
fn http1_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .http1_only()
        .tls_danger_accept_invalid_certs(true)
        .build()
        .unwrap()
}

fn presented(response: &reqwest::Response) -> TlsFingerprint {
    let info = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .expect("an HTTPS response reports its TLS info");
    TlsFingerprint::of_certificate(
        info.peer_certificate()
            .expect("the server sent a certificate"),
    )
}

fn url_of(status: &HttpServerStatus) -> String {
    status
        .url
        .as_deref()
        .unwrap()
        .trim_end_matches('/')
        .to_owned()
}

#[tokio::test]
async fn https_presents_the_certificate_and_marks_the_session_cookie_secure() {
    let harness = Harness::start().await;
    let status = harness.configure(tls_config()).await;

    let url = url_of(&status);
    assert!(url.starts_with("https://127.0.0.1:"), "{url}");
    let identity: TlsIdentity = harness.identity.identity().await.unwrap();
    assert_eq!(status.tls, Some(identity.summary()));

    let entry = harness.client().launch_url_for(&url).await.unwrap();
    let response = insecure_client().get(&entry).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(presented(&response), identity.fingerprint());
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.ends_with("; Secure"), "{cookie}");
    assert!(cookie.contains("HttpOnly; SameSite=Strict"), "{cookie}");

    // The same port does not also answer in the clear.
    let plain = url.replacen("https://", "http://", 1);
    let plain = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(&plain)
        .send()
        .await;
    assert!(plain.is_err() || plain.unwrap().status() != StatusCode::OK);

    harness.finish().await;
}

#[tokio::test]
async fn a_loopback_https_listener_answers_to_localhost() {
    let harness = Harness::start().await;
    let status = harness.configure(tls_config()).await;
    let port = status.port.unwrap();
    let http = reqwest::Client::builder()
        .no_proxy()
        .tls_danger_accept_invalid_certs(true)
        .resolve("localhost", ([127, 0, 0, 1], port).into())
        .build()
        .unwrap();

    let response = http
        .get(format!("https://localhost:{port}/login"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    harness.finish().await;
}

#[tokio::test]
async fn a_plain_loopback_listener_keeps_its_cookie_without_secure() {
    let harness = Harness::start().await;
    let status = harness
        .configure(HttpAccessConfig {
            enabled: true,
            ..Default::default()
        })
        .await;
    assert_eq!(status.tls, None);
    assert!(url_of(&status).starts_with("http://127.0.0.1:"));

    let entry = harness
        .client()
        .launch_url_for(&url_of(&status))
        .await
        .unwrap();
    let response = insecure_client().get(&entry).send().await.unwrap();
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(!cookie.contains("Secure"), "{cookie}");

    harness.finish().await;
}

#[tokio::test]
async fn a_certificate_that_is_unavailable_never_falls_back_to_plain_http() {
    let fixture = Fixture::new(false).await;
    let routes = Arc::new(routes::RpcHttpRoutes::default());
    routes.install(&fixture.rpc).unwrap();
    let authenticator = Arc::new(auth::NyanpasuClientHttpAuthenticator::new(
        &fixture.rpc.dependencies.client,
    ));
    // A host without TLS support.
    let server = HttpServerClient::spawn(None, routes, authenticator)
        .await
        .unwrap();

    for config in [tls_config(), lan_config()] {
        let status = server.configure(&config).await.unwrap();
        assert!(!status.enabled);
        assert_eq!(status.url, None);
        assert_eq!(status.fault, Some(HttpServerFault::TlsUnavailable));
    }

    // A network listener refuses plain HTTP even if an unvalidated value reaches the listener.
    let harness = Harness::start().await;
    let status = harness
        .configure(HttpAccessConfig {
            tls: HttpTls::Http,
            ..lan_config()
        })
        .await;
    assert!(!status.enabled);
    assert_eq!(status.fault, Some(HttpServerFault::TlsUnavailable));
    harness.finish().await;

    server.shutdown().await.unwrap();
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_certificate_that_cannot_be_served_is_unavailable_not_a_bind_failure() {
    let harness = Harness::start().await;
    harness.identity.replace(TlsIdentity::new(
        b"not a certificate".to_vec(),
        b"not a key".to_vec(),
        Vec::new(),
        Vec::new(),
        i64::MAX,
    ));

    for config in [tls_config(), lan_config()] {
        let status = harness.configure(config).await;
        assert!(!status.enabled);
        assert_eq!(status.url, None);
        assert_eq!(status.fault, Some(HttpServerFault::TlsUnavailable));
    }

    // Replacing the certificate, as the settings offer, brings the listener back.
    harness.identity.replace(self_signed_identity(&[]));
    let status = harness.configure(lan_config()).await;
    assert!(status.enabled);
    assert_eq!(status.fault, None);

    harness.finish().await;
}

#[tokio::test]
async fn a_certificate_that_becomes_unavailable_stops_the_listener() {
    let harness = Harness::start().await;
    let running = harness.configure(lan_config()).await;
    assert!(running.enabled);

    harness.identity.withdraw();
    let status = harness.configure(lan_config()).await;

    assert!(!status.enabled);
    assert_eq!(status.fault, Some(HttpServerFault::TlsUnavailable));
    let refused = insecure_client()
        .get(format!("{}/login", url_of(&running)))
        .send()
        .await;
    assert!(
        refused.is_err(),
        "nothing may keep serving without a certificate"
    );

    harness.finish().await;
}

#[tokio::test]
async fn replacing_the_certificate_restarts_the_listener_on_the_same_port() {
    let harness = Harness::start().await;
    let probe = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let config = HttpAccessConfig {
        port: Some(port),
        ..tls_config()
    };
    let first = harness.configure(config.clone()).await;
    let unchanged = harness.configure(config.clone()).await;
    assert_eq!(
        first.tls, unchanged.tls,
        "the same certificate keeps the listener"
    );

    let replacement = self_signed_identity(&[]);
    harness.identity.replace(replacement.clone());
    let second = harness.configure(config).await;

    assert_eq!(second.port, Some(port));
    assert_eq!(second.tls, Some(replacement.summary()));
    assert_ne!(first.tls, second.tls);
    let response = insecure_client()
        .get(format!("{}/login", url_of(&second)))
        .send()
        .await
        .unwrap();
    assert_eq!(presented(&response), replacement.fingerprint());

    harness.finish().await;
}

#[tokio::test]
async fn a_network_listener_needs_a_credential_also_from_loopback() {
    let harness = Harness::start().await;
    // Every IPv4 interface, reachable here through loopback whatever the allow list says.
    let status = harness.configure(lan_config()).await;
    let url = url_of(&status);
    assert!(url.starts_with("https://127.0.0.1:"), "{url}");
    let http = insecure_client();
    let request = json!({ "method": "rpc_describe", "params": {} });

    let anonymous = http
        .post(format!("{url}/bridge/rpc"))
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let token = harness.token().await;
    let authorized = http
        .post(format!("{url}/bridge/rpc"))
        .bearer_auth(&token)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);

    harness.finish().await;
}

#[tokio::test]
async fn a_network_listener_answers_only_to_addresses_and_known_names() {
    let harness = Harness::start().await;
    let status = harness.configure(lan_config()).await;
    let url = url_of(&status);
    let port = status.port.unwrap();
    let token = harness.token().await;
    let http = http1_client();
    let request = json!({ "method": "rpc_describe", "params": {} });
    let call = |host: String, origin: Option<String>| {
        let mut call = http
            .post(format!("{url}/bridge/rpc"))
            .bearer_auth(&token)
            .header("host", host)
            .json(&request);
        if let Some(origin) = origin {
            call = call.header("origin", origin);
        }
        async move { call.send().await.unwrap().status() }
    };

    for allowed in [
        format!("127.0.0.1:{port}"),
        format!("[::1]:{port}"),
        format!("192.168.1.20:{port}"),
        format!("localhost:{port}"),
        format!("LOCALHOST:{port}"),
    ] {
        assert_eq!(
            call(allowed.clone(), None).await,
            StatusCode::OK,
            "{allowed}"
        );
        assert_eq!(
            call(allowed.clone(), Some(format!("https://{allowed}"))).await,
            StatusCode::OK,
            "{allowed}"
        );
    }
    for refused in [
        format!("evil.example:{port}"),
        format!("rebind.localhost.evil.example:{port}"),
        format!("127.0.0.1:{}", port.wrapping_add(1)),
        "127.0.0.1".to_owned(),
        format!("[::1:{port}"),
    ] {
        assert_eq!(
            call(refused.clone(), None).await,
            StatusCode::FORBIDDEN,
            "{refused}"
        );
    }
    let host = format!("127.0.0.1:{port}");
    for origin in ["https://evil.example", &format!("http://{host}"), "null"] {
        assert_eq!(
            call(host.clone(), Some(origin.to_owned())).await,
            StatusCode::FORBIDDEN,
            "{origin}"
        );
    }

    harness.finish().await;
}

#[tokio::test]
async fn mixed_case_https_host_and_origin_are_the_same_local_origin() {
    let harness = Harness::start().await;
    let status = harness.configure(tls_config()).await;
    let port = status.port.unwrap();
    let token = harness.token().await;
    let http = http1_client();
    let request = json!({ "method": "rpc_describe", "params": {} });

    for (host, origin) in [
        (
            format!("LOCALHOST:{port}"),
            format!("https://localhost:{port}"),
        ),
        (
            format!("localhost:{port}"),
            format!("https://LOCALHOST:{port}"),
        ),
    ] {
        let response = http
            .post(format!("{}/bridge/rpc", url_of(&status)))
            .bearer_auth(&token)
            .header("host", &host)
            .header("origin", &origin)
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{host} / {origin}");
    }

    harness.finish().await;
}

#[test]
fn an_origin_must_match_the_request_scheme_and_authority_exactly() {
    assert!(super::origin_matches(
        "https://LOCALHOST:8443",
        "https",
        "localhost:8443",
        443,
    ));
    assert!(super::origin_matches(
        "https://localhost:8443",
        "https",
        "LOCALHOST:8443",
        443,
    ));
    assert!(super::origin_matches(
        "https://localhost",
        "https",
        "localhost:443",
        443,
    ));
    assert!(super::origin_matches(
        "https://localhost:443",
        "https",
        "localhost",
        443,
    ));
    for refused in [
        "http://localhost:8443",
        "https://localhost:8443/path",
        "https://user@localhost:8443",
        "https://localhost:8443?key=value",
        "https://localhost:8443#fragment",
        "https://localhost:8444",
        "null",
    ] {
        assert!(
            !super::origin_matches(refused, "https", "localhost:8443", 443),
            "{refused}"
        );
    }
}

#[tokio::test]
async fn leaving_the_lan_scope_stops_answering_to_lan_names_on_the_same_port() {
    let harness = Harness::start().await;
    let lan = harness.configure(lan_config()).await;
    let port = lan.port.unwrap();
    let token = harness.token().await;
    let http = http1_client();
    let rpc_describe = |host: String, url: String| {
        let request = http
            .post(format!("{url}/bridge/rpc"))
            .bearer_auth(&token)
            .header("host", host)
            .json(&json!({ "method": "rpc_describe", "params": {} }));
        async move { request.send().await.unwrap().status() }
    };
    let lan_host = format!("192.168.1.20:{port}");
    assert_eq!(
        rpc_describe(lan_host.clone(), url_of(&lan)).await,
        StatusCode::OK
    );

    // The loopback scope takes the port over, so only its own authority is answered.
    let loopback = harness
        .configure(HttpAccessConfig {
            port: Some(port),
            ..tls_config()
        })
        .await;
    assert!(loopback.enabled, "{loopback:?}");
    assert_eq!(loopback.port, Some(port));
    assert_eq!(
        rpc_describe(lan_host, url_of(&loopback)).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        rpc_describe(format!("127.0.0.1:{port}"), url_of(&loopback)).await,
        StatusCode::OK
    );

    harness.finish().await;
}

#[tokio::test]
async fn every_network_scope_listens_on_every_interface_over_https() {
    let harness = Harness::start().await;
    for config in [
        lan_config(),
        HttpAccessConfig {
            listen: HttpListen::Any,
            ..tls_config()
        },
    ] {
        let status = harness.configure(config).await;
        assert!(status.enabled, "{status:?}");
        // This computer reaches it through loopback.
        let url = url_of(&status);
        assert!(url.starts_with("https://127.0.0.1:"), "{url}");
        let response = insecure_client()
            .get(format!("{url}/login"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    harness.finish().await;
}

#[test]
fn a_lan_listener_admits_loopback_and_its_networks_only() {
    let lan = PeerPolicy::of(&HttpAccessConfig {
        listen: HttpListen::Lan,
        lan_allow: vec![
            "192.168.1.0/24".parse().unwrap(),
            "100.64.0.0/10".parse().unwrap(),
        ],
        ..tls_config()
    });
    for admitted in [
        "192.168.1.7",
        "100.101.102.103",
        "127.0.0.1",
        "::1",
        "::ffff:192.168.1.7",
    ] {
        let peer: IpAddr = admitted.parse().unwrap();
        assert!(lan.admits(peer), "{admitted}");
    }
    for refused in [
        "192.168.2.7",
        "10.0.0.1",
        "203.0.113.9",
        "fd00::1",
        "::ffff:10.0.0.1",
    ] {
        let peer: IpAddr = refused.parse().unwrap();
        assert!(!lan.admits(peer), "{refused}");
    }
    let any = PeerPolicy::of(&HttpAccessConfig {
        listen: HttpListen::Any,
        ..lan_config()
    });
    assert!(any.admits("203.0.113.9".parse().unwrap()));
}

#[tokio::test]
async fn http2_carries_the_authority_instead_of_a_host_header() {
    let harness = Harness::start().await;
    let status = harness.configure(lan_config()).await;
    let port = status.port.unwrap();
    let token = harness.token().await;
    // The browser's path: TLS negotiates HTTP/2, which has `:authority` and no `Host`.
    let http = reqwest::Client::builder()
        .no_proxy()
        .tls_danger_accept_invalid_certs(true)
        .resolve("localhost", ([127, 0, 0, 1], port).into())
        .build()
        .unwrap();
    let response = http
        .post(format!("https://localhost:{port}/bridge/rpc"))
        .bearer_auth(&token)
        .json(&json!({ "method": "rpc_describe", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.version(), reqwest::Version::HTTP_2);
    assert_eq!(response.status(), StatusCode::OK);

    let http = reqwest::Client::builder()
        .no_proxy()
        .tls_danger_accept_invalid_certs(true)
        .resolve("evil.example", ([127, 0, 0, 1], port).into())
        .build()
        .unwrap();
    let foreign = http
        .post(format!("https://evil.example:{port}/bridge/rpc"))
        .bearer_auth(&token)
        .json(&json!({ "method": "rpc_describe", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    drop(foreign);

    harness.finish().await;
}

#[tokio::test]
async fn a_stalled_handshake_does_not_hold_up_shutdown() {
    let harness = Harness::start().await;
    let status = harness.configure(tls_config()).await;
    // Connects and says nothing, as a scanner or a stuck client would.
    let _stalled = tokio::net::TcpStream::connect(("127.0.0.1", status.port.unwrap()))
        .await
        .unwrap();

    // The listener's own two-second abort would also end this, so the bound is tighter.
    tokio::time::timeout(std::time::Duration::from_secs(1), harness.server.shutdown())
        .await
        .expect("a handshake in progress must not delay shutdown")
        .unwrap();

    harness.fixture.shutdown().await;
}

#[test]
fn a_host_without_a_port_names_the_default_port_of_the_scheme() {
    // A browser leaves `:443` out of `Host` and `Origin` for an https URL.
    let lan = HttpEndpoint::lan(443, vec!["nyan.local".to_owned()]);
    for allowed in [
        "192.168.1.20",
        "192.168.1.20:443",
        "[::1]",
        "[fd00::5]:443",
        "nyan.local",
        "NYAN.local:443",
    ] {
        assert!(lan.allows(allowed), "{allowed}");
    }
    for refused in [
        "evil.example",
        "192.168.1.20:8443",
        "[::1",
        "[::1]x",
        // An IPv6 address is only valid in brackets.
        "::1",
        "fd00::5:443",
    ] {
        assert!(!lan.allows(refused), "{refused}");
    }

    // On another port the bare host is another port.
    let lan = HttpEndpoint::lan(8443, Vec::new());
    assert!(!lan.allows("192.168.1.20"));
    assert!(lan.allows("192.168.1.20:8443"));

    let https = HttpEndpoint::exact("127.0.0.1:443".into(), true);
    assert!(https.allows("127.0.0.1"));
    assert!(https.allows("127.0.0.1:443"));
    assert!(!https.allows("127.0.0.1:80"));
    let http = HttpEndpoint::exact("127.0.0.1:80".into(), false);
    assert!(http.allows("127.0.0.1"));
    assert!(!HttpEndpoint::exact("127.0.0.1:4000".into(), true).allows("127.0.0.1"));
    assert!(!HttpEndpoint::exact("127.0.0.1:1443".into(), true).allows("127.0.0.1"));
}

#[tokio::test]
async fn the_login_page_over_https_explains_how_to_compare_the_fingerprint() {
    let harness = Harness::start().await;
    let status = harness.configure(lan_config()).await;

    let response = insecure_client()
        .get(format!("{}/login", url_of(&status)))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = response.text().await.unwrap();

    // The note names the fingerprint check and is revealed only on an HTTPS origin, where there
    // is a certificate to compare; the page cannot show the fingerprint itself, because a server
    // an attacker stands in for would show its own.
    assert!(page.contains("id=\"fingerprint-note\""), "{page}");
    assert!(page.contains("SHA-256"), "{page}");
    assert!(
        page.contains("location.protocol === 'https:'"),
        "the note must not appear on plain HTTP"
    );

    harness.finish().await;
}

#[test]
fn a_loopback_endpoint_matches_only_its_authority() {
    let endpoint = HttpEndpoint::exact("127.0.0.1:4000".into(), true);
    assert!(endpoint.allows("127.0.0.1:4000"));
    assert!(!endpoint.allows("localhost:4000"));
    assert!(!endpoint.allows("127.0.0.1:4001"));
}

#[test]
fn a_secure_loopback_endpoint_also_matches_localhost() {
    let endpoint = HttpEndpoint::secure_loopback("127.0.0.1:4000".into());
    assert!(endpoint.allows("127.0.0.1:4000"));
    assert!(endpoint.allows("localhost:4000"));
    assert!(endpoint.allows("LOCALHOST:4000"));
    assert!(!endpoint.allows("localhost:4001"));
    assert!(!endpoint.allows("localhost.example:4000"));

    let endpoint = HttpEndpoint::secure_loopback("127.0.0.1:443".into());
    assert!(endpoint.allows("localhost"));
    assert!(endpoint.allows("localhost:443"));
}
