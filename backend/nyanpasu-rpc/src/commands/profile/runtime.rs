//! The runtime configuration the profiles produce.

nyanpasu_rpc::namespace!(expose = [http]);

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_profile_transform::PostProcessingOutput;
use nyanpasu_runtime_values::candidate::CandidateSnapshotId;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn get_postprocessing_output(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<PostProcessingOutput> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.postprocessing_output.clone())
        .unwrap_or_default())
}

/// get the runtime config
#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion. Replace with a typed ClashConfig struct if desired.
pub async fn get_runtime_config(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<specta_typescript::Any<serde_json::Value>>> {
    Ok(client
        .runtime_config()
        .await?
        .map(|config| serde_json::from_value(config).expect("a JSON value deserializes as itself")))
}

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_runtime_exists(client: RpcState<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.exists_keys.clone())
        .unwrap_or_default())
}

#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn get_runtime_yaml(client: RpcState<'_, NyanpasuClient>) -> Result<String> {
    Ok(client.runtime_yaml().await?)
}

#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn inspect_applied_runtime(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_applied_runtime().await)
}

#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn inspect_runtime(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_runtime().await)
}

#[nyanpasu_rpc::rpc(query, sensitive)]
#[specta::specta]
pub async fn inspect_runtime_node(
    client: RpcState<'_, NyanpasuClient>,
    snapshot_id: String,
    node_id: u32,
) -> Result<nyanpasu_core::client::runtime_inspection::RuntimeInspectionContent> {
    Ok(client.inspect_runtime_node(&snapshot_id, node_id).await?)
}

/// A candidate kept after a failed configuration change in this session. It
/// holds content that was never committed and unredacted step logs, so only
/// the desktop reaches it.
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[specta::specta]
pub async fn inspect_candidate_runtime(
    client: RpcState<'_, NyanpasuClient>,
    snapshot_id: CandidateSnapshotId,
) -> Result<nyanpasu_core::client::runtime_inspection::CandidateInspection> {
    Ok(client.inspect_candidate_runtime(&snapshot_id).await?)
}

/// One node of a kept candidate; desktop only, like the candidate itself.
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[specta::specta]
pub async fn inspect_candidate_node(
    client: RpcState<'_, NyanpasuClient>,
    snapshot_id: CandidateSnapshotId,
    node_id: u32,
) -> Result<nyanpasu_core::client::runtime_inspection::RuntimeInspectionContent> {
    Ok(client.inspect_candidate_node(&snapshot_id, node_id).await?)
}
