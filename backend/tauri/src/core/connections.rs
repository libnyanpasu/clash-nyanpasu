use nyanpasu_config::clash::config::clash_strategy::ProxyChangeBreakMode;

use super::actor_v2::api::{ApiClient, ApiError};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConnectionScope {
    All,
    ProxyGroup { name: String },
}

impl ConnectionScope {
    pub fn for_proxy_change(strategy: ProxyChangeBreakMode, group: String) -> Option<Self> {
        match strategy {
            ProxyChangeBreakMode::Off => None,
            ProxyChangeBreakMode::All => Some(Self::All),
            ProxyChangeBreakMode::ProxyGroup => Some(Self::ProxyGroup { name: group }),
        }
    }
}

pub(crate) async fn interrupt_connections(
    source: &ApiClient,
    scope: &ConnectionScope,
) -> Result<(), ApiError> {
    match scope {
        ConnectionScope::All => source.close_all_connections().await?,
        ConnectionScope::ProxyGroup { name } => {
            for connection in source.connections().await?.connections.unwrap_or_default() {
                if connection.chains.contains(name) {
                    source.close_connection(connection.id).await?;
                }
            }
        }
    }
    Ok(())
}
