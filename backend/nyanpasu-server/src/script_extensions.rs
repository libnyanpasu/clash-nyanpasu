//! Server-owned script IO and explicit JavaScript/Lua extensions.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, SystemTime},
};

use boa_engine::{JsResult, module::SimpleModuleLoader};
use boa_utils::module::{
    builtin::BuiltinModuleLoader,
    combine::CombineModuleLoader,
    http::{CachedItem, HttpModuleError, HttpModuleLoader, HttpModuleSource},
};
use nyanpasu_scripts::{
    ConsoleSink,
    extensions::{JavaScriptExtensions, LuaExtensions, ScriptExtensionFactory},
};

pub(crate) struct ServerScriptExtensions {
    source: Arc<dyn HttpModuleSource>,
}

impl ServerScriptExtensions {
    pub(crate) fn new(source: Arc<dyn HttpModuleSource>) -> Self {
        Self { source }
    }
}

impl ScriptExtensionFactory for ServerScriptExtensions {
    fn javascript(&self, scripts_dir: &Path) -> JsResult<Option<JavaScriptExtensions>> {
        Ok(Some(JavaScriptExtensions::new(
            Rc::new(CombineModuleLoader::new(
                SimpleModuleLoader::new(scripts_dir)?,
                HttpModuleLoader::new(self.source.clone()),
                BuiltinModuleLoader,
            )),
            ConsoleSink::register_javascript,
        )))
    }

    fn lua(&self) -> Option<LuaExtensions> {
        Some(LuaExtensions::new(
            mlua::StdLib::ALL_SAFE,
            ConsoleSink::register_lua,
        ))
    }
}

pub(crate) struct HttpSource {
    client: reqwest::Client,
    cache: PathBuf,
    runtime: tokio::runtime::Handle,
}

impl HttpSource {
    pub(crate) fn new(cache: PathBuf, runtime: tokio::runtime::Handle) -> reqwest::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()?,
            cache,
            runtime,
        })
    }
}

#[async_trait::async_trait]
impl HttpModuleSource for HttpSource {
    async fn load(&self, url: &url::Url) -> Result<CachedItem, HttpModuleError> {
        let url = url.clone();
        let cache = self.cache.clone();
        let client = self.client.clone();
        let task = self.runtime.spawn(async move {
            let host = url.host_str().unwrap_or("unknown").replace('.', "--");
            let port = url
                .port()
                .map(|port| format!("__{port}"))
                .unwrap_or_default();
            let directory = cache.join(format!("{}_{}{}", url.scheme(), host, port));
            let path = directory.join(url.path().replace('/', "_").replace('.', "--"));
            let fresh = tokio::fs::metadata(&path)
                .await
                .ok()
                .and_then(|meta| meta.modified().ok())
                .is_some_and(|modified| {
                    modified > SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 30)
                });
            let item = if fresh {
                postcard::from_bytes(&tokio::fs::read(&path).await?)?
            } else {
                let response = client.get(url).send().await?;
                let mime = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("text/plain")
                    .to_owned();
                CachedItem {
                    mime,
                    content: response.text().await?,
                }
            };
            if let Err(error) = async {
                tokio::fs::create_dir_all(directory).await?;
                tokio::fs::write(path, postcard::to_stdvec(&item)?).await?;
                Ok::<_, anyhow::Error>(())
            }
            .await
            {
                tracing::warn!(%error, "script module cache write failed");
            }
            Ok::<_, anyhow::Error>(item)
        });
        match task.await {
            Ok(result) => result.map_err(|source| HttpModuleError::LoadModule {
                source: source.into(),
            }),
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(error) => Err(HttpModuleError::LoadModule {
                source: anyhow::Error::from(error).into(),
            }),
        }
    }
}
