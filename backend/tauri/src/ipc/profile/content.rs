//! Probing a subscription URL from the desktop. The shared profile file commands are in
//! `nyanpasu_rpc::commands::profile::content`.

use nyanpasu_config::profile::RemoteProfileOptionsPatch;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use nyanpasu_runtime_values::profiles::InvalidSubscriptionUrlSnafu;
use tauri::State;

/// Downloads a profile URL and suggests how to import it, for the user to
/// confirm. Desktop-only: it sends a request to any URL the caller names.
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[tauri::command]
#[specta::specta]
pub async fn probe_profile_url(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<nyanpasu_core::client::profiles::ProfileUrlProbe> {
    let url = snafu::ResultExt::context(
        url::Url::parse(&url),
        InvalidSubscriptionUrlSnafu { url: &url },
    )?;
    Ok(client.probe_profile_url(url, option).await?)
}
