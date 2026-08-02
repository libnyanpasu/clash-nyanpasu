use std::sync::Arc;

use camino::Utf8PathBuf;
use nyanpasu_config::application::ClashCore;

use crate::{
    client::{ApplicationClient, CoreClient},
    utils::path::PathResolver,
};

use super::types::CoreRequest;

pub trait CoreBinaryResolver: Send + Sync + 'static {
    fn resolve(&self, kind: &nyanpasu_utils::core::CoreType) -> anyhow::Result<Utf8PathBuf>;
}

#[derive(Clone)]
pub(crate) struct CoreRequestFactory {
    data_dir: Utf8PathBuf,
    pid_path: Utf8PathBuf,
    runtime_paths: crate::client::RuntimePaths,
    binary: Arc<dyn CoreBinaryResolver>,
}

impl CoreRequestFactory {
    pub(crate) fn new(
        paths: &PathResolver,
        runtime_paths: crate::client::RuntimePaths,
        binary: Arc<dyn CoreBinaryResolver>,
    ) -> anyhow::Result<Self> {
        let data_dir = Utf8PathBuf::from_path_buf(paths.app_data_dir().to_path_buf())
            .map_err(|path| anyhow::anyhow!("data directory is not UTF-8: {path:?}"))?;
        let pid_path = Utf8PathBuf::from_path_buf(paths.clash_pid_path())
            .map_err(|path| anyhow::anyhow!("pid path is not UTF-8: {path:?}"))?;
        Ok(Self {
            data_dir,
            pid_path,
            runtime_paths,
            binary,
        })
    }

    pub(crate) fn for_product(
        &self,
        core: ClashCore,
    ) -> Result<CoreRequest, super::backend::CoreBackendError> {
        let core_type = (&core).into();
        let binary_path = self
            .binary
            .resolve(&core_type)
            .map_err(super::backend::CoreBackendError::Binary)?;
        Ok(CoreRequest {
            core_type,
            binary_path,
            config_path: self.runtime_paths.product().to_owned(),
            working_dir: self.data_dir.clone(),
            pid_path: Some(self.pid_path.clone()),
        })
    }

    pub(crate) fn manager_runtime_dir(&self) -> Utf8PathBuf {
        self.runtime_paths.candidate_dir().join(".core-manager")
    }

    pub(crate) fn runtime_paths(&self) -> &crate::client::RuntimePaths {
        &self.runtime_paths
    }
}

#[derive(Clone)]
pub(crate) struct CoreModeReconciler {
    pub(crate) core: CoreClient,
    pub(crate) application: ApplicationClient,
    pub(crate) requests: CoreRequestFactory,
}

impl CoreModeReconciler {
    pub(crate) async fn reconcile(
        &self,
        ipc_state: crate::core::service::ipc::IpcState,
    ) -> anyhow::Result<()> {
        let app = self.application.get().await?.state;
        if !app.enable_service_mode {
            return Ok(());
        }
        let mode = crate::core::RunType::classify(true, ipc_state);
        let operation = self.core.begin_operation().await?;
        self.core.set_backend(&operation, mode).await?;
        let request = self.requests.for_product(app.core)?;
        self.core.run(&operation, &request).await?;
        Ok(())
    }
}
