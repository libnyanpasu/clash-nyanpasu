use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::sync::oneshot;

use super::*;

/// Flags its own drop, which is how a cancelled producer ends.
struct Dropped(Arc<AtomicBool>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn the_cancel_ends_a_running_producer_and_the_wait_sees_it_gone() {
    let (token, tasks) = (CancellationToken::new(), TaskTracker::new());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(Arc::clone(&dropped));
    let task = tokio::spawn(track_until_shutdown(&tasks, &token, async move {
        let _guard = guard;
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    }));
    started_rx.await.unwrap();

    token.cancel();
    tasks.close();
    tasks.wait().await;

    assert!(dropped.load(Ordering::SeqCst));
    task.await.expect("a cancelled producer ends normally");
}

#[tokio::test]
async fn a_producer_tracked_after_the_cancel_never_runs() {
    let (token, tasks) = (CancellationToken::new(), TaskTracker::new());
    token.cancel();

    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    tokio::spawn(track_until_shutdown(&tasks, &token, async move {
        flag.store(true, Ordering::SeqCst);
    }))
    .await
    .unwrap();
    assert!(!ran.load(Ordering::SeqCst));
}

#[tokio::test]
async fn tracking_registers_before_polling_and_dropping_releases_the_wait() {
    let (token, tasks) = (CancellationToken::new(), TaskTracker::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(Arc::clone(&dropped));
    let producer = track_until_shutdown(&tasks, &token, async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });

    tasks.close();
    assert_eq!(tasks.len(), 1, "registration does not wait for a poll");
    assert_wait_pending(&tasks).await;
    drop(producer);
    tasks.wait().await;

    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn tracking_after_cancel_and_close_still_waits_for_the_producer_drop() {
    let (token, tasks) = (CancellationToken::new(), TaskTracker::new());
    token.cancel();
    tasks.close();

    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(Arc::clone(&dropped));
    let producer = track_until_shutdown(&tasks, &token, async move {
        let _guard = guard;
        panic!("a producer registered after cancellation must not run");
    });
    assert_eq!(tasks.len(), 1);
    assert_wait_pending(&tasks).await;
    producer.await;
    tasks.wait().await;

    assert!(dropped.load(Ordering::SeqCst));
}

async fn assert_wait_pending(tasks: &TaskTracker) {
    tokio::select! {
        biased;
        () = tasks.wait() => panic!("the tracked work has not finished"),
        () = std::future::ready(()) => {}
    }
}

struct DrainActor;

struct Cleanup {
    started: Option<oneshot::Sender<()>>,
    release: Option<oneshot::Receiver<()>>,
    finished: Arc<AtomicBool>,
}

#[derive(Debug)]
enum DrainMessage {
    Work {
        started: oneshot::Sender<()>,
        release: oneshot::Receiver<()>,
    },
    Queued(oneshot::Sender<()>),
}

impl Actor for DrainActor {
    type Msg = DrainMessage;
    type State = Cleanup;
    type Arguments = Cleanup;

    async fn pre_start(
        &self,
        _: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        message: Self::Msg,
        _: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            DrainMessage::Work { started, release } => {
                started.send(()).unwrap();
                release.await.unwrap();
            }
            DrainMessage::Queued(handled) => handled.send(()).unwrap(),
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.started.take().unwrap().send(()).unwrap();
        state.release.take().unwrap().await.unwrap();
        state.finished.store(true, Ordering::SeqCst);
        Ok(())
    }
}

async fn assert_drain_waits_for_work_and_cleanup(register_after_cancel: bool) {
    let (token, tasks) = (CancellationToken::new(), TaskTracker::new());
    let (cleanup_started_tx, cleanup_started_rx) = oneshot::channel();
    let (cleanup_release_tx, cleanup_release_rx) = oneshot::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let (actor, actor_task) = Actor::spawn(
        None,
        DrainActor,
        Cleanup {
            started: Some(cleanup_started_tx),
            release: Some(cleanup_release_rx),
            finished: Arc::clone(&finished),
        },
    )
    .await
    .unwrap();
    let (work_started_tx, work_started_rx) = oneshot::channel();
    let (work_release_tx, work_release_rx) = oneshot::channel();
    actor
        .cast(DrainMessage::Work {
            started: work_started_tx,
            release: work_release_rx,
        })
        .unwrap();
    work_started_rx.await.unwrap();
    let (queued_tx, mut queued_rx) = oneshot::channel();
    actor.cast(DrainMessage::Queued(queued_tx)).unwrap();

    if register_after_cancel {
        token.cancel();
        tasks.close();
    }
    drain_on_shutdown(&tasks, token.clone(), actor.get_cell());
    if !register_after_cancel {
        token.cancel();
        tasks.close();
    }
    assert_wait_pending(&tasks).await;
    assert_eq!(
        queued_rx.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );

    work_release_tx.send(()).unwrap();
    queued_rx.await.unwrap();
    cleanup_started_rx.await.unwrap();
    let (late_tx, _) = oneshot::channel();
    assert!(actor.cast(DrainMessage::Queued(late_tx)).is_err());
    assert_wait_pending(&tasks).await;
    assert!(!finished.load(Ordering::SeqCst));

    cleanup_release_tx.send(()).unwrap();
    tasks.wait().await;
    actor_task.await.unwrap();
    assert!(finished.load(Ordering::SeqCst));

    // Registering an already-stopped owner on the closed tracker also settles.
    drain_on_shutdown(&tasks, token, actor.get_cell());
    tasks.wait().await;
}

#[tokio::test]
async fn drain_registered_before_cancel_waits_for_queued_work_and_post_stop() {
    assert_drain_waits_for_work_and_cleanup(false).await;
}

#[tokio::test]
async fn drain_registered_after_cancel_and_close_waits_for_queued_work_and_post_stop() {
    assert_drain_waits_for_work_and_cleanup(true).await;
}
