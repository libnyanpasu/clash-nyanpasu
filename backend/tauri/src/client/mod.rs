mod error;
mod event_sink;

use crate::{
    config::{Config, IVerge, Profiles, ProfilesBuilder},
    core::{CoreManager, RunType},
};
use nyanpasu_ipc::api::status::CoreState;
use std::{borrow::Cow, sync::Arc};

pub use error::{ClientError, Result};
pub use event_sink::{TauriUiEventSink, UiEventSink};

#[derive(Clone)]
pub struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

struct NyanpasuClientInner {
    ui: Arc<dyn UiEventSink>,
}

impl NyanpasuClient {
    pub fn new(ui: Arc<dyn UiEventSink>) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner { ui }),
        }
    }

    pub fn get_profiles(&self) -> Profiles {
        Config::profiles().data().clone()
    }

    pub async fn patch_profiles_config(&self, profiles: ProfilesBuilder) -> Result<()> {
        Config::profiles().draft().apply(profiles);

        match CoreManager::global().update_config().await {
            Ok(_) => {
                self.inner.ui.refresh_clash();
                Config::profiles().apply();
                Config::profiles().data().save_file()?;

                let _ = crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change().await;

                Ok(())
            }
            Err(err) => {
                Config::profiles().discard();
                log::error!(target: "app", "{err:?}");
                Err(err.into())
            }
        }
    }

    pub fn get_verge_config(&self) -> IVerge {
        Config::verge().data().clone()
    }

    pub async fn patch_verge_config(&self, payload: IVerge) -> Result<()> {
        crate::feat::patch_verge(payload).await?;
        Ok(())
    }

    pub async fn get_core_status(&self) -> (Cow<'static, CoreState>, i64, RunType) {
        CoreManager::global().status().await
    }
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(manager: &M) -> anyhow::Result<()> {
    let sink: Arc<dyn UiEventSink> = Arc::new(TauriUiEventSink::new(manager.app_handle().clone()));
    manager.manage(NyanpasuClient::new(sink));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::event_sink::NoopUiEventSink, ipc::IpcError};

    #[test]
    fn client_constructs_without_tauri_runtime() {
        let client = NyanpasuClient::new(Arc::new(NoopUiEventSink));
        let _ = client.clone();
    }

    #[test]
    fn client_error_bridges_to_ipc_error() {
        assert!(matches!(
            IpcError::from(ClientError::Custom("boom".into())),
            IpcError::Custom(msg) if msg == "boom"
        ));
        assert!(matches!(
            IpcError::from(ClientError::Io(std::io::Error::other("io"))),
            IpcError::Io(_)
        ));
        assert!(matches!(
            IpcError::from(ClientError::Anyhow(anyhow::anyhow!("oops"))),
            IpcError::Anyhow(_)
        ));
    }
}
