//! Facade network calls keep the current confirmed-or-configured proxy port per request.

use std::{io, sync::Arc};

use bytes::Bytes;
use nyanpasu_config::clash::config::ClashConfig;
use nyanpasu_contracts::net::{MockIconFetcher, MockNetworkDiagnostics};
use serde_json::json;
use struct_patch::Patch as _;
use tempfile::TempDir;

use super::{NyanpasuClient, test_support};

async fn client(
    dir: &TempDir,
    icons: MockIconFetcher,
    diagnostics: MockNetworkDiagnostics,
) -> NyanpasuClient {
    let mut adapters = test_support::profile_test_adapters(dir);
    adapters.icon_fetcher = Arc::new(icons);
    adapters.network_diagnostics = Arc::new(diagnostics);
    let client = test_support::facade::test_client_with_profile_ports(dir, adapters).await;
    set_port(&client, 7891).await;
    client
}

async fn set_port(client: &NyanpasuClient, port: u16) {
    let mut patch = ClashConfig::new_empty_patch();
    patch.mixed_port.start_port = Some(port);
    client.patch_clash_config(patch).await.unwrap();
    assert!(
        client.session_ports().is_none(),
        "this fixture exercises the configured fallback"
    );
    assert_eq!(client.clash_info().port, port);
}

#[tokio::test]
async fn icon_fetch_receives_current_port_and_preserves_results_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut icons = MockIconFetcher::new();
    icons
        .expect_fetch()
        .times(1)
        .withf(|url, port| url.as_str() == "https://icons.example/icon.png" && *port == 7891)
        .return_once(|_, _| Ok(("image/png".into(), Bytes::from_static(b"icon"))));
    icons
        .expect_fetch()
        .times(1)
        .withf(|url, port| url.as_str() == "https://icons.example/rejected.png" && *port == 7891)
        .return_once(|_, _| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "icon rejected").into())
        });
    let client = client(&dir, icons, MockNetworkDiagnostics::new()).await;

    assert_eq!(
        client
            .get_cached_icon("https://icons.example/icon.png")
            .await
            .unwrap(),
        ("image/png".into(), Bytes::from_static(b"icon"))
    );
    let error = client
        .get_cached_icon("https://icons.example/rejected.png")
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "icon rejected");
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[tokio::test]
async fn delay_probe_receives_url_status_and_current_port_and_preserves_some_and_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = MockNetworkDiagnostics::new();
    diagnostics
        .expect_url_delay_test()
        .times(1)
        .withf(|url, status, port| {
            url == "https://probe.example/" && *status == 204 && *port == 7891
        })
        .return_once(|_, _, _| Some(42));
    diagnostics
        .expect_url_delay_test()
        .times(1)
        .withf(|url, status, port| {
            url == "https://probe.example/" && *status == 204 && *port == 7891
        })
        .return_once(|_, _, _| None);
    let client = client(&dir, MockIconFetcher::new(), diagnostics).await;

    assert_eq!(
        client.url_delay_test("https://probe.example/", 204).await,
        Some(42)
    );
    assert_eq!(
        client.url_delay_test("https://probe.example/", 204).await,
        None
    );
}

#[tokio::test]
async fn asn_query_receives_current_port_and_preserves_values_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = MockNetworkDiagnostics::new();
    diagnostics
        .expect_get_ipsb_asn()
        .times(1)
        .withf(|port| *port == 7891)
        .return_once(|_| Ok(json!({ "asn": 64512 })));
    diagnostics
        .expect_get_ipsb_asn()
        .times(1)
        .withf(|port| *port == 7891)
        .return_once(|_| {
            Err(io::Error::new(io::ErrorKind::ConnectionRefused, "ASN unavailable").into())
        });
    let client = client(&dir, MockIconFetcher::new(), diagnostics).await;

    assert_eq!(
        client.get_ipsb_asn().await.unwrap(),
        json!({ "asn": 64512 })
    );
    let error = client.get_ipsb_asn().await.unwrap_err();
    assert_eq!(error.to_string(), "ASN unavailable");
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::ConnectionRefused
    );
}

#[tokio::test]
async fn diagnostics_read_the_port_again_after_configuration_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = MockNetworkDiagnostics::new();
    diagnostics
        .expect_url_delay_test()
        .times(1)
        .withf(|url, status, port| {
            url == "https://probe.example/" && *status == 200 && *port == 7891
        })
        .return_once(|_, _, _| Some(1));
    diagnostics
        .expect_url_delay_test()
        .times(1)
        .withf(|url, status, port| {
            url == "https://probe.example/" && *status == 200 && *port == 7892
        })
        .return_once(|_, _, _| Some(2));
    let client = client(&dir, MockIconFetcher::new(), diagnostics).await;

    assert_eq!(
        client.url_delay_test("https://probe.example/", 200).await,
        Some(1)
    );
    set_port(&client, 7892).await;
    assert_eq!(
        client.url_delay_test("https://probe.example/", 200).await,
        Some(2)
    );
}
