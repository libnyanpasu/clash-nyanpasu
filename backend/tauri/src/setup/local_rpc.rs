//! Explicit desktop local-listener composition, drained with the root owners.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use nyanpasu_rpc::{
    local::{
        endpoint::{Endpoint, EndpointError, EndpointListener},
        server::{ConnectionLimits, LocalServer},
    },
    registry::UnifiedRpc,
};

use super::StartupOwners;

#[derive(Debug, Default)]
pub(super) struct LocalRpcConfig {
    enabled: bool,
    endpoint: Option<PathBuf>,
}

impl LocalRpcConfig {
    pub(super) fn from_environment() -> Self {
        Self::from_values(
            std::env::var_os("NYANPASU_LOCAL_RPC").as_deref(),
            std::env::var_os("NYANPASU_LOCAL_RPC_ENDPOINT").map(PathBuf::from),
        )
    }

    fn from_values(enable: Option<&OsStr>, endpoint: Option<PathBuf>) -> Self {
        Self {
            enabled: enable == Some(OsStr::new("1")),
            endpoint,
        }
    }
}

pub(super) async fn start(
    config: LocalRpcConfig,
    data_directory: &Path,
    rpc: UnifiedRpc,
    owners: &StartupOwners,
) -> Result<Option<Endpoint>, EndpointError> {
    if !config.enabled {
        return Ok(None);
    }
    let directory = data_directory.to_owned();
    let endpoint = nyanpasu_owner_support::blocking::join(
        tokio::task::spawn_blocking(move || {
            Endpoint::discover(&directory, config.endpoint.as_deref())
        })
        .await,
    )?;
    let listener = EndpointListener::bind(&endpoint).await?;
    let shutdown = owners.shutdown.child_token();
    let root = owners.shutdown.clone();
    // Do not use track_until_shutdown: cancellation must drain accepted work,
    // not drop the server future before connection/resource cleanup finishes.
    let task = owners.tasks.spawn(async move {
        if let Err(error) = LocalServer::new(rpc, ConnectionLimits::default())
            .serve(listener, shutdown)
            .await
        {
            tracing::error!(%error, "desktop local RPC listener failed");
            root.cancel();
        }
    });
    *owners
        .local_rpc
        .lock()
        .expect("listener join ownership is never panicked under lock") = Some(task);
    Ok(Some(endpoint))
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, time::Duration};

    use nyanpasu_rpc::test_support::Fixture;
    use nyanpasu_rpc_protocol::{client::LocalClient, frame::Reply};
    use serde_json::json;

    use super::{LocalRpcConfig, start};
    use crate::setup::StartupOwners;

    #[test]
    fn local_listener_enable_requires_exactly_one() {
        for enable in [
            None,
            Some("0"),
            Some("true"),
            Some(""),
            Some(" 1"),
            Some("1 "),
        ] {
            let config = LocalRpcConfig::from_values(enable.map(OsStr::new), None);
            assert!(
                !config.enabled,
                "listener must stay disabled for {enable:?}"
            );
        }
        assert!(LocalRpcConfig::from_values(Some(OsStr::new("1")), None).enabled);
    }

    #[tokio::test]
    async fn desktop_local_listener_is_opt_in_and_joins_shutdown() {
        let fixture = Fixture::new(false).await;
        let directory = tempfile::tempdir().unwrap();
        let owners = StartupOwners::default();
        let config = LocalRpcConfig::from_values(None, None);
        assert!(!config.enabled);
        assert!(
            start(config, directory.path(), fixture.rpc.clone(), &owners)
                .await
                .unwrap()
                .is_none()
        );
        assert!(owners.tasks.is_empty());
        let endpoint = start(
            LocalRpcConfig::from_values(Some(OsStr::new("1")), None),
            directory.path(),
            fixture.rpc.clone(),
            &owners,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!owners.tasks.is_empty());
        let client = LocalClient::connect(endpoint.path(), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(matches!(
            client.call("get_profiles", json!(null)).await.unwrap(),
            Reply::Result(_)
        ));
        owners.request_shutdown();
        tokio::time::timeout(Duration::from_secs(10), owners.wait_shutdown())
            .await
            .expect("local connections join root shutdown");
        assert!(owners.tasks.is_empty());
        assert!(client.call("get_profiles", json!(null)).await.is_err());
        client.close().await;
        fixture.shutdown().await;
    }
}
