//! Configuration-sync command adapters.
//!
//! None of these declares `rpc(http)`: a browser caller must not be able to
//! redirect a device's configuration to another endpoint or read conflict
//! contents. Exposure to WebUI or the local socket is a separate, reviewed
//! opt-in.

use nyanpasu_contracts::sync::{ProviderKind, ProviderSecrets, SyncSettings};
use nyanpasu_core::{
    client::NyanpasuClient,
    sync::{SettingsReply, SnapshotSummary},
};
use nyanpasu_runtime_values::sync::{ConflictChoice, SyncConflict, SyncStatus};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{error::Result, registry::RpcState};

/// The sync status, without the credentials a provider may hold.
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_sync_status(client: RpcState<'_, NyanpasuClient>) -> Result<SyncStatus> {
    Ok(client.sync_status().await?)
}

/// Settings, whether credentials exist, and the provider kinds this build has.
#[nyanpasu_rpc::rpc(query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_sync_settings(client: RpcState<'_, NyanpasuClient>) -> Result<SyncSettingsView> {
    let reply: SettingsReply = client.sync_settings().await?;
    Ok(SyncSettingsView::from(reply))
}

/// Provider kinds compiled into this build.
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_sync_provider_kinds(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<ProviderKind>> {
    let settings = client.sync_settings().await?;
    Ok(settings.provider_kinds)
}

/// Open conflicts with both values.
#[nyanpasu_rpc::rpc(query, scope = sensitive_read)]
#[specta::specta]
pub async fn list_sync_conflicts(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<SyncConflict>> {
    Ok(client.list_sync_conflicts().await?)
}

/// Pre-image snapshots this device took.
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub fn list_sync_snapshots(client: RpcState<'_, NyanpasuClient>) -> Vec<SnapshotSummary> {
    client.list_sync_snapshots()
}

/// Saves the provider, its secrets and the sync options.
#[nyanpasu_rpc::rpc(scope = settings)]
#[specta::specta]
pub async fn configure_sync(
    client: RpcState<'_, NyanpasuClient>,
    settings: SyncSettings,
    secrets: Option<SyncSecrets>,
) -> Result<()> {
    Ok(client
        .configure_sync(settings, secrets.map(Into::into))
        .await?)
}

/// Checks reachability and credentials without saving anything.
#[nyanpasu_rpc::rpc(scope = settings)]
#[specta::specta]
pub async fn test_sync_connection(
    client: RpcState<'_, NyanpasuClient>,
    settings: SyncSettings,
    secrets: Option<SyncSecrets>,
) -> Result<()> {
    Ok(client
        .test_sync_connection(settings, secrets.map(Into::into))
        .await?)
}

/// Runs the first cycle and keeps syncing.
#[nyanpasu_rpc::rpc(scope = settings)]
#[specta::specta]
pub async fn enable_sync(client: RpcState<'_, NyanpasuClient>) -> Result<SyncRunView> {
    Ok(client.enable_sync().await?.into())
}

/// Stops syncing; local data and the remote vault stay.
#[nyanpasu_rpc::rpc(scope = settings)]
#[specta::specta]
pub async fn disable_sync(client: RpcState<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.disable_sync().await?)
}

/// Runs one cycle now.
#[nyanpasu_rpc::rpc(scope = settings)]
#[specta::specta]
pub async fn sync_now(client: RpcState<'_, NyanpasuClient>) -> Result<SyncRunView> {
    Ok(client.sync_now().await?.into())
}

/// Records a conflict resolution and runs a cycle.
#[nyanpasu_rpc::rpc(scope = settings)]
#[specta::specta]
pub async fn resolve_sync_conflict(
    client: RpcState<'_, NyanpasuClient>,
    conflict_id: String,
    choice: ConflictChoice,
) -> Result<SyncRunView> {
    Ok(client
        .resolve_sync_conflict(conflict_id, choice)
        .await?
        .into())
}

/// Re-applies a snapshot's pre-images; the next cycle pushes them.
#[nyanpasu_rpc::rpc(scope = settings)]
#[specta::specta]
pub async fn restore_sync_snapshot(
    client: RpcState<'_, NyanpasuClient>,
    snapshot_id: String,
) -> Result<()> {
    Ok(client.restore_sync_snapshot(snapshot_id).await?)
}

/// The settings view: no secret value can appear in it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncSettingsView {
    pub settings: SyncSettings,
    pub credentials_present: bool,
    pub provider_kinds: Vec<ProviderKind>,
}

impl From<SettingsReply> for SyncSettingsView {
    fn from(reply: SettingsReply) -> Self {
        Self {
            settings: reply.settings,
            credentials_present: reply.credentials_present,
            provider_kinds: reply.provider_kinds,
        }
    }
}

/// Write-only credentials: accepted, never returned.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct SyncSecrets {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
}

impl From<SyncSecrets> for ProviderSecrets {
    fn from(secrets: SyncSecrets) -> Self {
        let mut provider = ProviderSecrets::default();
        if let Some(password) = secrets.password {
            provider = provider.with(
                "password",
                nyanpasu_contracts::auth::SecretString::new(password),
            );
        }
        if let Some(key) = secrets.secret_access_key {
            provider = provider.with(
                "secret_access_key",
                nyanpasu_contracts::auth::SecretString::new(key),
            );
        }
        provider
    }
}

/// One cycle's outcome, without conflict contents.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncRunView {
    pub outcome: Option<nyanpasu_runtime_values::sync::SyncOutcome>,
    pub pushed: u32,
    pub pulled: u32,
    pub merged: u32,
    pub blocked: Vec<nyanpasu_runtime_values::sync::BlockedItem>,
    pub conflicts: u32,
}

impl From<nyanpasu_core::sync::CycleReport> for SyncRunView {
    fn from(report: nyanpasu_core::sync::CycleReport) -> Self {
        Self {
            outcome: report.outcome,
            pushed: report.pushed,
            pulled: report.pulled,
            merged: report.merged,
            blocked: report.blocked,
            conflicts: report.conflicts.len() as u32,
        }
    }
}
