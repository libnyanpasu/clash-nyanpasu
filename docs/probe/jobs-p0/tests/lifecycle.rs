use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{oneshot, watch};

#[tokio::test]
async fn abort_does_not_stop_started_blocking_work_or_release_its_slot() {
    let slot = Arc::new(AtomicBool::new(true));
    let worker_slot = slot.clone();
    let (started, ready) = oneshot::channel();
    let (release, proceed) = std::sync::mpsc::channel();
    let task = tokio::task::spawn_blocking(move || {
        started.send(()).unwrap();
        let released = proceed.recv_timeout(Duration::from_secs(5)).is_ok();
        worker_slot.store(false, Ordering::SeqCst);
        released
    });
    ready.await.unwrap();
    task.abort();
    let still_occupied = slot.load(Ordering::SeqCst);
    release.send(()).unwrap();
    let completed = task.await.unwrap();
    assert!(still_occupied);
    assert!(completed);
    assert!(!slot.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn wait_timeout_does_not_cancel_owner_and_late_waiters_see_the_result() {
    let (result, mut first) = watch::channel(None::<u32>);
    let mut second = first.clone();
    let (release, proceed) = oneshot::channel();
    let worker = tokio::spawn(async move {
        proceed.await.unwrap();
        result.send_replace(Some(42));
        result
    });
    assert!(
        tokio::time::timeout(Duration::from_secs(1), first.wait_for(Option::is_some))
            .await
            .is_err()
    );
    release.send(()).unwrap();
    assert_eq!(*second.wait_for(Option::is_some).await.unwrap(), Some(42));
    let result = worker.await.unwrap();
    let mut late = result.subscribe();
    assert_eq!(*late.wait_for(Option::is_some).await.unwrap(), Some(42));
}
