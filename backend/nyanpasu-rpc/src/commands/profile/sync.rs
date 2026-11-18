//! Profile synchronization: its status and run history.

nyanpasu_rpc::namespace!(expose = [http]);

use nyanpasu_config::profile::ProfileId;
use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn get_profile_sync_logs(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    run: String,
    after: Option<String>,
) -> Result<nyanpasu_jobs::dto::LogPageDto> {
    Ok(client.profile_sync_logs(uid, run, after).await?)
}

#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn get_profile_sync_runs(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    after: Option<nyanpasu_jobs::dto::RunCursorDto>,
) -> Result<nyanpasu_jobs::dto::RunPageDto> {
    Ok(client.profile_sync_runs(uid, after).await?)
}

/// One page of sync runs with each outcome reduced to its kind. The full page is
/// [`get_profile_sync_runs`].
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_profile_sync_runs_summary(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    after: Option<nyanpasu_jobs::dto::RunCursorDto>,
) -> Result<nyanpasu_core::client::jobs::SyncRunPageSummary> {
    Ok(client.profile_sync_runs_summary(uid, after).await?)
}

// Run logs and errors carry the failing request's URL, which can hold a subscription token.
#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn get_profile_sync_status(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::jobs::ProfileSyncStatus> {
    Ok(client.profile_sync_status(uid).await?)
}

/// The sync schedule and active runs without the registration error text. The full status is
/// [`get_profile_sync_status`].
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_profile_sync_status_summary(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::jobs::ProfileSyncSummary> {
    Ok(client.profile_sync_status_summary(uid).await?)
}
