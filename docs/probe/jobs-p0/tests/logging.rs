use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    Layer,
    filter::LevelFilter,
    layer::{Context, SubscriberExt},
};

#[derive(Clone, Default)]
struct Count(Arc<AtomicUsize>);
impl<S: Subscriber> Layer<S> for Count {
    fn on_event(&self, _: &Event<'_>, _: Context<'_, S>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn global_filter_hides_job_events_but_per_layer_filters_keep_them() {
    let hidden = Count::default();
    tracing::subscriber::with_default(
        tracing_subscriber::registry()
            .with(LevelFilter::WARN)
            .with(hidden.clone()),
        || tracing::info!("profile committed"),
    );
    assert_eq!(hidden.0.load(Ordering::SeqCst), 0);
    let file = Count::default();
    let journal = Count::default();
    tracing::subscriber::with_default(
        tracing_subscriber::registry()
            .with(file.clone().with_filter(LevelFilter::WARN))
            .with(journal.clone().with_filter(LevelFilter::INFO)),
        || tracing::info!("profile committed"),
    );
    assert_eq!(file.0.load(Ordering::SeqCst), 0);
    assert_eq!(journal.0.load(Ordering::SeqCst), 1);
}
