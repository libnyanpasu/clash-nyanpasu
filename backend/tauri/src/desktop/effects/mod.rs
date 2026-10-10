//! Peripheral effects are queued after commit and never vote on source state.
use self::plan::TrayView;
use nyanpasu_core::{NyanpasuClient, effects::plan::ApplicationEffectInputs};

pub mod plan;
pub mod presentation;

/// What the tray renders from the committed configuration, for the tray
/// to start from before the first tray effect reaches it.
pub(crate) async fn tray_view(client: &NyanpasuClient) -> nyanpasu_core::client::Result<TrayView> {
    let inputs = ApplicationEffectInputs::project(
        &client.app_config_snapshot(),
        &client.get_clash_config().await?,
        client.session_ports(),
    );
    Ok(presentation::tray_view(&inputs.app, &inputs.clash))
}

#[cfg(test)]
mod tests;
