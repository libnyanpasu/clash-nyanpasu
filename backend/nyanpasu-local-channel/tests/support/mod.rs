//! Shared by the end-to-end and stress tests: the message types, and running
//! the test binary itself as the child process.
//!
//! A test spawns its own executable with only the `peer_entry` test selected
//! and the scenario in an environment variable; `peer_entry` runs the scenario
//! as the child and returns at once in an ordinary test run. A child that
//! panics exits unsuccessfully, which the parent asserts against.

// Each test binary compiles this module and uses only part of it.
#![allow(dead_code)]

use std::{ffi::OsStr, process::Stdio, time::Duration};

use nyanpasu_local_channel::{Bootstrap, tokio::OneShotServer};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use tokio::process::{Child, Command};

const SCENARIO_ENV: &str = "NYANPASU_LOCAL_CHANNEL_TEST_SCENARIO";
const ARG_ENV: &str = "NYANPASU_LOCAL_CHANNEL_TEST_ARG";
const ENTRY: &str = "peer_entry";

/// Bounds every wait on the other process, so a hung peer fails the test
/// instead of hanging the run.
pub const DEADLINE: Duration = Duration::from_secs(120);

/// Messages from the server to the child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Down {
    Text(String),
    Seq(u64),
    Lane { lane: u32, seq: u64 },
    Blob(Vec<u8>),
    End,
}

/// Messages from the child to the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Up {
    Text(String),
    Seq { lane: u32, seq: u64 },
    Blob(Vec<u8>),
    Ready,
    Summary { count: u64 },
}

/// What an echoing child answers to `message`.
pub fn echo(message: Down) -> Up {
    match message {
        Down::Text(text) => Up::Text(text),
        Down::Seq(seq) => Up::Seq { lane: 0, seq },
        Down::Lane { lane, seq } => Up::Seq { lane, seq },
        Down::Blob(blob) => Up::Blob(blob),
        Down::End => Up::Ready,
    }
}

/// Encodes like a `u8` but refuses to decode 13, so a well-formed frame can
/// still fail to decode on the receiving side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Picky(pub u8);

impl<'de> Deserialize<'de> for Picky {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            13 => Err(D::Error::custom("thirteen is refused")),
            value => Ok(Picky(value)),
        }
    }
}

/// Starts this test binary as the child running `scenario`, with `bootstrap`
/// in the variable the child reads it from.
pub fn spawn_peer(scenario: &str, bootstrap: impl AsRef<OsStr>, arg: Option<&str>) -> Child {
    let mut command = Command::new(std::env::current_exe().expect("the test binary has a path"));
    command
        .args([
            ENTRY,
            "--exact",
            "--nocapture",
            "--test-threads=1",
            "--quiet",
        ])
        .env(SCENARIO_ENV, scenario)
        .env(Bootstrap::ENV, bootstrap)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // A failing test must not leave its child running.
        .kill_on_drop(true);
    if let Some(arg) = arg {
        command.env(ARG_ENV, arg);
    }
    command
        .spawn()
        .expect("the test binary starts again as the peer")
}

pub async fn expect_success(child: &mut Child) {
    let status = tokio::time::timeout(DEADLINE, child.wait())
        .await
        .expect("the peer exits in time")
        .expect("the peer status is readable");
    assert!(status.success(), "the peer failed: {status}");
}

pub async fn accept<S, R>(
    server: OneShotServer<S, R>,
) -> (
    nyanpasu_local_channel::tokio::Sender<S>,
    nyanpasu_local_channel::tokio::Receiver<R>,
)
where
    S: Serialize,
    R: serde::de::DeserializeOwned,
{
    tokio::time::timeout(DEADLINE, server.accept())
        .await
        .expect("the child connects in time")
        .expect("the child is accepted")
}

/// The scenario this process runs as a child, if it is one.
pub fn scenario() -> Option<String> {
    std::env::var(SCENARIO_ENV).ok()
}

/// The numeric argument the parent passed to the child.
pub fn arg() -> u64 {
    std::env::var(ARG_ENV)
        .expect("the scenario takes an argument")
        .parse()
        .expect("the argument is a number")
}

pub fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime starts")
}

/// Changes one digit of the token, so the bootstrap names the right socket
/// with the wrong token.
pub fn tamper_token(bootstrap: &str) -> String {
    let mut bytes = bootstrap.as_bytes().to_vec();
    // "v1:" is followed by the token's first hex digit.
    bytes[3] = if bytes[3] == b'0' { b'1' } else { b'0' };
    String::from_utf8(bytes).expect("hex digits are ASCII")
}

/// Deterministic bytes that differ by `seed`.
pub fn pattern(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

pub fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Opens a raw connection to the socket a bootstrap names, bypassing the
/// handshake, to act as something other than the child.
pub fn raw_connect(bootstrap: &Bootstrap) -> interprocess::local_socket::Stream {
    use interprocess::local_socket::{Stream, prelude::*};

    #[cfg(windows)]
    let name = std::ffi::OsString::from(bootstrap.name())
        .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
        .unwrap();
    #[cfg(unix)]
    let name = std::path::PathBuf::from(bootstrap.name())
        .to_fs_name::<interprocess::local_socket::GenericFilePath>()
        .unwrap();
    Stream::connect(name).expect("the socket accepts a raw connection")
}
