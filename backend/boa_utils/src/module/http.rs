//! Boa module parsing over a host-supplied HTTP/cache capability.
use std::{cell::RefCell, rc::Rc, str::FromStr, sync::Arc};

use boa_engine::{
    Context, JsNativeError, JsResult, Module,
    module::{ModuleLoader, ModuleRequest},
};
use boa_parser::Source;
use mime::Mime;
use snafu::Snafu;
use url::Url;

/// The transport/cache failure retains its original display and error chain.
#[derive(Debug, Snafu)]
pub enum HttpModuleError {
    /// Loading source bytes failed before Boa parsed the module.
    #[snafu(display("{source}"))]
    LoadModule {
        /// Original transport or cache failure; never flattened into a string.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Supplies module contents, including the host's cache and clock policy.
#[async_trait::async_trait]
pub trait HttpModuleSource: Send + Sync {
    /// Loads a URL, preserving transport/cache failures as their original sources.
    ///
    /// # Errors
    /// Returns the original failure wrapped in [`HttpModuleError::LoadModule`].
    async fn load(&self, url: &Url) -> Result<CachedItem, HttpModuleError>;
}

/// Parses source supplied by the host on the thread owning the Boa context.
pub struct HttpModuleLoader {
    source: Arc<dyn HttpModuleSource>,
}

/// The persisted HTTP module payload; its encoding remains host-owned.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedItem {
    /// Response MIME type, including the host's fallback when absent.
    pub mime: String,
    /// Raw text; binary module contents are not supported.
    pub content: String,
}

impl HttpModuleLoader {
    /// Uses only the supplied source for network and cache access.
    pub fn new(source: Arc<dyn HttpModuleSource>) -> Self {
        Self { source }
    }

    #[tracing::instrument(skip(item, context), fields(mime = %item.mime, content = %nyanpasu_log_preview::text(&item.content)))]
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
        request: ModuleRequest,
        context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let url = request.specifier().to_std_string_escaped();
        let url = Url::from_str(&url).expect("invalid url"); // The combining loader validates URLs.
        let item = self
            .source
            .load(&url)
            .await
            .map_err(|err| JsNativeError::typ().with_message(err.to_string()))?;
        Self::handle_cached_item(item, &mut context.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error as _, io};

    use boa_engine::{Source, builtins::promise::PromiseState, job::SimpleJobExecutor};

    use super::*;

    struct FailingSource;

    #[async_trait::async_trait]
    impl HttpModuleSource for FailingSource {
        async fn load(&self, _: &Url) -> Result<CachedItem, HttpModuleError> {
            Err(HttpModuleError::LoadModule {
                source: io::Error::other("controlled module failure").into(),
            })
        }
    }

    #[test]
    fn module_load_error_preserves_display_and_source_chain() {
        let error = smol::block_on(
            FailingSource.load(&Url::parse("https://fixture.invalid/module").unwrap()),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "controlled module failure");
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .kind(),
            io::ErrorKind::Other
        );
        let mut context = Context::builder()
            .job_executor(Rc::new(SimpleJobExecutor::new()))
            .module_loader(Rc::new(HttpModuleLoader::new(Arc::new(FailingSource))))
            .build()
            .unwrap();
        let module = Module::parse(
            Source::from_bytes("import 'https://fixture.invalid/module';"),
            None,
            &mut context,
        )
        .unwrap();
        let promise = module.load_link_evaluate(&mut context);
        context.run_jobs().unwrap();
        let PromiseState::Rejected(error) = promise.state() else {
            panic!("source failure must reject the import")
        };
        assert_eq!(
            error.display().to_string(),
            "TypeError: controlled module failure (native)"
        );
    }
}
