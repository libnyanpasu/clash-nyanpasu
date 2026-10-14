use super::open_traffic_store;
use nyanpasu_paths::PathResolver;
use nyanpasu_traffic::{
    RedbTrafficStore, TrafficStore,
    accounting::{FlushBatch, Prune},
    model::SessionMeta,
};
use std::{
    fs,
    sync::{Arc, Mutex},
};
use tracing::{
    Level, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    Layer,
    layer::{Context, SubscriberExt},
};

fn paths(root: &std::path::Path) -> PathResolver {
    PathResolver::with_base_dirs(
        root.join("config").try_into().unwrap(),
        root.join("data").try_into().unwrap(),
    )
}

fn seed(store: &dyn TrafficStore) -> SessionMeta {
    let meta = SessionMeta {
        last_sample_at: Some(1234),
        instance_id: Some("retained-history".into()),
        global_counters: None,
    };
    store
        .flush(&FlushBatch {
            meta: meta.clone(),
            active: Vec::new(),
            removed: Vec::new(),
            minutes: Vec::new(),
            hours: Vec::new(),
            closed: Vec::new(),
            prune: Prune::default(),
        })
        .unwrap();
    meta
}

#[cfg(unix)]
fn assert_private(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn open_traffic_store_creates_at_resolved_location() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let store = open_traffic_store(&paths).unwrap();
    assert_eq!(store.load().unwrap(), None);
    let dir = paths.app_data_dir().join("traffic");
    assert!(dir.join("traffic.redb").is_file());
    assert!(!paths.app_config_dir().exists());
    #[cfg(unix)]
    assert_private(dir.as_std_path());
}

#[test]
fn open_traffic_store_hardens_existing_directory_and_retains_history() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let dir = paths.app_data_dir().join("traffic");
    fs::create_dir_all(&dir).unwrap();
    let store = RedbTrafficStore::open(dir.join("traffic.redb").as_std_path()).unwrap();
    let meta = seed(&store);
    drop(store);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
    }
    let store = open_traffic_store(&paths).unwrap();
    assert_eq!(store.load().unwrap(), Some((meta, Vec::new())));
    #[cfg(unix)]
    assert_private(dir.as_std_path());
}

#[derive(Default)]
struct Fields {
    message: String,
    error: String,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = format!("{value:?}"),
            "error" => self.error = format!("{value:?}"),
            _ => {}
        }
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<(Level, Fields)>>>);

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap()
            .push((*event.metadata().level(), fields));
    }
}

fn assert_disabled_with_error(paths: &PathResolver) {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::subscriber::with_default(subscriber, || {
        assert!(open_traffic_store(paths).is_none());
    });
    let events = capture.0.lock().unwrap();
    let failures: Vec<_> = events
        .iter()
        .filter(|(_, fields)| {
            fields.message == "failed to open the traffic store; recording is disabled"
        })
        .collect();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, Level::ERROR);
    assert!(!failures[0].1.error.is_empty());
}

#[test]
fn open_traffic_store_mkdir_error_disables_with_exact_event() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    fs::create_dir_all(paths.app_data_dir()).unwrap();
    let dir = paths.app_data_dir().join("traffic");
    fs::write(&dir, b"obstruction").unwrap();
    assert_disabled_with_error(&paths);
    assert_eq!(fs::read(dir).unwrap(), b"obstruction");
}

#[test]
fn open_traffic_store_open_error_disables_with_exact_event() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let db = paths.app_data_dir().join("traffic/traffic.redb");
    fs::create_dir_all(&db).unwrap();
    assert_disabled_with_error(&paths);
    assert!(db.is_dir());
    assert_eq!(fs::read_dir(db).unwrap().count(), 0);
}

#[test]
fn open_traffic_store_locked_database_disables_without_deleting_history() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let db = paths.app_data_dir().join("traffic/traffic.redb");
    let store = RedbTrafficStore::open(db.as_std_path()).unwrap();
    let meta = seed(&store);
    assert_disabled_with_error(&paths);
    assert!(db.is_file());
    assert_eq!(store.load().unwrap(), Some((meta.clone(), Vec::new())));
    drop(store);
    let reopened = open_traffic_store(&paths).unwrap();
    assert_eq!(reopened.load().unwrap(), Some((meta, Vec::new())));
}
