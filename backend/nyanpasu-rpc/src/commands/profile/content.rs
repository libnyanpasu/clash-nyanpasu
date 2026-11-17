//! Profile files: reading and saving them. A profile file can hold subscription URLs and
//! credentials, so reading one is sensitive.

use nyanpasu_config::profile::ProfileId;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(operation = nyanpasu_rpc_protocol::operations::ReadProfileFile, query, sensitive)]
#[specta::specta]
pub async fn read_profile_file(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn save_profile_file(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.save_profile_file(uid, file_data).await?.into())
}
