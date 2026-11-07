use std::{
    fs::{File, FileTimes},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use boa_engine::{
    Context, Module, Source, builtins::promise::PromiseState, job::SimpleJobExecutor, js_string,
    module::SimpleModuleLoader,
};
use boa_utils::module::{
    builtin::BuiltinModuleLoader,
    combine::CombineModuleLoader,
    http::{CachedItem, HttpModuleLoader, HttpModuleSource},
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    runtime::Runtime,
    task::JoinHandle,
};
use url::Url;

use super::CachedHttpModuleSource;

struct Fixture {
    source: CachedHttpModuleSource,
    clock: Arc<Mutex<SystemTime>>,
    requests: Arc<AtomicUsize>,
    base: Url,
    server: JoinHandle<()>,
    _cache: tempfile::TempDir,
    runtime: Runtime,
}

impl Fixture {
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let server = runtime.spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if stream.read(&mut byte).await.unwrap() == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                let ordinal = count.fetch_add(1, Ordering::SeqCst) + 1;
                let (status, headers, body) = if !request.to_ascii_lowercase().contains("x-script-fixture: injected") {
                    ("200 OK", String::new(), "missing injected client".to_string())
                } else if let Some(hops) = path.strip_prefix("/redirect/") {
                    let hops: usize = hops.parse().unwrap();
                    if hops == 0 {
                        ("200 OK", String::new(), "redirected".to_string())
                    } else {
                        ("302 Found", format!("Location: /redirect/{}\r\n", hops - 1), String::new())
                    }
                } else {
                    let (mime, body) = match path {
                        "/yaml" => ("Content-Type: application/javascript\r\n", "export { default } from 'nyan:yaml';".to_string()),
                        "/base64" => ("Content-Type: application/javascript\r\n", "export { Base64 } from 'nyan:js-base64';".to_string()),
                        "/toolkit" => ("Content-Type: application/javascript\r\n", "export { isEqual } from 'nyan:es-toolkit';".to_string()),
                        "/from-async" => ("Content-Type: application/javascript\r\n", "export default async function fromAsync(values) { return Promise.all(values); }".to_string()),
                        "/workspace" => ("", "packages:\n  - frontend/*\n".to_string()),
                        "/javascript" => ("Content-Type: application/javascript\r\n", "export default 42;".to_string()),
                        "/json" => ("Content-Type: application/json\r\n", "{\"key\":42}".to_string()),
                        _ => ("", format!("injected response {ordinal}")),
                    };
                    ("200 OK", mime.to_string(), body)
                };
                let response = format!("HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let cache = tempfile::tempdir().unwrap();
        let clock = Arc::new(Mutex::new(SystemTime::now()));
        let observed_clock = clock.clone();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-script-fixture",
            reqwest::header::HeaderValue::from_static("injected"),
        );
        let client = reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap();
        let source = CachedHttpModuleSource::new(
            client,
            cache.path().to_path_buf(),
            Duration::from_secs(60),
            Arc::new(move || *observed_clock.lock().unwrap()),
            runtime.handle().clone(),
        );
        Self {
            source,
            clock,
            requests,
            base,
            server,
            _cache: cache,
            runtime,
        }
    }

    fn load(&self, path: &str) -> CachedItem {
        self.runtime
            .block_on(self.source.load(&self.base.join(path).unwrap()))
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[test]
fn module_fetch_uses_the_injected_client() {
    let fixture = Fixture::new();
    assert_eq!(fixture.load("module").content, "injected response 1");
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.load("redirect/4").content, "redirected");
    let error = fixture
        .runtime
        .block_on(
            fixture
                .source
                .load(&fixture.base.join("redirect/6").unwrap()),
        )
        .unwrap_err();
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .to_string()
            .contains("redirect"),
        "{error}"
    );
    let cause = std::error::Error::source(&error).unwrap();
    assert!(
        cause.source().is_some(),
        "the adapter must preserve the underlying redirect failure: {error}"
    );
}

#[test]
fn module_cache_expiry_uses_the_injected_clock() {
    let fixture = Fixture::new();
    assert_eq!(fixture.load("module?first").content, "injected response 1");
    let path = fixture
        .source
        .mapping_cache_dir(&fixture.base.join("module?first").unwrap());
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    *fixture.clock.lock().unwrap() = modified + Duration::from_secs(59);
    // Query strings have never participated in the persisted key.
    assert_eq!(fixture.load("module?second").content, "injected response 1");
    let path = fixture
        .source
        .mapping_cache_dir(&fixture.base.join("module").unwrap());
    let refreshed = std::fs::metadata(path).unwrap().modified().unwrap();
    *fixture.clock.lock().unwrap() = refreshed + Duration::from_secs(60);
    assert_eq!(fixture.load("module?third").content, "injected response 2");
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 2);
}

#[test]
fn cache_hit_preserves_existing_refresh_behavior() {
    let fixture = Fixture::new();
    fixture.load("module");
    let path = fixture
        .source
        .mapping_cache_dir(&fixture.base.join("module").unwrap());
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old))
        .unwrap();
    *fixture.clock.lock().unwrap() = old + Duration::from_secs(59);
    assert_eq!(fixture.load("module").content, "injected response 1");
    assert!(std::fs::metadata(path).unwrap().modified().unwrap() > old);
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
}

#[test]
fn test_http_module_loader() {
    let fixture = Fixture::new();
    let source = CachedHttpModuleSource::new(
        fixture.source.client.clone(),
        fixture.source.cache_dir.clone(),
        fixture.source.max_age,
        fixture.source.clock.clone(),
        fixture.runtime.handle().clone(),
    );
    let mut context = Context::builder()
        .job_executor(Rc::new(SimpleJobExecutor::new()))
        .module_loader(Rc::new(CombineModuleLoader::new(
            SimpleModuleLoader::new(fixture._cache.path()).unwrap(),
            HttpModuleLoader::new(Arc::new(source)),
            BuiltinModuleLoader,
        )))
        .build()
        .unwrap();
    let script = r#"
        import YAML from '@root@yaml';
        import fromAsync from '@root@from-async';
        import { Base64 } from '@root@base64';
        import { isEqual } from '@root@toolkit';
        import { text } from '@root@workspace';
        import number from '@root@javascript';
        import data from '@root@json';
        if (isEqual(1, 2)) throw new Error('Wrong isEqual implementation');
        if (number !== 42 || data.key !== 42) throw new Error('Wrong MIME parsing');
        const object = YAML.parse(`
            object:
                array: ["hello", "world"]
                key: "value"
        `).object;
        let result = await fromAsync([
            Promise.resolve(Base64.encode(object.array[0])),
            Promise.resolve(Base64.encode(object.array[1])),
        ]);
        result.push(JSON.stringify(YAML.parse(text)));
        export default result;
    "#
    .replace("@root@", fixture.base.as_str());
    let module = Module::parse(Source::from_bytes(&script), None, &mut context).unwrap();
    let promise = module.load_link_evaluate(&mut context);
    context.run_jobs().unwrap();
    match promise.state() {
        PromiseState::Fulfilled(_) => {}
        PromiseState::Rejected(error) => panic!("{}", error.display()),
        PromiseState::Pending => panic!("module did not finish"),
    }
    let result = module
        .namespace(&mut context)
        .get(js_string!("default"), &mut context)
        .unwrap()
        .to_json(&mut context)
        .unwrap()
        .unwrap();
    assert_eq!(
        result,
        serde_json::json!(["aGVsbG8=", "d29ybGQ=", "{\"packages\":[\"frontend/*\"]}"])
    );
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 7);
}
