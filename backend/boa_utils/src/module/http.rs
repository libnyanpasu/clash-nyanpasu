use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    path::PathBuf,
    str::FromStr,
    time::{Duration, SystemTime},
};

use async_fs::create_dir_all;
use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, Module,
    job::{FutureJob, JobQueue, NativeJob},
    module::ModuleLoader,
};
use boa_parser::Source;
use futures_util::{StreamExt, stream::FuturesUnordered};
use isahc::{
    AsyncReadResponseExt, Request, RequestExt,
    config::{Configurable, RedirectPolicy},
};
use mime::Mime;
use smol::{LocalExecutor, future};
use url::Url;

// Most of the boilerplate is taken from the `futures.rs` example.
// This file only explains what is exclusive of async module loading.

#[derive(Debug, Default)]
pub struct HttpModuleLoader {
    cache_dir: PathBuf,
    max_age: Duration,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedItem {
    pub mime: String,
    /// raw string content
    /// We have no plan for now to support binary content,
    /// so we just use `String` to store the content.
    pub content: String,
}

impl HttpModuleLoader {
    pub fn new(cache_dir: PathBuf, max_age: Duration) -> Self {
        Self { cache_dir, max_age }
    }

    fn mapping_cache_dir(&self, url: &url::Url) -> PathBuf {
        let mut buf = self.cache_dir.clone();
        let host = match url.host() {
            Some(host) => host.to_string().replace('.', "--"),
            None => "unknown".to_string(),
        };
        let port = match url.port() {
            Some(port) => format!("__{port}"),
            None => "".to_string(),
        };
        buf.push(format!("{}_{}{}", url.scheme(), host, port));
        buf.push(url.path().replace('/', "_").replace(".", "--"));
        buf
    }

    #[tracing::instrument(skip(finish_load, context))]
    fn handle_cached_item(
        item: CachedItem,
        finish_load: Box<dyn FnOnce(JsResult<Module>, &mut Context)>,
        context: &mut Context,
    ) {
        let Ok(mime) = Mime::from_str(item.mime.as_str()) else {
            log::error!("failed to parse mime type `{}`", item.mime);
            finish_load(
                Err(JsNativeError::typ()
                    .with_message("failed to parse mime type")
                    .into()),
                context,
            );
            return;
        };
        let source_str = match (mime.type_(), mime.subtype()) {
            (mime::APPLICATION, mime::JAVASCRIPT) => item.content.clone(),
            (mime::APPLICATION, mime::JSON) => {
                format!("export default {};", item.content)
            }
            _ => {
                let Ok(escaped_str) = serde_json::to_string(&item.content) else {
                    log::error!("failed to serialize content.");
                    finish_load(
                        Err(JsNativeError::typ()
                            .with_message("failed to serialize content")
                            .into()),
                        context,
                    );
                    return;
                };
                format!("export const text = {escaped_str};")
            }
        };

        // Could also add a path if needed.
        let source = Source::from_bytes(source_str.as_bytes());

        let module = Module::parse(source, None, context);
        // TODO: rm cache or create cache after judge module is ok

        // We don't do any error handling, `finish_load` takes care of that for us.
        finish_load(module, context);
    }
}

impl ModuleLoader for HttpModuleLoader {
    fn load_imported_module(
        &self,
        _referrer: boa_engine::module::Referrer,
        specifier: JsString,
        finish_load: Box<dyn FnOnce(JsResult<Module>, &mut Context)>,
        context: &mut Context,
    ) {
        let url = specifier.to_std_string_escaped();
        let url = Url::from_str(&url).expect("invalid url"); // SAFETY: `url` is a valid URL, if it's not, its caller side issue
        let cache_path = self.mapping_cache_dir(&url);
        let parent_dir = match cache_path.parent() {
            Some(parent) => parent.to_path_buf(),
            None => {
                log::error!("failed to get parent directory for `{url}`");
                finish_load(
                    Err(JsNativeError::typ()
                        .with_message(format!(
                            "failed to get cache parent directory for `{url}`; path: `{}`",
                            cache_path.display()
                        ))
                        .into()),
                    context,
                );
                return;
            }
        };
        let max_age = self.max_age;

        let fetch = async move {
            log::debug!("checking cache for `{url}`...");

            let now = SystemTime::now();
            let should_use_cached_content = match async_fs::metadata(&cache_path).await {
                Ok(metadata)
                    if metadata
                        .modified()
                        .is_ok_and(|modified| modified > now - max_age) =>
                {
                    true
                }
                Err(err) => {
                    // create dir if not exists
                    if err.kind() == std::io::ErrorKind::NotFound
                        && let Err(e) = async_fs::metadata(&parent_dir).await
                        && e.kind() == std::io::ErrorKind::NotFound
                    {
                        if let Err(err) = create_dir_all(parent_dir).await {
                            log::error!(
                                "failed to create cache directory for `{url}`; path: `{}`. error: `{}`",
                                cache_path.display(),
                                err
                            );
                        }
                    }
                    false
                }
                _ => false,
            };

            // Adding some prints to show the non-deterministic nature of the async fetches.
            // Try to run the example several times to see how sometimes the fetches start in order
            // but finish in disorder.

            // This could also retry fetching in case there's an error while requesting the module.
            let item: anyhow::Result<CachedItem> = if should_use_cached_content {
                async {
                    log::debug!("fetching `{url}` from cache...");
                    let item = async_fs::read(&cache_path).await?;
                    let item = postcard::from_bytes(&item)?;
                    log::debug!("finished fetching `{url}` from cache");
                    Ok(item)
                }
                .await
            } else {
                async {
                    log::debug!("fetching `{url}`...");
                    let mut response = Request::get(url.as_str())
                        .redirect_policy(RedirectPolicy::Limit(5))
                        .body(())?
                        .send_async()
                        .await?;

                    let mime = response
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .map(|v| v.to_string())
                        .unwrap_or(mime::TEXT_PLAIN.to_string());
                    let body = response.text().await?;

                    log::debug!("finished fetching `{url}`");
                    Ok(CachedItem {
                        mime,
                        content: body,
                    })
                }
                .await
            };

            if let Ok(item) = &item {
                match postcard::to_stdvec(&item) {
                    Ok(item) => {
                        if let Err(err) = async_fs::write(&cache_path, &item).await {
                            log::error!(
                                "failed to write cache for `{url}`; path: `{}`. error: `{}`",
                                cache_path.display(),
                                err
                            );
                        }
                    }
                    Err(err) => {
                        log::error!("failed to serialize content: {err}");
                    }
                }
            }

            // Since the async context cannot take the `context` by ref, we have to continue
            // parsing inside a new `NativeJob` that will be enqueued into the promise job queue.
            NativeJob::new(move |context| -> JsResult<JsValue> {
                let item = match item {
                    Ok(item) => item,
                    Err(err) => {
                        // On error we always call `finish_load` to notify the load promise about the
                        // error.
                        finish_load(
                            Err(JsNativeError::typ().with_message(err.to_string()).into()),
                            context,
                        );

                        // Just returns anything to comply with `NativeJob::new`'s signature.
                        return Ok(JsValue::undefined());
                    }
                };
                Self::handle_cached_item(item, finish_load, context);
                // Also needed to match `NativeJob::new`.
                Ok(JsValue::undefined())
            })
        };

        // Just enqueue the future for now. We'll advance all the enqueued futures inside our custom
        // `JobQueue`.
        context
            .job_queue()
            .enqueue_future_job(Box::pin(fetch), context)
    }
}

#[test]
fn test_http_module_loader() -> JsResult<()> {
    use boa_engine::{builtins::promise::PromiseState, js_string};
    use std::rc::Rc;
    let temp_dir = tempfile::tempdir().unwrap();
    // A simple snippet that imports modules from the web instead of the file system.
    const SRC: &str = r#"
        import YAML from 'https://esm.run/yaml@2.3.4';
        import fromAsync from 'https://esm.run/array-from-async@3.0.0';
        import { Base64 } from 'https://esm.run/js-base64@3.7.6';

        const data = `
            object:
                array: ["hello", "world"]
                key: "value"
        `;

        const object = YAML.parse(data).object;

        let result = await fromAsync([
            Promise.resolve(Base64.encode(object.array[0])),
            Promise.resolve(Base64.encode(object.array[1])),
        ]);

        export default result;
    "#;

    let queue = Rc::new(Queue::new(LocalExecutor::new()));
    let context = &mut Context::builder()
        .job_queue(queue)
        // NEW: sets the context module loader to our custom loader
        .module_loader(Rc::new(HttpModuleLoader::new(
            temp_dir.path().to_path_buf(),
            Duration::from_secs(10),
        )))
        .build()?;

    let module = Module::parse(Source::from_bytes(SRC.as_bytes()), None, context)?;

    // Calling `Module::load_link_evaluate` takes care of having to define promise handlers for
    // `Module::load` and `Module::evaluate`.
    let promise = module.load_link_evaluate(context);

    // Important to call `Context::run_jobs`, or else all the futures and promises won't be
    // pushed forward by the job queue.
    context.run_jobs();

    match promise.state() {
        // Our job queue guarantees that all promises and futures are finished after returning
        // from `Context::run_jobs`.
        // Some other job queue designs only execute a "microtick" or a single pass through the
        // pending promises and futures. In that case, you can pass this logic as a promise handler
        // for `promise` instead.
        PromiseState::Pending => panic!("module didn't execute!"),
        // All modules after successfully evaluating return `JsValue::undefined()`.
        PromiseState::Fulfilled(v) => {
            assert_eq!(v, JsValue::undefined())
        }
        PromiseState::Rejected(err) => {
            panic!("{}", err.display());
        }
    }

    let default = module
        .namespace(context)
        .get(js_string!("default"), context)?;

    // `default` should contain the result of our calculations.
    let default = default
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("default export was not an object"))?;

    assert_eq!(
        default
            .get(0, context)?
            .as_string()
            .ok_or_else(|| JsNativeError::typ().with_message("array element was not a string"))?,
        &js_string!("aGVsbG8=")
    );
    assert_eq!(
        default
            .get(1, context)?
            .as_string()
            .ok_or_else(|| JsNativeError::typ().with_message("array element was not a string"))?,
        &js_string!("d29ybGQ=")
    );

    Ok(())
}

// Taken from the `futures.rs` example.
pub struct Queue<'a> {
    executor: LocalExecutor<'a>,
    futures: RefCell<FuturesUnordered<FutureJob>>,
    jobs: RefCell<VecDeque<NativeJob>>,
}

impl Default for Queue<'_> {
    fn default() -> Self {
        Self::new(LocalExecutor::new())
    }
}

impl<'a> Queue<'a> {
    fn new(executor: LocalExecutor<'a>) -> Self {
        Self {
            executor,
            futures: RefCell::default(),
            jobs: RefCell::default(),
        }
    }
}

impl JobQueue for Queue<'_> {
    fn enqueue_promise_job(&self, job: NativeJob, _context: &mut Context) {
        self.jobs.borrow_mut().push_back(job);
    }

    fn enqueue_future_job(&self, future: FutureJob, _context: &mut Context) {
        self.futures.borrow().push(future)
    }

    fn run_jobs(&self, context: &mut Context) {
        // Early return in case there were no jobs scheduled.
        if self.jobs.borrow().is_empty() && self.futures.borrow().is_empty() {
            return;
        }

        let context = RefCell::new(context);

        future::block_on(self.executor.run(async move {
            // Used to sync the finalization of both tasks
            let finished = Cell::new(0b00u8);

            let fqueue = async {
                loop {
                    if self.futures.borrow().is_empty() {
                        finished.set(finished.get() | 0b01);
                        if finished.get() >= 0b11 {
                            // All possible futures and jobs were completed. Exit.
                            return;
                        }
                        // All possible jobs were completed, but `jqueue` could have
                        // pending jobs. Yield to the executor to try to progress on
                        // `jqueue` until we have more pending futures.
                        future::yield_now().await;
                        continue;
                    }
                    finished.set(finished.get() & 0b10);

                    let futures = &mut std::mem::take(&mut *self.futures.borrow_mut());
                    while let Some(job) = futures.next().await {
                        self.enqueue_promise_job(job, &mut context.borrow_mut());
                    }
                }
            };

            let jqueue = async {
                loop {
                    if self.jobs.borrow().is_empty() {
                        finished.set(finished.get() | 0b10);
                        if finished.get() >= 0b11 {
                            // All possible futures and jobs were completed. Exit.
                            return;
                        }
                        // All possible jobs were completed, but `fqueue` could have
                        // pending futures. Yield to the executor to try to progress on
                        // `fqueue` until we have more pending jobs.
                        future::yield_now().await;
                        continue;
                    };
                    finished.set(finished.get() & 0b01);

                    let jobs = std::mem::take(&mut *self.jobs.borrow_mut());
                    for job in jobs {
                        if let Err(e) = job.call(&mut context.borrow_mut()) {
                            eprintln!("Uncaught {e}");
                        }
                        future::yield_now().await;
                    }
                }
            };

            future::zip(fqueue, jqueue).await;
        }))
    }
}
