//! Desktop local-socket endpoint override helper and lifecycle tests.

use std::path::PathBuf;

pub(super) fn endpoint_override_from_environment() -> Option<PathBuf> {
    std::env::var_os("NYANPASU_LOCAL_RPC_ENDPOINT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use nyanpasu_config::application::LocalSocketAccessConfig;
    use nyanpasu_rpc::{
        local::control::{LocalSocketClient, RpcLocalRoutes},
        test_support::Fixture,
    };
    use nyanpasu_rpc_protocol::{client::LocalClient, frame::Reply};
    use serde_json::json;

    #[tokio::test]
    async fn desktop_local_socket_actor_reconciles_enabled_state_and_drains_on_shutdown() {
        let fixture = Fixture::new(false).await;
        let routes = Arc::new(RpcLocalRoutes::default());
        routes.install(&fixture.rpc).unwrap();
        let directory = tempfile::tempdir().unwrap();

        let control = LocalSocketClient::spawn(directory.path().to_owned(), None, routes)
            .await
            .unwrap();
        assert!(!control.status().await.unwrap().enabled);

        let enabled = control
            .configure(&LocalSocketAccessConfig { enabled: true })
            .await
            .unwrap();
        assert!(enabled.enabled);
        let endpoint = enabled.endpoint.unwrap();

        let client = LocalClient::connect(std::path::Path::new(&endpoint), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(matches!(
            client
                .call("get_configuration_status", json!(null))
                .await
                .unwrap(),
            Reply::Result(_)
        ));
        client.close().await;

        control.shutdown().await.unwrap();
        assert!(
            LocalClient::connect(std::path::Path::new(&endpoint), Duration::from_millis(200))
                .await
                .is_err()
        );
        fixture.shutdown().await;
    }
}
