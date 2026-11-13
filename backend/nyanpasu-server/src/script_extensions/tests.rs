use std::{
    fs::{File, FileTimes},
    io::{Read as _, Seek as _, SeekFrom},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use boa_utils::module::http::{CachedItem, HttpModuleSource};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    runtime::Handle,
    sync::oneshot,
    task::JoinHandle,
};
use url::Url;

use super::HttpSource;

struct Fixture {
    source: HttpSource,
    url: Url,
    path: PathBuf,
    requests: Arc<AtomicUsize>,
    fetched: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    server: JoinHandle<()>,
    _cache: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let url = Url::parse(&format!("http://{address}/module")).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let (requested, fetched) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut gate = Some((requested, released));
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    assert_ne!(stream.read(&mut byte).await.unwrap(), 0);
                    request.push(byte[0]);
                }
                let ordinal = count.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some((requested, release)) = gate.take() {
                    requested.send(()).unwrap();
                    release.await.unwrap();
                }
                let body = format!("fetched module {ordinal}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let cache = tempfile::tempdir().unwrap();
        let mut source = HttpSource::new(cache.path().to_path_buf(), Handle::current()).unwrap();
        source.client = reqwest::Client::builder().no_proxy().build().unwrap();
        let path = cache
            .path()
            .join(format!("http_127--0--0--1__{}", address.port()))
            .join("_module");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        Self {
            source,
            url,
            path,
            requests,
            fetched,
            release: Some(release),
            server,
            _cache: cache,
        }
    }

    fn release_fetch(&mut self) {
        self.release.take().unwrap().send(()).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn concurrent_loads_publish_complete_cache_entries() {
    let mut fixture = Fixture::new().await;
    let old = CachedItem {
        mime: "text/plain".to_owned(),
        content: "cached module".repeat(128),
    };
    let bytes = postcard::to_stdvec(&old).unwrap();
    std::fs::write(&fixture.path, &bytes).unwrap();
    File::options()
        .write(true)
        .open(&fixture.path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
        .unwrap();

    let refresh = fixture.source.load(&fixture.url);
    tokio::pin!(refresh);
    tokio::select! {
        result = &mut refresh => panic!("fetch must wait for release: {result:?}"),
        result = &mut fixture.fetched => result.unwrap(),
    }

    // Retain the entry's identity without an open handle blocking replacement on Windows.
    let retained_path = fixture.path.with_extension("reader");
    std::fs::hard_link(&fixture.path, &retained_path).unwrap();
    let mut reader = File::options()
        .read(true)
        .write(true)
        .open(&retained_path)
        .unwrap();
    let mut observed = vec![0; bytes.len() / 2];
    reader.read_exact(&mut observed).unwrap();
    reader
        .set_times(FileTimes::new().set_modified(SystemTime::now()))
        .unwrap();
    drop(reader);
    let cached = fixture.source.load(&fixture.url).await.unwrap();
    assert_eq!(cached.mime, old.mime);
    assert_eq!(cached.content, old.content);

    fixture.release.take().unwrap().send(()).unwrap();
    let fetched = refresh.await.unwrap();
    assert_eq!(fetched.mime, "application/javascript");
    assert_eq!(fetched.content, "fetched module 1");
    let mut reader = File::open(&retained_path).unwrap();
    reader.seek(SeekFrom::Start(observed.len() as u64)).unwrap();
    reader.read_to_end(&mut observed).unwrap();
    let retained: CachedItem = postcard::from_bytes(&observed).unwrap();
    assert_eq!(retained.mime, old.mime);
    assert_eq!(retained.content, old.content);
    drop(reader);
    let published: CachedItem =
        postcard::from_bytes(&std::fs::read(&fixture.path).unwrap()).unwrap();
    assert_eq!(published.mime, fetched.mime);
    assert_eq!(published.content, fetched.content);
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn corrupt_cache_entry_is_fetched_again() {
    let mut fixture = Fixture::new().await;
    std::fs::write(&fixture.path, [0xff]).unwrap();
    fixture.release_fetch();

    let item = fixture.source.load(&fixture.url).await.unwrap();
    assert_eq!(item.mime, "application/javascript");
    assert_eq!(item.content, "fetched module 1");
    let cached: CachedItem = postcard::from_bytes(&std::fs::read(&fixture.path).unwrap()).unwrap();
    assert_eq!(cached.mime, item.mime);
    assert_eq!(cached.content, item.content);
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cache_hit_preserves_the_entry_timestamp() {
    let mut fixture = Fixture::new().await;
    fixture.release_fetch();
    fixture.source.load(&fixture.url).await.unwrap();
    let old = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    File::options()
        .write(true)
        .open(&fixture.path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old))
        .unwrap();
    let modified = std::fs::metadata(&fixture.path)
        .unwrap()
        .modified()
        .unwrap();

    let cached = fixture.source.load(&fixture.url).await.unwrap();
    assert_eq!(cached.content, "fetched module 1");
    assert_eq!(
        std::fs::metadata(&fixture.path)
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
}
