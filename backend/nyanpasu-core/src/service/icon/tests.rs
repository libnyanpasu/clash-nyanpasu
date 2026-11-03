//! Cache hits retain the original icon bytes without another network operation.

use std::sync::Arc;

use bytes::Bytes;
use nyanpasu_contracts::net::MockIconFetcher;

use super::IconService;

#[tokio::test]
async fn fetches_and_caches_icons_with_the_callers_port() {
    let temp = tempfile::tempdir().unwrap();
    let paths = crate::utils::test_support::test_paths(temp.path(), temp.path().join("data"));
    let mut fetcher = MockIconFetcher::new();
    fetcher
        .expect_fetch()
        .times(1)
        .withf(|url, port| url.as_str() == "https://icons.example/icon.png" && *port == 7891)
        .return_once(|_, _| Ok(("image/png".into(), Bytes::from_static(b"cached icon"))));
    let service = IconService::new(paths, Arc::new(fetcher));

    for port in [7891, 7892] {
        assert_eq!(
            service
                .get_cached_icon("https://icons.example/icon.png", port)
                .await
                .unwrap(),
            ("image/png".into(), Bytes::from_static(b"cached icon"))
        );
    }
}
