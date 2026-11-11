use super::{IconRequest, allowed_content_type, parse};

#[test]
fn icon_url_parser_matches_frontend_fixtures() {
    let request = parse("/cached", Some("url=https%3A%2F%2Ficons.example%2F%E7%8C%AB+icon.svg%3Fa%3D1%26token%3Da%2Bb%26next%3D%252F")).unwrap();
    let IconRequest::Cached { url } = request else {
        panic!("cached request")
    };
    assert_eq!(
        url,
        "https://icons.example/猫 icon.svg?a=1&token=a+b&next=%2F"
    );
    for (path, query, expected) in [
        (
            "/tray",
            "mode=normal&version=0",
            serde_json::json!({"kind":"tray","mode":"normal","version":0}),
        ),
        (
            "/widget",
            "mode=system_proxy&version=4294967295",
            serde_json::json!({"kind":"widget","mode":"system_proxy","version":4294967295u32}),
        ),
        (
            "/tray",
            "mode=tun",
            serde_json::json!({"kind":"tray","mode":"tun","version":null}),
        ),
    ] {
        assert_eq!(
            serde_json::to_value(parse(path, Some(query)).unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn icon_parser_rejects_ambiguous_or_invalid_requests() {
    for (path, query) in [
        ("/other", "mode=normal"),
        ("/tray/normal", ""),
        ("/tray", ""),
        ("/tray", "mode="),
        ("/tray", "mode=unknown"),
        ("/tray", "mode=tun&mode=normal"),
        ("/tray", "mode=tun&%6Dode=normal"),
        ("/tray", "mode=tun&url=https://secret.example"),
        ("/tray", "mode=tun&version=-1"),
        ("/tray", "mode=tun&version=%2B1"),
        ("/tray", "mode=tun&version=4294967296"),
        ("/tray", "mode=tun&version="),
        ("/cached", "url=file%3A%2F%2F%2Fsecret"),
        ("/cached", "url=%2Frelative"),
        ("/cached", "url=https%3A%2F%2F"),
        ("/cached", "url=https://secret.example&x=1"),
        (
            "/cached",
            "url=https://secret.example&url=https://other.example",
        ),
        ("/cached", "url=%"),
        ("/cached", "url=%zz"),
    ] {
        let error = parse(path, Some(query)).unwrap_err();
        assert_eq!(error.kind, "invalid_params", "{path} {query}");
        assert!(!error.message.contains("secret"));
    }
}

#[test]
fn icon_entry_rejects_non_image_content_types() {
    for mime in [
        "",
        "text/html",
        "application/xhtml+xml",
        "image/*",
        "image/unknown",
        "text/plain",
    ] {
        assert_eq!(allowed_content_type(mime).unwrap_err().kind, "invalid_icon");
    }
    for mime in [
        "image/png",
        "image/apng",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/avif",
        "image/bmp",
        "image/x-icon",
        "image/vnd.microsoft.icon",
        "image/svg+xml",
    ] {
        assert_eq!(allowed_content_type(mime).unwrap(), mime);
    }
    assert_eq!(
        allowed_content_type("IMAGE/SVG+XML; charset=utf-8").unwrap(),
        "image/svg+xml"
    );
}

#[cfg(feature = "http")]
pub(crate) async fn fixture(mime: &'static str, fetches: usize) -> crate::test_support::Fixture {
    use nyanpasu_contracts::net::MockIconFetcher;
    use std::sync::Arc;

    let directory = tempfile::tempdir().unwrap();
    let mut args = crate::test_support::test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    )
    .await;
    let mut fetcher = MockIconFetcher::new();
    fetcher
        .expect_fetch()
        .times(fetches)
        .returning(move |_, _| Ok((mime.to_owned(), b"untrusted icon bytes".to_vec().into())));
    args.host_adapters.icon_fetcher = Arc::new(fetcher);
    crate::test_support::Fixture::with_setup(false, &[], directory, args).await
}

#[cfg(feature = "http")]
#[tokio::test]
async fn cached_icon_content_is_checked_before_serving() {
    let fixture = fixture("text/html", 1).await;
    for _ in 0..2 {
        let request = IconRequest::Cached {
            url: "https://icons.example/untrusted".into(),
        };
        let error = super::read(&fixture.rpc.dependencies, request)
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind, "invalid_icon");
        assert!(!error.message.contains("untrusted icon bytes"));
    }
    fixture.shutdown().await;
}

#[cfg(feature = "http")]
#[tokio::test]
async fn headless_host_icons_report_unavailable() {
    let fixture = fixture("image/png", 0).await;
    for path in ["/tray", "/widget"] {
        let request = parse(path, Some("mode=normal")).unwrap();
        assert_eq!(
            super::read(&fixture.rpc.dependencies, request)
                .await
                .err()
                .unwrap()
                .kind,
            "unavailable"
        );
    }
    fixture.shutdown().await;
}
