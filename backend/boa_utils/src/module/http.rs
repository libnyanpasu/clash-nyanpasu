use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    str::FromStr,
    time::{Duration, SystemTime},
};

use async_fs::create_dir_all;
use boa_engine::{Context, JsNativeError, JsResult, JsString, Module, module::ModuleLoader};
use boa_parser::Source;
use mime::Mime;
// Tokio sync is not runtime related
use tokio::sync::oneshot::channel as oneshot_channel;
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

    #[tracing::instrument(skip(context))]
    fn handle_cached_item(item: CachedItem, context: &mut Context) -> JsResult<Module> {
        let mime = Mime::from_str(item.mime.as_str()).map_err(|_| {
            log::error!("failed to parse mime type `{}`", item.mime);
            JsNativeError::typ().with_message("failed to parse mime type")
        })?;

        let source_str = match (mime.type_(), mime.subtype()) {
            (mime::APPLICATION, mime::JAVASCRIPT) => item.content.clone(),
            (mime::APPLICATION, mime::JSON) => {
                format!("export default {};", item.content)
            }
            _ => {
                let escaped_str = serde_json::to_string(&item.content).map_err(|_| {
                    log::error!("failed to serialize content.");
                    JsNativeError::typ().with_message("failed to serialize content")
                })?;
                format!("export const text = {escaped_str};")
            }
        };

        // Could also add a path if needed.
        let source = Source::from_bytes(source_str.as_bytes());

        Module::parse(source, None, context)
    }
}

impl ModuleLoader for HttpModuleLoader {
    async fn load_imported_module(
        self: Rc<Self>,
        _referrer: boa_engine::module::Referrer,
        specifier: JsString,
        context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let url = specifier.to_std_string_escaped();
        let url = Url::from_str(&url).expect("invalid url"); // SAFETY: `url` is a valid URL, if it's not, its caller side issue
        let cache_path = self.mapping_cache_dir(&url);
        let parent_dir = cache_path
            .parent()
            .ok_or_else(|| {
                log::error!("failed to get parent directory for `{url}`");
                JsNativeError::typ().with_message(format!(
                    "failed to get cache parent directory for `{url}`; path: `{}`",
                    cache_path.display()
                ))
            })?
            .to_path_buf();

        let max_age = self.max_age;

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
                    && let Err(err) = create_dir_all(parent_dir).await
                {
                    log::error!(
                        "failed to create cache directory for `{url}`; path: `{}`. error: `{}`",
                        cache_path.display(),
                        err
                    );
                }
                false
            }
            _ => false,
        };

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
            log::debug!("fetching `{url}`...");
            let (tx, rx) = oneshot_channel();
            let fetcher_url = url.clone();
            nyanpasu_utils::runtime::spawn(async move {
                let result = async {
                    let response = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::limited(5))
                        .build()?
                        .get(fetcher_url.as_str())
                        .send()
                        .await?;

                    let mime = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .map(|v| v.to_string())
                        .unwrap_or(mime::TEXT_PLAIN.to_string());
                    let body = response.text().await?;

                    log::debug!("finished fetching `{fetcher_url}`");
                    Ok(CachedItem {
                        mime,
                        content: body,
                    })
                }
                .await;
                let _ = tx.send(result);
            });
            rx.await.expect("should never drop oneshot tx")
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

        let item = item.map_err(|err| JsNativeError::typ().with_message(err.to_string()))?;
        Self::handle_cached_item(item, &mut context.borrow_mut())
    }
}

#[test]
fn test_http_module_loader() -> JsResult<()> {
    use boa_engine::{builtins::promise::PromiseState, job::SimpleJobExecutor, js_string};
    use std::rc::Rc;
    let temp_dir = tempfile::tempdir().unwrap();
    // A simple snippet that imports modules from the web instead of the file system.
    const SRC: &str = r#"
        import YAML from 'https://esm.run/yaml@2.3.4';
        import fromAsync from 'https://esm.run/array-from-async@3.0.0';
        import { Base64 } from 'https://esm.run/js-base64@3.7.6';
        // Test toolkit
        import { isEqual } from 'https://esm.run/es-toolkit@1.39.10';
        import { text } from 'https://github.com/libnyanpasu/clash-nyanpasu/raw/refs/heads/main/pnpm-workspace.yaml';

        if (isEqual(1, 2)) {
            throw new Error('Wrong isEqual implementation');
        }

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

        const parsed = YAML.parse(text);
        result.push(JSON.stringify(parsed));

        export default result;
    "#;

    let queue = Rc::new(SimpleJobExecutor::new());
    let mut context = Context::builder()
        .job_executor(queue)
        // NEW: sets the context module loader to our custom loader
        .module_loader(Rc::new(HttpModuleLoader::new(
            temp_dir.path().to_path_buf(),
            Duration::from_secs(10),
        )))
        .build()?;

    let module = Module::parse(Source::from_bytes(SRC.as_bytes()), None, &mut context)?;

    // Calling `Module::load_link_evaluate` takes care of having to define promise handlers for
    // `Module::load` and `Module::evaluate`.
    let promise = module.load_link_evaluate(&mut context);

    // Important to call `Context::run_jobs`, or else all the futures and promises won't be
    // pushed forward by the job queue.
    let _ = context.run_jobs();

    match promise.state() {
        // Our job queue guarantees that all promises and futures are finished after returning
        // from `Context::run_jobs`.
        // Some other job queue designs only execute a "microtick" or a single pass through the
        // pending promises and futures. In that case, you can pass this logic as a promise handler
        // for `promise` instead.
        PromiseState::Pending => panic!("module didn't execute!"),
        // All modules after successfully evaluating return `JsValue::undefined()`.
        PromiseState::Fulfilled(v) => {
            assert_eq!(v, boa_engine::JsValue::undefined())
        }
        PromiseState::Rejected(err) => {
            panic!("{}", err.display());
        }
    }

    let default = module
        .namespace(&mut context)
        .get(js_string!("default"), &mut context)?;

    // `default` should contain the result of our calculations.
    let default = default
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("default export was not an object"))?;

    assert_eq!(
        default
            .get(0, &mut context)?
            .as_string()
            .ok_or_else(|| JsNativeError::typ().with_message("array element was not a string"))?,
        js_string!("aGVsbG8=")
    );
    assert_eq!(
        default
            .get(1, &mut context)?
            .as_string()
            .ok_or_else(|| JsNativeError::typ().with_message("array element was not a string"))?,
        js_string!("d29ybGQ=")
    );
    assert!(
        default
            .get(2, &mut context)?
            .as_string()
            .ok_or_else(|| JsNativeError::typ().with_message("array element was not a string"))?
            .to_std_string_escaped()
            .contains("packages"),
        "YAML content should contain 'packages' field"
    );

    Ok(())
}
