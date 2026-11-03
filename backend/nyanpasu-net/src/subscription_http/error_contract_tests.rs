//! Wire and source-chain expectations for subscription failures.

use std::{error::Error, io};

use nyanpasu_config::profile::RemoteProfileOptions;
use nyanpasu_contracts::profiles::{error::SubscriptionFetchError, ports::SubscriptionFetcher};
use serde::Serialize;
use serde_json::{Value, json};
use url::Url;

use super::tests::service;

fn source_chain(error: &(dyn Error + 'static)) -> Vec<String> {
    let mut chain = Vec::new();
    let mut source = error.source();
    while let Some(error) = source {
        chain.push(error.to_string());
        source = error.source();
    }
    chain
}

fn assert_error(
    error: &(impl Error + Serialize + 'static),
    wire: Value,
    display: &str,
    sources: &[&str],
) {
    assert_eq!(serde_json::to_value(error).unwrap(), wire);
    assert_eq!(error.to_string(), display);
    assert_eq!(source_chain(error), sources);
}

fn library_source<'error, T: Error + 'static>(error: &'error (dyn Error + 'static)) -> &'error T {
    error
        .source()
        .expect("variant has a library cause")
        .downcast_ref::<T>()
        .expect("variant must preserve the original library error object")
}

fn direct_options() -> RemoteProfileOptions {
    RemoteProfileOptions {
        self_proxy: false,
        with_proxy: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn build_http_client_error_contract() {
    let service = service();
    let error = service
        .fetch(
            &Url::parse("http://127.0.0.1/").unwrap(),
            &RemoteProfileOptions {
                user_agent: Some("invalid\nheader".into()),
                ..direct_options()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SubscriptionFetchError::BuildHttpClient { .. }
    ));
    library_source::<reqwest::Error>(&error);
    assert_error(
        &error,
        json!({ "kind": "build_http_client" }),
        "could not build the http client",
        &["builder error", "failed to parse header value"],
    );
}

#[tokio::test]
async fn request_subscription_error_contract() {
    let service = service();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    drop(listener);
    let error = service.fetch(&url, &direct_options()).await.unwrap_err();
    assert!(matches!(
        error,
        SubscriptionFetchError::RequestSubscription { .. }
    ));
    let source = library_source::<reqwest::Error>(&error);
    assert!(source.is_connect());
    assert_eq!(source.url(), Some(&url));
    let io = source
        .source()
        .unwrap()
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .unwrap();
    assert_eq!(io.kind(), io::ErrorKind::ConnectionRefused);
    // The ephemeral port and OS-localized refusal message vary by environment.
    assert_error(
        &error,
        json!({ "kind": "request_subscription" }),
        "could not request the subscription",
        &[
            &format!("error sending request for url ({url})"),
            "client error (Connect)",
            "tcp connect error",
            &io.to_string(),
        ],
    );
    assert!(
        source_chain(&error)[3].ends_with(&format!("(os error {})", io.raw_os_error().unwrap()))
    );
}

#[tokio::test]
async fn read_subscription_body_error_contract() {
    use std::io::{Read as _, Write as _};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    // A dedicated thread serves a deliberately truncated body without blocking the async worker.
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nx")
            .unwrap();
    });
    let service = service();
    let error = service
        .fetch(
            &Url::parse(&format!("http://{address}/")).unwrap(),
            &direct_options(),
        )
        .await
        .unwrap_err();
    server.join().unwrap();
    // Only the server's ephemeral port varies; every source level is still compared.
    assert!(matches!(
        error,
        SubscriptionFetchError::ReadSubscriptionBody { .. }
    ));
    library_source::<reqwest::Error>(&error);
    assert_error(
        &error,
        json!({ "kind": "read_subscription_body" }),
        "could not read the subscription response body",
        &[
            &format!("error decoding response body for url (http://{address}/)"),
            "request or response body error",
            "error reading a body from connection",
            "end of file before message length reached",
        ],
    );
}
