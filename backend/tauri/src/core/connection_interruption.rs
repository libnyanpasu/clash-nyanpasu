use crate::{config::Config, core::clash::api};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ConnectionInfo {
    pub id: String,
    pub chains: Vec<String>,
}

/// Connection interruption service that handles closing connections based on configuration settings
pub struct ConnectionInterruptionService;

impl ConnectionInterruptionService {
    /// Interrupt connections when proxy changes
    pub async fn on_proxy_change() -> Result<()> {
        let config = Config::verge().data().clone();
        let break_when = config.break_when_proxy_change.unwrap_or_default();

        match break_when {
            crate::config::nyanpasu::BreakWhenProxyChange::None => {
                // Do nothing
                Ok(())
            }
            crate::config::nyanpasu::BreakWhenProxyChange::Chain => {
                // TODO: Implement chain-based connection interruption
                // This would require tracking which connections use which proxy chains
                // For now, we'll fall back to closing all connections
                api::delete_connections(None).await
            }
            crate::config::nyanpasu::BreakWhenProxyChange::All => {
                api::delete_connections(None).await
            }
        }
    }

    /// Interrupt connections when profile changes
    pub async fn on_profile_change() -> Result<()> {
        let config = Config::verge().data().clone();
        let break_when = config.break_when_profile_change.unwrap_or_default();

        match break_when {
            crate::config::nyanpasu::BreakWhenProfileChange::Off => {
                // Do nothing
                Ok(())
            }
            crate::config::nyanpasu::BreakWhenProfileChange::On => {
                api::delete_connections(None).await
            }
        }
    }

    /// Interrupt connections when mode changes
    pub async fn on_mode_change() -> Result<()> {
        let config = Config::verge().data().clone();
        let break_when = config.break_when_mode_change.unwrap_or_default();

        match break_when {
            crate::config::nyanpasu::BreakWhenModeChange::Off => {
                // Do nothing
                Ok(())
            }
            crate::config::nyanpasu::BreakWhenModeChange::On => api::delete_connections(None).await,
        }
    }

    /// Interrupt all connections
    pub async fn interrupt_all() -> Result<()> {
        api::delete_connections(None).await
    }

    /// Interrupt connections based on proxy chain (not yet implemented)
    pub async fn interrupt_by_chain(_chain: &[String]) -> Result<()> {
        // TODO: Implement chain-based connection interruption
        // This would require:
        // 1. Getting the current connections from the Clash API
        // 2. Filtering connections that use the specified proxy chain
        // 3. Closing only those connections
        // For now, we'll close all connections as a fallback
        api::delete_connections(None).await
    }
}
