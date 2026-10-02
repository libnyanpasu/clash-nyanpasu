use super::*;
use serde_json::Value;
use tracing::Instrument;
use tracing_subscriber::layer::SubscriberExt;

fn events(directory: &Path) -> Vec<Value> {
    let path = fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn capture(test: impl FnOnce(&StartupTrace, &tracing::Dispatch)) -> Vec<Value> {
    let directory = tempfile::tempdir().unwrap();
    let (layer, trace) = layer(Some(directory.path())).unwrap();
    // Keep ordinary spans enabled, as the application's jobs layer does.
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(layer)
            .with(tracing_subscriber::layer::Identity::new()),
    );
    tracing::dispatcher::with_default(&dispatch, || {
        trace.entry();
        test(&trace, &dispatch);
        trace.finish();
    });
    events(directory.path())
}

fn named<'a>(events: &'a [Value], name: &str, phase: &str) -> &'a Value {
    events
        .iter()
        .find(|e| e["name"] == name && e["ph"] == phase)
        .unwrap()
}

#[test]
fn early_stages_and_first_milestones_are_exported_without_application_logs() {
    let events = capture(|trace, _| {
        drop(span!("entry.single_instance"));
        trace.milestone("main_window_ready");
        trace.milestone("main_window_ready");
        tracing::info!(target: "unrelated", secret = "not in startup trace");
    });
    let begin = named(&events, "entry.single_instance", "b");
    let end = named(&events, "entry.single_instance", "e");
    assert_eq!(begin["id"], end["id"]);
    assert!(end["ts"].as_f64().unwrap() >= begin["ts"].as_f64().unwrap());
    assert_eq!(
        named(&events, "startup_entry", "i")["args"]["pid"],
        std::process::id().to_string()
    );
    let milestones: Vec<_> = events
        .iter()
        .filter(|e| e["name"] == "startup.milestone")
        .collect();
    assert_eq!(milestones.len(), 1);
    assert_eq!(milestones[0]["args"]["milestone"], "main_window_ready");
    assert!(events.iter().all(|e| e["cat"] != "unrelated"));
}

#[test]
fn finishing_with_a_live_clone_keeps_an_unclosed_begin_in_valid_json() {
    let events = capture(|trace, _| {
        let stage = span!("setup.pending");
        let retained = stage.clone();
        drop(stage);
        trace.finish();
        trace.finish();
        drop(retained);
    });
    named(&events, "setup.pending", "b");
    assert!(
        !events
            .iter()
            .any(|e| e["name"] == "setup.pending" && e["ph"] == "e")
    );
    assert_eq!(
        events.iter().filter(|e| e["name"] == "capture_end").count(),
        1
    );
}

#[test]
fn last_clone_can_close_on_another_thread_before_finishing_the_file() {
    let events = capture(|_, dispatch| {
        let stage = span!("setup.cross_thread");
        let retained = stage.clone();
        drop(stage);
        let dispatch = dispatch.clone();
        std::thread::spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || drop(retained));
        })
        .join()
        .unwrap();
    });
    let begin = named(&events, "setup.cross_thread", "b");
    let end = named(&events, "setup.cross_thread", "e");
    assert_eq!(begin["id"], end["id"]);
    assert_ne!(begin["tid"], end["tid"]);
}

#[test]
fn asynchronous_errors_close_the_stage_after_the_awaited_work() {
    let events = capture(|_, _| {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let result = async {
                    tokio::task::yield_now().await;
                    tracing::info!(name: "after_await", target: TARGET, observed = true);
                    Err::<(), _>("failed")
                }
                .instrument(span!("reconcile.failure"))
                .await;
                assert!(result.is_err());
            });
    });
    let begin = named(&events, "reconcile.failure", "b");
    let after = named(&events, "after_await", "i");
    let end = named(&events, "reconcile.failure", "e");
    assert!(begin["ts"].as_f64().unwrap() <= after["ts"].as_f64().unwrap());
    assert!(after["ts"].as_f64().unwrap() <= end["ts"].as_f64().unwrap());
}

#[test]
fn spawning_an_actor_closes_the_startup_stage_while_the_actor_is_alive() {
    struct IdleActor;
    impl ractor::Actor for IdleActor {
        type Msg = ();
        type State = ();
        type Arguments = ();

        async fn pre_start(
            &self,
            _: ractor::ActorRef<Self::Msg>,
            _: Self::Arguments,
        ) -> Result<Self::State, ractor::ActorProcessingErr> {
            Ok(())
        }
    }

    let events = capture(|trace, _| {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let (actor, task) = ractor::Actor::spawn(None, IdleActor, ())
                    .instrument(span!("client.actor_startup"))
                    .await
                    .unwrap();
                assert!(!task.is_finished());
                trace.finish();
                actor.stop(None);
                task.await.unwrap();
            });
    });
    named(&events, "client.actor_startup", "b");
    named(&events, "client.actor_startup", "e");
}

#[test]
fn overlapping_stages_use_independent_async_tracks() {
    let events = capture(|_, _| {
        let first = span!("setup.first");
        let second = first.in_scope(|| span!("setup.second"));
        drop(first);
        drop(second);
    });
    assert_ne!(
        named(&events, "setup.first", "b")["id"],
        named(&events, "setup.second", "b")["id"]
    );
    for name in ["setup.first", "setup.second"] {
        assert_eq!(
            named(&events, name, "b")["id"],
            named(&events, name, "e")["id"]
        );
    }
}

#[test]
fn disabled_trace_has_no_writer_and_relative_directories_are_rejected() {
    let (layer, trace) = layer(None).unwrap();
    assert!(layer.is_none());
    assert!(trace.guard.lock().is_none());
    assert!(super::layer(Some(Path::new("relative-traces"))).is_err());
}
