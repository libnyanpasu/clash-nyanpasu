//! Fatal policy acceptance through the real executable, including its composition and exit boundary.
#![cfg(feature = "panic-injection")]

use std::{path::PathBuf, process::Stdio, time::Duration};

use nyanpasu_rpc_protocol::{client::LocalClient, frame::Reply};
use serde_json::json;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _};

const WATCHDOG: Duration = Duration::from_secs(45);
const PANIC_PAYLOAD: &str = "e4b-fatal-original-payload-53fd73";

async fn panic_process(mode: &str) {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("child.log");
    let log = std::fs::File::create(&output).unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_nyanpasu-server"))
        .arg("--config-dir")
        .arg(directory.path().join("config"))
        .arg("--data-dir")
        .arg(directory.path().join("data"))
        .arg("--http")
        .env("NYANPASU_E4B_PANIC_TEST", mode)
        .stdout(Stdio::piped())
        .stderr(log)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    // Each child discovers an endpoint from its own temporary data directory.
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
    let result = tokio::time::timeout(WATCHDOG, async {
        let mut endpoint = None;
        let url = loop {
            let mut line = String::new();
            assert_ne!(
                stdout.read_line(&mut line).await.unwrap(),
                0,
                "child exited before readiness: {}",
                std::fs::read_to_string(&output).unwrap()
            );
            if let Some(path) = line.trim().strip_prefix("ready endpoint=") {
                endpoint = Some(PathBuf::from(path));
            }
            if let Some(url) = line.trim().strip_prefix("HTTP access: ") {
                break url.to_owned();
            }
        };
        let endpoint = endpoint.expect("local readiness precedes HTTP access URL");
        let observed = match mode {
            "http" => {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap();
                let response = client.get(&url).send().await.unwrap();
                assert_eq!(response.status(), 303);
                let cookies = response
                    .headers()
                    .get_all("set-cookie")
                    .iter()
                    .map(|value| value.to_str().unwrap().split(';').next().unwrap())
                    .collect::<Vec<_>>()
                    .join("; ");
                client
                    .post(format!("{}/bridge/rpc", url.split("/?").next().unwrap()))
                    .header("cookie", cookies)
                    .json(&json!({"method":"e4b_test_panic", "params":{}}))
                    .send()
                    .await
                    .is_err()
            }
            "local" | "owner" => {
                let local = LocalClient::connect(&endpoint, WATCHDOG).await.unwrap();
                let method = if mode == "owner" {
                    "e4b_test_owner_work"
                } else {
                    "e4b_test_panic"
                };
                let reply = local.call(method, json!({})).await;
                // Owner work may acknowledge the committed mutation before its effect panics.
                let observed =
                    reply.is_err() || (mode == "owner" && matches!(reply, Ok(Reply::Result(_))));
                local.close().await;
                observed
            }
            _ => unreachable!(),
        };
        let status = child.wait().await.unwrap();
        let mut remaining = String::new();
        stdout.read_to_string(&mut remaining).await.unwrap();
        (status, observed)
    })
    .await;
    let (status, observed) = match result {
        Ok(result) => result,
        Err(error) => {
            child.kill().await.unwrap();
            child.wait().await.unwrap();
            panic!(
                "subprocess watchdog failed: {error}; {}",
                std::fs::read_to_string(&output).unwrap()
            );
        }
    };
    let log = std::fs::read_to_string(output).unwrap();
    assert!(
        !status.success(),
        "fatal child unexpectedly succeeded: {log}"
    );
    assert!(
        log.contains(PANIC_PAYLOAD),
        "original payload missing: {log}"
    );
    assert!(
        log.contains("panic_injection.rs:"),
        "original panic location missing: {log}"
    );
    assert!(observed, "panic produced an RPC error response: {log}");
}

#[tokio::test]
async fn http_handler_panic_terminates_server_process() {
    panic_process("http").await;
}

#[tokio::test]
async fn local_handler_panic_terminates_server_process() {
    panic_process("local").await;
}

#[tokio::test]
async fn background_owner_panic_terminates_server_process() {
    panic_process("owner").await;
}
