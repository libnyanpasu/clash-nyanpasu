use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, Module,
    builtins::promise::PromiseState,
    job::{FutureJob, JobQueue, NativeJob},
    js_string,
    module::ModuleLoader,
};
use boa_parser::Source;
use futures_util::{StreamExt, stream::FuturesUnordered};
use isahc::{
    AsyncReadResponseExt, Request, RequestExt,
    config::{Configurable, RedirectPolicy},
};
use smol::{LocalExecutor, future};

// Most of the boilerplate is taken from the `futures.rs` example.
// This file only explains what is exclusive of async module loading.

#[derive(Debug, Default)]
pub struct HttpModuleLoader;

impl ModuleLoader for HttpModuleLoader {
    fn load_imported_module(
        &self,
        _referrer: boa_engine::module::Referrer,
        specifier: JsString,
        finish_load: Box<dyn FnOnce(JsResult<Module>, &mut Context)>,
        context: &mut Context,
    ) {
        let url = specifier.to_std_string_escaped();

        let fetch = async move {
            // Adding some prints to show the non-deterministic nature of the async fetches.
            // Try to run the example several times to see how sometimes the fetches start in order
            // but finish in disorder.
            log::debug!("fetching `{url}`...");
            // This could also retry fetching in case there's an error while requesting the module.
            let body: Result<_, isahc::Error> = async {
                let mut response = Request::get(&url)
                    .redirect_policy(RedirectPolicy::Limit(5))
                    .body(())?
                    .send_async()
                    .await?;

                Ok(response.text().await?)
            }
            .await;
            log::debug!("finished fetching `{url}`");

            // Since the async context cannot take the `context` by ref, we have to continue
            // parsing inside a new `NativeJob` that will be enqueued into the promise job queue.
            NativeJob::new(move |context| -> JsResult<JsValue> {
                let body = match body {
                    Ok(body) => body,
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

                // Could also add a path if needed.
                let source = Source::from_bytes(body.as_bytes());

                let module = Module::parse(source, None, context);

                // We don't do any error handling, `finish_load` takes care of that for us.
                finish_load(module, context);

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
        .module_loader(Rc::new(HttpModuleLoader))
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
