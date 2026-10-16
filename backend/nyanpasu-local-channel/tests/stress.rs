//! Load against a real child process: volume, large messages, contention,
//! backpressure, parallel channels and connection churn.
//!
//! `NYANPASU_LOCAL_CHANNEL_STRESS_SCALE` multiplies every workload (default 1,
//! sized for CI); throughput is printed to stderr.

mod support;

use std::time::{Duration, Instant};

use nyanpasu_local_channel::{RecvError, tokio::OneShotServer};
use support::{Down, Up, accept, digest, expect_success, pattern, spawn_peer};

#[test]
fn peer_entry() {
    if let Some(scenario) = support::scenario() {
        peer::run(&scenario);
    }
}

mod peer {
    use nyanpasu_local_channel::{Bootstrap, RecvError, blocking};

    use crate::support::{self, Down, Up, echo};

    pub fn run(scenario: &str) {
        match scenario {
            "echo-blocking" => echo_blocking(),
            "echo-tokio" => support::current_thread_runtime().block_on(echo_tokio()),
            "lane-checker" => lane_checker(),
            "stall" => stall(),
            other => panic!("unknown scenario {other}"),
        }
    }

    fn bootstrap() -> Bootstrap {
        Bootstrap::from_env().expect("the parent passed a bootstrap")
    }

    fn echo_blocking() {
        let (tx, rx) = blocking::connect::<Up, Down>(&bootstrap()).unwrap();
        for message in rx {
            tx.send(&echo(message.unwrap())).unwrap();
        }
    }

    async fn echo_tokio() {
        let (tx, mut rx) = nyanpasu_local_channel::tokio::connect::<Up, Down>(&bootstrap())
            .await
            .unwrap();
        loop {
            match rx.recv().await {
                Ok(message) => tx.send(&echo(message)).await.unwrap(),
                Err(RecvError::Disconnected) => break,
                Err(error) => panic!("{error:?}"),
            }
        }
    }

    /// Checks that each lane arrives in order, then reports the total at `End`.
    fn lane_checker() {
        let (tx, rx) = blocking::connect::<Up, Down>(&bootstrap()).unwrap();
        let mut next = Vec::<u64>::new();
        let mut count = 0;
        for message in rx {
            match message.unwrap() {
                Down::Lane { lane, seq } => {
                    let lane = lane as usize;
                    if next.len() <= lane {
                        next.resize(lane + 1, 0);
                    }
                    assert_eq!(seq, next[lane], "lane {lane} stays in order");
                    next[lane] += 1;
                    count += 1;
                }
                Down::End => tx.send(&Up::Summary { count }).unwrap(),
                other => panic!("{other:?}"),
            }
        }
    }

    /// Connects and never reads, until the parent kills it.
    fn stall() {
        let (tx, _rx) = blocking::connect::<Up, Down>(&bootstrap()).unwrap();
        tx.send(&Up::Ready).unwrap();
        loop {
            std::thread::park();
        }
    }
}

fn scale() -> u64 {
    std::env::var("NYANPASU_LOCAL_CHANNEL_STRESS_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

fn report(what: &str, count: u64, bytes: u64, started: Instant) {
    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "{what}: {count} messages, {:.1} MiB in {seconds:.2}s ({:.0} msg/s, {:.1} MiB/s)",
        bytes as f64 / 1048576.0,
        count as f64 / seconds,
        bytes as f64 / 1048576.0 / seconds,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_flood_in_both_directions_keeps_order() {
    let total = 100_000 * scale();
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("stress").unwrap();
    let mut child = spawn_peer("echo-tokio", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    let started = Instant::now();
    let sending = tokio::spawn(async move {
        for seq in 0..total {
            tx.send(&Down::Seq(seq)).await.unwrap();
        }
    });
    for seq in 0..total {
        assert_eq!(rx.recv().await.unwrap(), Up::Seq { lane: 0, seq });
    }
    report("flood round trips", total, 0, started);
    sending.await.unwrap();
    drop(rx);
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn large_messages_arrive_intact() {
    const BLOB: usize = 4 * 1024 * 1024;
    let count = 24 * scale();
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("stress").unwrap();
    let mut child = spawn_peer("echo-blocking", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    let started = Instant::now();
    let sending = tokio::spawn(async move {
        for seed in 0..count {
            tx.send(&Down::Blob(pattern(BLOB, seed))).await.unwrap();
        }
    });
    for seed in 0..count {
        let Up::Blob(blob) = rx.recv().await.unwrap() else {
            panic!("an echo of a blob is a blob");
        };
        assert_eq!(blob.len(), BLOB);
        assert_eq!(
            digest(&blob),
            digest(&pattern(BLOB, seed)),
            "blob {seed} is intact"
        );
    }
    report("4 MiB round trips", count, count * BLOB as u64 * 2, started);
    sending.await.unwrap();
    drop(rx);
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_senders_interleave_whole_frames() {
    let (lanes, per_lane) = (16_u32, 5_000 * scale());
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("stress").unwrap();
    let mut child = spawn_peer("lane-checker", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    let started = Instant::now();
    let senders: Vec<_> = (0..lanes)
        .map(|lane| {
            let tx = tx.clone();
            tokio::spawn(async move {
                for seq in 0..per_lane {
                    tx.send(&Down::Lane { lane, seq }).await.unwrap();
                }
            })
        })
        .collect();
    for sender in senders {
        sender.await.unwrap();
    }
    tx.send(&Down::End).await.unwrap();
    let total = u64::from(lanes) * per_lane;
    assert_eq!(rx.recv().await.unwrap(), Up::Summary { count: total });
    report("contended sends", total, 0, started);
    drop((tx, rx));
    expect_success(&mut child).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stalled_reader_bounds_what_the_sender_buffers() {
    const BLOB: usize = 64 * 1024;
    const LIMIT: usize = 64 * 1024 * 1024;
    let (server, bootstrap) = OneShotServer::<Down, Up>::new("stress").unwrap();
    let mut child = spawn_peer("stall", bootstrap.to_string(), None);
    let (tx, mut rx) = accept(server).await;
    assert_eq!(rx.recv().await.unwrap(), Up::Ready);
    let blob = Down::Blob(vec![7; BLOB]);
    let mut sent = 0;
    // A send that stays pending this long is waiting for the reader. Only the
    // operating system buffers what is written, so the sender stops once that
    // buffer is full, which may be before the first message.
    while tokio::time::timeout(Duration::from_millis(500), tx.send(&blob))
        .await
        .is_ok()
    {
        sent += BLOB;
        assert!(sent < LIMIT, "{sent} bytes went out without anyone reading");
    }
    eprintln!(
        "a stalled reader stopped the sender after {} KiB",
        sent / 1024
    );
    child.kill().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_children_each_keep_their_own_channel() {
    let (children, per_child) = (8, 2_000 * scale());
    let started = Instant::now();
    let runs: Vec<_> = (0..children)
        .map(|index| {
            tokio::spawn(async move {
                let (server, bootstrap) = OneShotServer::<Down, Up>::new("stress").unwrap();
                let mut child = spawn_peer("echo-blocking", bootstrap.to_string(), None);
                let (tx, mut rx) = accept(server).await;
                for seq in 0..per_child {
                    let text = format!("child {index} message {seq}");
                    tx.send(&Down::Text(text.clone())).await.unwrap();
                    assert_eq!(rx.recv().await.unwrap(), Up::Text(text));
                }
                drop((tx, rx));
                expect_success(&mut child).await;
            })
        })
        .collect();
    for run in runs {
        run.await.unwrap();
    }
    report("parallel ping-pong", children * per_child, 0, started);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_churn_leaks_no_handles_or_sockets() {
    let rounds = 300 * scale();
    // Warm up, so lazily created runtime resources are not counted as leaks.
    churn(5).await;
    let before = open_handles();
    let started = Instant::now();
    churn(rounds).await;
    report("connect, round trip, close", rounds, 0, started);

    // Writer tasks end on their own once their senders are gone.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut after = open_handles();
    while after > before + 32 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
        after = open_handles();
    }
    assert!(
        after <= before + 32,
        "{before} handles before, {after} after"
    );

    #[cfg(unix)]
    {
        let (_server, bootstrap) = OneShotServer::<Down, Up>::new("probe").unwrap();
        let dir = std::path::Path::new(bootstrap.name())
            .parent()
            .unwrap()
            .to_owned();
        let prefix = format!("churn-{}-", std::process::id());
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .collect();
        assert!(
            leftovers.is_empty(),
            "socket files left behind: {leftovers:?}"
        );
    }

    // The same through real child processes, fewer times.
    for _ in 0..20 * scale() {
        let (server, bootstrap) = OneShotServer::<Down, Up>::new("churn").unwrap();
        let mut child = spawn_peer("echo-blocking", bootstrap.to_string(), None);
        let (tx, mut rx) = accept(server).await;
        tx.send(&Down::End).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), Up::Ready);
        drop((tx, rx));
        expect_success(&mut child).await;
    }
}

async fn churn(rounds: u64) {
    for round in 0..rounds {
        let (server, bootstrap) = OneShotServer::<Down, Up>::new("churn").unwrap();
        let (accepted, connected) = tokio::join!(
            accept(server),
            nyanpasu_local_channel::tokio::connect::<Up, Down>(&bootstrap)
        );
        let (server_tx, mut server_rx) = accepted;
        let (client_tx, mut client_rx) = connected.unwrap();
        server_tx.send(&Down::Seq(round)).await.unwrap();
        let Down::Seq(seq) = client_rx.recv().await.unwrap() else {
            panic!("the client receives the round number");
        };
        client_tx.send(&Up::Seq { lane: 0, seq }).await.unwrap();
        assert_eq!(
            server_rx.recv().await.unwrap(),
            Up::Seq {
                lane: 0,
                seq: round
            }
        );
        drop((server_tx, server_rx));
        assert!(matches!(
            client_rx.recv().await,
            Err(RecvError::Disconnected)
        ));
    }
}

/// Handles (Windows) or file descriptors (Unix) this process holds.
fn open_handles() -> usize {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
        let mut count = 0;
        // SAFETY: the out-parameter is a valid u32.
        let ok = unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
        assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
        count as usize
    }
    #[cfg(unix)]
    {
        std::fs::read_dir("/dev/fd").unwrap().count()
    }
}
