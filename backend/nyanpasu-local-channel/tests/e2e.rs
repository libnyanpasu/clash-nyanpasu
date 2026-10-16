//! End-to-end behavior between a parent and a real child process: every public
//! entry point, every error a caller can observe, and the security boundary.

mod support;

use std::time::Duration;

use nyanpasu_local_channel::{
    AcceptError, Bootstrap, HandshakeError, Never, Options, RecvError, SendError,
    tokio::{self as channel, OneShotServer},
};
use support::{Down, Picky, Up, accept, echo, expect_success, pattern, spawn_peer};

#[test]
fn peer_entry() {
    if let Some(scenario) = support::scenario() {
        peer::run(&scenario);
    }
}

/// What the child does in each scenario. A panic fails the parent's test
/// through the child's exit status.
mod peer {
    use std::io::{Read as _, Write as _};

    use nyanpasu_local_channel::{
        Bootstrap, BootstrapError, ConnectError, HandshakeError, Never, Options, RecvError,
        RejectReason, SendError, blocking,
    };

    use crate::support::{self, Down, Picky, Up, echo};

    pub fn run(scenario: &str) {
        match scenario {
            "echo-blocking" => echo_blocking(),
            "echo-tokio" => support::current_thread_runtime().block_on(echo_tokio()),
            "lanes" => lanes(),
            "count" => count(),
            "receive-only" => receive_only(),
            "burst-then-exit" => burst_then_exit(),
            "ready-then-abort" => ready_then_abort(),
            "ready-then-wait" => ready_then_wait(),
            "send-until-disconnected" => send_until_disconnected(),
            "expect-rejected-token" => expect_rejected(RejectReason::Token),
            "wrong-types" => wrong_types(),
            "junk" => junk(),
            "send-oversized" => send_oversized(),
            "expect-oversized" => expect_oversized(),
            "picky" => picky(),
            "expect-connect-failure" => expect_connect_failure(),
            "expect-not-unicode" => expect_not_unicode(),
            "report-disconnect" => support::current_thread_runtime().block_on(report_disconnect()),
            "drop-sender-keep-receiving" => {
                support::current_thread_runtime().block_on(drop_sender_keep_receiving())
            }
            "blocking-drop-sender-keep-receiving" => blocking_drop_sender_keep_receiving(),
            other => panic!("unknown scenario {other}"),
        }
    }

    fn bootstrap() -> Bootstrap {
        Bootstrap::from_env().expect("the parent passed a bootstrap")
    }

    fn connect() -> (blocking::Sender<Up>, blocking::Receiver<Down>) {
        blocking::connect(&bootstrap()).expect("the child connects")
    }

    fn echo_blocking() {
        let (tx, rx) = connect();
        for message in rx {
            tx.send(&echo(message.expect("each message decodes")))
                .unwrap();
        }
    }

    async fn echo_tokio() {
        let (tx, mut rx) = nyanpasu_local_channel::tokio::connect::<Up, Down>(&bootstrap())
            .await
            .expect("the child connects");
        loop {
            match rx.recv().await {
                Ok(message) => tx.send(&echo(message)).await.unwrap(),
                Err(RecvError::Disconnected) => break,
                Err(error) => panic!("{error:?}"),
            }
        }
    }

    /// Eight threads send through clones of one sender at once.
    fn lanes() {
        let (tx, _rx) = connect();
        let per_lane = support::arg();
        let threads: Vec<_> = (0..8)
            .map(|lane| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    for seq in 0..per_lane {
                        tx.send(&Up::Seq { lane, seq }).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }

    fn count() {
        let (_tx, rx) = connect();
        let mut next = 0;
        for message in rx {
            assert_eq!(message.unwrap(), Down::Seq(next));
            next += 1;
        }
        assert_eq!(next, support::arg(), "every queued message arrived");
    }

    fn receive_only() {
        let (_tx, rx) = blocking::connect::<Never, Down>(&bootstrap()).unwrap();
        let received = rx.map(Result::unwrap).count() as u64;
        assert_eq!(received, support::arg());
    }

    /// Exits the process right after the last write, without closing anything.
    fn burst_then_exit() {
        let (tx, _rx) = connect();
        for seq in 0..support::arg() {
            tx.send(&Up::Seq { lane: 0, seq }).unwrap();
        }
        std::process::exit(0);
    }

    fn ready_then_abort() {
        let (tx, _rx) = connect();
        tx.send(&Up::Ready).unwrap();
        std::process::abort();
    }

    /// Blocks in `recv` until the parent kills this process.
    fn ready_then_wait() {
        let (tx, mut rx) = connect();
        tx.send(&Up::Ready).unwrap();
        let _ = rx.recv();
        panic!("the parent kills the child before it closes the channel");
    }

    fn send_until_disconnected() {
        let (tx, mut rx) = connect();
        tx.send(&Up::Ready).unwrap();
        assert!(matches!(rx.recv(), Err(RecvError::Disconnected)));
        let blob = Up::Blob(vec![0; 64 * 1024]);
        let error = loop {
            if let Err(error) = tx.send(&blob) {
                break error;
            }
        };
        assert!(matches!(error, SendError::Disconnected), "{error:?}");
        // A broken sender stays broken.
        assert!(matches!(tx.send(&Up::Ready), Err(SendError::Disconnected)));
    }

    fn expect_rejected(expected: RejectReason) {
        let error = blocking::connect::<Up, Down>(&bootstrap())
            .err()
            .expect("the server refuses");
        assert!(
            matches!(
                &error,
                ConnectError::Handshake { source: HandshakeError::Rejected { reason } }
                    if *reason == expected
            ),
            "{error:?}"
        );
    }

    /// Connects with the directions swapped, as a child built against other types would.
    fn wrong_types() {
        let error = blocking::connect::<Down, Up>(&bootstrap())
            .err()
            .expect("the server refuses");
        assert!(
            matches!(
                &error,
                ConnectError::Handshake {
                    source: HandshakeError::Rejected {
                        reason: RejectReason::Types
                    }
                }
            ),
            "{error:?}"
        );
    }

    /// Connections that are not a client at all.
    fn junk() {
        let bootstrap = bootstrap();

        // Connects and leaves at once.
        drop(support::raw_connect(&bootstrap));

        // A well-formed frame that is not a hello: the server hangs up.
        let mut stream = support::raw_connect(&bootstrap);
        let mut frame = 8_u32.to_le_bytes().to_vec();
        frame.extend_from_slice(b"GET / HT");
        stream.write_all(&frame).unwrap();
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
        assert!(rest.is_empty(), "the server says nothing to a stranger");

        // A prefix announcing far more than a handshake: the server hangs up.
        let mut stream = support::raw_connect(&bootstrap);
        stream.write_all(&u32::MAX.to_le_bytes()).unwrap();
        let _ = stream.read_to_end(&mut rest);
        assert!(rest.is_empty());
    }

    fn send_oversized() {
        let (tx, mut rx) = connect();
        let len = support::arg() as usize;
        tx.send(&Up::Blob(vec![1; len])).unwrap();
        assert!(matches!(rx.recv(), Err(RecvError::Disconnected)));
    }

    fn expect_oversized() {
        let options = Options::default().with_max_frame_len(1024);
        let (_tx, mut rx) =
            blocking::connect_with_options::<Up, Down>(&bootstrap(), options).unwrap();
        let error = rx.recv().unwrap_err();
        assert!(
            matches!(error, RecvError::FrameTooLarge { len, max: 1024 } if len > 1024),
            "{error:?}"
        );
        assert!(
            matches!(rx.recv(), Err(RecvError::Disconnected)),
            "the channel ended"
        );
    }

    fn picky() {
        let (tx, mut rx) = blocking::connect::<Up, Picky>(&bootstrap()).unwrap();
        let error = rx.recv().unwrap_err();
        assert!(matches!(error, RecvError::Decode { .. }), "{error:?}");
        assert_eq!(
            rx.recv().unwrap(),
            Picky(1),
            "the channel survives a decode error"
        );
        tx.send(&Up::Ready).unwrap();
        assert!(matches!(rx.recv(), Err(RecvError::Disconnected)));
    }

    fn expect_connect_failure() {
        let error = blocking::connect::<Up, Down>(&bootstrap())
            .err()
            .expect("nothing listens");
        assert!(matches!(error, ConnectError::Connect { .. }), "{error:?}");
    }

    /// Tells the parent when this end sees the channel end, which reaches it
    /// only if the parent still reads.
    async fn report_disconnect() {
        let (tx, mut rx) = nyanpasu_local_channel::tokio::connect::<Up, Down>(&bootstrap())
            .await
            .unwrap();
        tx.send(&Up::Ready).await.unwrap();
        assert!(matches!(rx.recv().await, Err(RecvError::Disconnected)));
        let _ = tx.send(&Up::Text("disconnected".to_owned())).await;
    }

    async fn drop_sender_keep_receiving() {
        let (tx, mut rx) = nyanpasu_local_channel::tokio::connect::<Up, Down>(&bootstrap())
            .await
            .unwrap();
        tx.send(&Up::Ready).await.unwrap();
        drop(tx);
        for seq in 0..support::arg() {
            assert_eq!(rx.recv().await.unwrap(), Down::Seq(seq));
        }
        assert!(matches!(rx.recv().await, Err(RecvError::Disconnected)));
    }

    fn blocking_drop_sender_keep_receiving() {
        let (tx, mut rx) = connect();
        tx.send(&Up::Ready).unwrap();
        drop(tx);
        for seq in 0..support::arg() {
            assert_eq!(rx.recv().unwrap(), Down::Seq(seq));
        }
        assert!(matches!(rx.recv(), Err(RecvError::Disconnected)));
    }

    fn expect_not_unicode() {
        let error = Bootstrap::from_env().unwrap_err();
        assert!(
            matches!(&error, BootstrapError::NotUnicode { var } if var == Bootstrap::ENV),
            "{error:?}"
        );
    }
}

async fn round_trip(scenario: &str) {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer(scenario, bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    let messages = [
        Down::Text(String::new()),
        Down::Text("héllo, 世界 ✓".to_owned()),
        Down::Seq(0),
        Down::Seq(u64::MAX),
        Down::Lane { lane: 3, seq: 9 },
        Down::Blob(Vec::new()),
        Down::Blob(pattern(256 * 1024, 1)),
        Down::End,
    ];
    for message in messages {
        tx.send(&message).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), echo(message));
    }
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blocking_child_echoes_every_kind_of_message() {
    round_trip("echo-blocking").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tokio_child_echoes_every_kind_of_message() {
    round_trip("echo-tokio").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_in_the_same_process_talks_to_the_server() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let (accepted, connected) =
        tokio::join!(accept(server), channel::connect::<Up, Down>(&bootstrap));
    let (server_tx, mut server_rx) = accepted;
    let (client_tx, mut client_rx) = connected.unwrap();
    server_tx.send(&Down::Seq(1)).await.unwrap();
    assert_eq!(client_rx.recv().await.unwrap(), Down::Seq(1));
    client_tx.send(&Up::Ready).await.unwrap();
    assert_eq!(server_rx.recv().await.unwrap(), Up::Ready);
    drop((server_tx, server_rx));
    assert!(matches!(
        client_rx.recv().await,
        Err(RecvError::Disconnected)
    ));
    assert!(
        matches!(client_rx.recv().await, Err(RecvError::Disconnected)),
        "it stays ended"
    );
}

/// Guards the harness: several tests pass only through assertions the child
/// makes, which would pass vacuously if the child ran no scenario at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_child_runs_its_scenario_and_reports_a_failure() {
    let (_server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("no-such-scenario", bootstrap.to_string(), None);
    let status = tokio::time::timeout(support::DEADLINE, child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success(), "a panicking scenario fails the child");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clones_sending_from_many_threads_keep_every_frame_whole() {
    let per_lane = 2_000;
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("lanes", bootstrap.to_string(), Some(&per_lane.to_string()));
    let (_tx, mut rx) = accept(server).await;
    let mut next = [0_u64; 8];
    loop {
        match rx.recv().await {
            Ok(Up::Seq { lane, seq }) => {
                assert_eq!(seq, next[lane as usize], "lane {lane} stays in order");
                next[lane as usize] += 1;
            }
            Ok(other) => panic!("{other:?}"),
            Err(RecvError::Disconnected) => break,
            Err(error) => panic!("{error:?}"),
        }
    }
    assert_eq!(next, [per_lane; 8]);
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_messages_reach_the_child_after_the_parent_lets_go() {
    let total = 5_000_u64;
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("count", bootstrap.to_string(), Some(&total.to_string()));
    let (tx, rx) = accept(server).await;
    for seq in 0..total {
        tx.send(&Down::Seq(seq)).await.unwrap();
    }
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn messages_written_before_the_child_exits_are_all_received() {
    let total = 5_000_u64;
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer(
        "burst-then-exit",
        bootstrap.to_string(),
        Some(&total.to_string()),
    );
    let (_tx, mut rx) = accept(server).await;
    for seq in 0..total {
        assert_eq!(rx.recv().await.unwrap(), Up::Seq { lane: 0, seq });
    }
    assert!(matches!(rx.recv().await, Err(RecvError::Disconnected)));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_one_way_channel_carries_never_back() {
    let total = 1_000_u64;
    let (server, bootstrap) = OneShotServer::<Down, Never>::new("e2e").unwrap();
    let mut child = spawn_peer(
        "receive-only",
        bootstrap.to_string(),
        Some(&total.to_string()),
    );
    let (tx, rx) = accept(server).await;
    drop(rx);
    for seq in 0..total {
        tx.send(&Down::Seq(seq)).await.unwrap();
    }
    drop(tx);
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_crashed_child_is_a_disconnect_on_both_directions() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("ready-then-abort", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    assert_eq!(rx.recv().await.unwrap(), Up::Ready);
    assert!(matches!(rx.recv().await, Err(RecvError::Disconnected)));
    let status = child.wait().await.unwrap();
    assert!(!status.success(), "the child aborted");

    let blob = Down::Blob(vec![0; 64 * 1024]);
    let error = tokio::time::timeout(support::DEADLINE, async {
        loop {
            if let Err(error) = tx.send(&blob).await {
                break error;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("sending to a dead child fails in time");
    assert!(matches!(error, SendError::Disconnected), "{error:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_killed_child_is_a_disconnect() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("ready-then-wait", bootstrap.to_string(), None);
    let (_tx, mut rx) = accept(server).await;
    assert_eq!(rx.recv().await.unwrap(), Up::Ready);
    child.kill().await.unwrap();
    assert!(matches!(rx.recv().await, Err(RecvError::Disconnected)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blocking_sender_reports_the_parent_leaving() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("send-until-disconnected", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    assert_eq!(rx.recv().await.unwrap(), Up::Ready);
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_token_is_rejected_and_the_child_is_still_accepted() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let accepting = tokio::spawn(accept(server));

    let tampered = support::tamper_token(&bootstrap.to_string());
    assert_ne!(tampered, bootstrap.to_string());
    let mut impostor = spawn_peer("expect-rejected-token", &tampered, None);
    expect_success(&mut impostor).await;
    assert!(!accepting.is_finished(), "the server waits for its child");

    let mut child = spawn_peer("echo-blocking", bootstrap.to_string(), None);
    let (tx, mut rx) = accepting.await.unwrap();
    tx.send(&Down::Seq(7)).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Up::Seq { lane: 0, seq: 7 });
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connections_that_are_not_clients_are_dropped_and_the_wait_goes_on() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let accepting = tokio::spawn(accept(server));
    let mut stranger = spawn_peer("junk", bootstrap.to_string(), None);
    expect_success(&mut stranger).await;
    assert!(!accepting.is_finished(), "the server waits for its child");

    let mut child = spawn_peer("echo-blocking", bootstrap.to_string(), None);
    let (tx, rx) = accepting.await.unwrap();
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_with_other_types_fails_on_both_sides() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("wrong-types", bootstrap.to_string(), None);
    let error = tokio::time::timeout(support::DEADLINE, server.accept())
        .await
        .expect("the child connects in time")
        .err()
        .expect("the accept fails");
    assert!(
        matches!(
            error,
            AcceptError::Handshake {
                source: HandshakeError::TypeMismatch
            }
        ),
        "{error:?}"
    );
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_oversized_incoming_frame_ends_the_channel() {
    let options = Options::default().with_max_frame_len(1024);
    let (server, bootstrap) = OneShotServer::<Down, Up>::with_options("e2e", options).unwrap();
    let mut child = spawn_peer("send-oversized", bootstrap.to_string(), Some("4096"));
    let (tx, mut rx) = accept(server).await;
    let error = rx.recv().await.unwrap_err();
    assert!(
        matches!(error, RecvError::FrameTooLarge { len, max: 1024 } if len > 4096),
        "{error:?}"
    );
    assert!(
        matches!(rx.recv().await, Err(RecvError::Disconnected)),
        "the channel ended"
    );
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_oversized_outgoing_message_is_refused_and_the_channel_stays_usable() {
    let options = Options::default().with_max_frame_len(1024);
    let (server, bootstrap) = OneShotServer::<Down, Up>::with_options("e2e", options).unwrap();
    let mut child = spawn_peer("echo-blocking", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    let error = tx.send(&Down::Blob(vec![0; 4096])).await.unwrap_err();
    assert!(
        matches!(error, SendError::FrameTooLarge { len, max: 1024 } if len > 4096),
        "{error:?}"
    );
    tx.send(&Down::Seq(5)).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Up::Seq { lane: 0, seq: 5 });
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blocking_receiver_refuses_a_frame_over_its_own_limit() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("expect-oversized", bootstrap.to_string(), None);
    let (tx, rx) = accept(server).await;
    tx.send(&Down::Blob(vec![0; 4096])).await.unwrap();
    // The child hangs up once it has refused the frame.
    expect_success(&mut child).await;
    drop((tx, rx));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_that_does_not_decode_is_skipped_on_both_receivers() {
    // Blocking receiver in the child.
    let (server, bootstrap) = OneShotServer::<Picky, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("picky", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    tx.send(&Picky(13)).await.unwrap();
    tx.send(&Picky(1)).await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), Up::Ready);
    drop((tx, rx));
    expect_success(&mut child).await;

    // Async receiver in this process.
    let (server, bootstrap) = OneShotServer::<Never, Picky>::new("e2e").unwrap();
    let (accepted, connected) =
        tokio::join!(accept(server), channel::connect::<Picky, Never>(&bootstrap));
    let (_server_tx, mut server_rx) = accepted;
    let (client_tx, _client_rx) = connected.unwrap();
    client_tx.send(&Picky(13)).await.unwrap();
    client_tx.send(&Picky(2)).await.unwrap();
    let error = server_rx.recv().await.unwrap_err();
    assert!(matches!(error, RecvError::Decode { .. }), "{error:?}");
    assert_eq!(server_rx.recv().await.unwrap(), Picky(2));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recv_loses_nothing_when_select_cancels_it() {
    let total = 20_000_u64;
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer(
        "burst-then-exit",
        bootstrap.to_string(),
        Some(&total.to_string()),
    );
    let (_tx, mut rx) = accept(server).await;
    let mut tick = tokio::time::interval(Duration::from_micros(50));
    let (mut next, mut ticks) = (0, 0_u64);
    loop {
        tokio::select! {
            biased;
            _ = tick.tick() => ticks += 1,
            received = rx.recv() => match received {
                Ok(message) => {
                    assert_eq!(message, Up::Seq { lane: 0, seq: next });
                    next += 1;
                }
                Err(RecvError::Disconnected) => break,
                Err(error) => panic!("{error:?}"),
            },
        }
    }
    assert_eq!(next, total);
    assert!(ticks > 1, "recv was cancelled along the way");
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_accept_frees_the_name() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let waited = tokio::time::timeout(Duration::from_millis(20), server.accept()).await;
    assert!(waited.is_err(), "nobody connected");
    #[cfg(unix)]
    assert!(
        !std::path::Path::new(bootstrap.name()).exists(),
        "the socket file is gone"
    );
    let mut child = spawn_peer("expect-connect-failure", bootstrap.to_string(), None);
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_server_frees_the_name() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    drop(server);
    let mut child = spawn_peer("expect-connect-failure", bootstrap.to_string(), None);
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_after_the_child_cannot_connect() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("echo-blocking", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    #[cfg(unix)]
    assert!(
        !std::path::Path::new(bootstrap.name()).exists(),
        "the socket file is gone"
    );
    // On Unix the late client fails at once; on Windows it waits for a free
    // pipe instance, which never comes, until the channel closes.
    let mut late = spawn_peer("expect-connect-failure", bootstrap.to_string(), None);
    tx.send(&Down::End).await.unwrap();
    assert_eq!(
        rx.recv().await.unwrap(),
        Up::Ready,
        "the channel is unaffected"
    );
    drop((tx, rx));
    expect_success(&mut child).await;
    expect_success(&mut late).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_names_a_fresh_socket_each_time() {
    let (_first, first) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let (_second, second) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    assert_ne!(first.name(), second.name());
    assert_ne!(first.to_string(), second.to_string());
    let parsed: Bootstrap = first.to_string().parse().unwrap();
    assert_eq!(parsed, first);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_socket_lives_in_a_directory_private_to_the_user() {
    use std::os::unix::fs::MetadataExt as _;

    let (_server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let socket = std::path::Path::new(bootstrap.name());
    let dir = socket.parent().unwrap().symlink_metadata().unwrap();
    assert!(dir.is_dir());
    // SAFETY: geteuid has no preconditions and cannot fail.
    assert_eq!(dir.uid(), unsafe { libc::geteuid() });
    assert_eq!(dir.mode() & 0o077, 0, "{:o}", dir.mode());
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pipe_grants_access_to_the_current_user_only() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let accepting = tokio::spawn(server.accept());
    let name = bootstrap.name().to_owned();
    let dacl = tokio::task::spawn_blocking(move || windows_dacl::of_pipe(&name))
        .await
        .unwrap();
    // Compared in the form Windows prints, which abbreviates well-known SIDs
    // (the built-in Administrator a CI runner may use is `LA`).
    let expected = format!("D:P(A;;FA;;;{})", windows_dacl::current_user_sid());
    assert_eq!(dacl, windows_dacl::canonical(&expected));
    accepting.abort();
}

#[cfg(windows)]
mod windows_dacl {
    use std::{os::windows::ffi::OsStrExt as _, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW,
                ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
                SDDL_REVISION_1, SE_KERNEL_OBJECT,
            },
            DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
        Storage::FileSystem::{CreateFileW, OPEN_EXISTING, READ_CONTROL},
    };

    /// The DACL of the pipe, read through a client handle opened with READ_CONTROL.
    pub fn of_pipe(name: &str) -> String {
        let path: Vec<u16> = std::ffi::OsStr::new(&format!(r"\\.\pipe\{name}"))
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: the path is NUL-terminated; every out-parameter is filled by
        // the call, and each handle and buffer it allocates is released.
        unsafe {
            let handle = CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
                0,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            );
            assert_ne!(
                handle,
                INVALID_HANDLE_VALUE,
                "{}",
                std::io::Error::last_os_error()
            );
            let mut descriptor = ptr::null_mut();
            let status = GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            );
            assert_eq!(status, 0, "GetSecurityInfo");
            let sddl = dacl_text(descriptor);
            LocalFree(descriptor);
            CloseHandle(handle);
            sddl
        }
    }

    /// `sddl` as Windows prints it back: parsed, then converted to text again.
    pub fn canonical(sddl: &str) -> String {
        let wide: Vec<u16> = std::ffi::OsStr::new(sddl)
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: the string is NUL-terminated; the descriptor the call
        // allocates is released after use.
        unsafe {
            let mut descriptor = ptr::null_mut();
            let parsed = ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            );
            assert_ne!(parsed, 0, "{sddl}: {}", std::io::Error::last_os_error());
            let text = dacl_text(descriptor);
            LocalFree(descriptor);
            text
        }
    }

    /// The DACL part of a security descriptor, as SDDL.
    ///
    /// # Safety
    ///
    /// `descriptor` must point to a valid security descriptor.
    unsafe fn dacl_text(descriptor: PSECURITY_DESCRIPTOR) -> String {
        // SAFETY: the caller vouches for the descriptor; the string the call
        // allocates is read up to its NUL and then released.
        unsafe {
            let mut text = ptr::null_mut();
            let converted = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                ptr::null_mut(),
            );
            assert_ne!(converted, 0, "{}", std::io::Error::last_os_error());
            let len = (0..).take_while(|&index| *text.add(index) != 0).count();
            let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            sddl
        }
    }

    /// The SID of the user running the tests, as `whoami` reports it.
    pub fn current_user_sid() -> String {
        let output = std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("whoami runs");
        let text = String::from_utf8_lossy(&output.stdout);
        text.trim()
            .rsplit(',')
            .next()
            .expect("whoami prints the SID last")
            .trim_matches('"')
            .to_owned()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bootstrap_that_is_not_unicode_is_reported() {
    #[cfg(windows)]
    let value = {
        use std::os::windows::ffi::OsStringExt as _;
        std::ffi::OsString::from_wide(&[0xd800])
    };
    #[cfg(unix)]
    let value = {
        use std::os::unix::ffi::OsStringExt as _;
        std::ffi::OsString::from_vec(vec![0xff])
    };
    let mut child = spawn_peer("expect-not-unicode", value, None);
    expect_success(&mut child).await;
}

/// What a stand-in server does once a client connects.
#[derive(Debug, Clone, Copy)]
enum FakeServer {
    /// Closes the connection without reading anything.
    HangUpAtOnce,
    /// Reads the hello, then closes the connection without replying.
    HangUpAfterHello,
    /// Answers with a prefix announcing more than a handshake frame holds.
    OversizedReply,
    /// Answers with a frame that is not a reply.
    GarbageReply,
}

/// Listens on a socket like a server but does not speak the protocol; it
/// serves one client and returns once that client has gone.
fn fake_server(
    label: &str,
    behavior: FakeServer,
) -> (
    Bootstrap,
    std::thread::JoinHandle<()>,
    Option<tempfile::TempDir>,
) {
    use std::io::{Read as _, Write as _};

    use interprocess::local_socket::{ListenerOptions, prelude::*};

    #[cfg(windows)]
    let (name, socket, dir) = {
        let name = format!("nyanpasu-fake-{}-{label}", std::process::id());
        let socket = std::ffi::OsString::from(&name)
            .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
            .unwrap();
        (name, socket, None)
    };
    #[cfg(unix)]
    let (name, socket, dir) = {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{label}.sock"));
        let name = path.to_str().unwrap().to_owned();
        let socket = path
            .to_fs_name::<interprocess::local_socket::GenericFilePath>()
            .unwrap();
        (name, socket, Some(dir))
    };
    let listener = ListenerOptions::new().name(socket).create_sync().unwrap();
    let server = std::thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        if let FakeServer::HangUpAtOnce = behavior {
            return;
        }
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut hello = vec![0; u32::from_le_bytes(prefix) as usize];
        stream.read_exact(&mut hello).unwrap();
        match behavior {
            FakeServer::HangUpAtOnce | FakeServer::HangUpAfterHello => return,
            FakeServer::OversizedReply => stream.write_all(&1_000_u32.to_le_bytes()).unwrap(),
            FakeServer::GarbageReply => stream.write_all(&[1, 0, 0, 0, 9]).unwrap(),
        }
        let _ = stream.read_to_end(&mut Vec::new());
    });
    let bootstrap = format!("v1:{}:{name}", "00".repeat(32)).parse().unwrap();
    (bootstrap, server, dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_clients_report_a_server_that_breaks_the_handshake() {
    use nyanpasu_local_channel::ConnectError;

    for (label, behavior) in [
        ("at-once", FakeServer::HangUpAtOnce),
        ("after-hello", FakeServer::HangUpAfterHello),
        ("oversized", FakeServer::OversizedReply),
        ("garbage", FakeServer::GarbageReply),
    ] {
        for blocking in [true, false] {
            let (bootstrap, server, _dir) = fake_server(&format!("{label}-{blocking}"), behavior);
            let error = if blocking {
                let bootstrap = bootstrap.clone();
                tokio::task::spawn_blocking(move || {
                    nyanpasu_local_channel::blocking::connect::<Up, Down>(&bootstrap).err()
                })
                .await
                .unwrap()
            } else {
                channel::connect::<Up, Down>(&bootstrap).await.err()
            }
            .expect("the handshake fails");
            let ConnectError::Handshake { source } = &error else {
                panic!("{behavior:?}, blocking {blocking}: {error:?}");
            };
            let expected = match behavior {
                FakeServer::HangUpAtOnce | FakeServer::HangUpAfterHello => {
                    matches!(source, HandshakeError::Closed)
                }
                FakeServer::OversizedReply | FakeServer::GarbageReply => {
                    matches!(source, HandshakeError::Malformed)
                }
            };
            assert!(expected, "{behavior:?}, blocking {blocking}: {error:?}");
            server.join().unwrap();
        }
    }
}

/// How long a test waits to conclude that nothing arrives.
const QUIET: Duration = Duration::from_millis(500);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_only_the_senders_keeps_the_connection_open() {
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
    let mut child = spawn_peer("report-disconnect", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    assert_eq!(rx.recv().await.unwrap(), Up::Ready);
    drop(tx);
    let early = tokio::time::timeout(QUIET, rx.recv()).await;
    assert!(
        early.is_err(),
        "the child saw the channel end while this end still receives: {early:?}"
    );
    drop(rx);
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_that_drops_only_its_sender_keeps_receiving() {
    for scenario in [
        "drop-sender-keep-receiving",
        "blocking-drop-sender-keep-receiving",
    ] {
        let total = 100_u64;
        let (server, bootstrap) = OneShotServer::<Down, Up>::new("e2e").unwrap();
        let mut child = spawn_peer(scenario, bootstrap.to_string(), Some(&total.to_string()));
        let (tx, mut rx) = accept(server).await;
        assert_eq!(rx.recv().await.unwrap(), Up::Ready);
        let early = tokio::time::timeout(QUIET, rx.recv()).await;
        assert!(
            early.is_err(),
            "{scenario}: the channel ended early: {early:?}"
        );
        for seq in 0..total {
            tx.send(&Down::Seq(seq)).await.unwrap();
        }
        drop((tx, rx));
        expect_success(&mut child).await;
    }
}
