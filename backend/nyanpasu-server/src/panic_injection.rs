//! Faults linked only into the server binary built with the test-only feature.

use std::sync::Arc;

use nyanpasu_core::client::effects::ports::LogRotation;
use nyanpasu_rpc::registry::{CommandEntry, RpcDependencies, RpcFuture};
use nyanpasu_rpc_protocol::{CommandKind, CommandMetadata, RpcError, RpcOwner, RpcTarget};
use serde_json::Value;

const PANIC_PAYLOAD: &str = "e4b-fatal-original-payload-53fd73";

fn panic_handler(_: Arc<RpcDependencies>, _: RpcOwner, _: Value) -> Result<RpcFuture, RpcError> {
    Ok(Box::pin(nyanpasu_rpc::registry::unreported(async {
        panic!("{PANIC_PAYLOAD}")
    })))
}

fn owner_work(deps: Arc<RpcDependencies>, _: RpcOwner, _: Value) -> Result<RpcFuture, RpcError> {
    use struct_patch::Patch as _;
    Ok(Box::pin(nyanpasu_rpc::registry::unreported(async move {
        let mut patch = nyanpasu_config::application::NyanpasuAppConfig::new_empty_patch();
        patch.max_log_file_size = Some(9);
        deps.client
            .patch_app_config(patch)
            .await
            .map_err(nyanpasu_rpc::error::application_error)?;
        Ok(Value::Null)
    })))
}

// This is called by the real effect owner, after the triggering mutation commits.
pub(crate) fn logger_refresh(rotation: Option<&LogRotation>) {
    if rotation.is_some_and(|rotation| rotation.max_file_size == 9)
        && std::env::var("NYANPASU_E4B_PANIC_TEST").as_deref() == Ok("owner")
    {
        panic!("{PANIC_PAYLOAD}");
    }
}

inventory::submit! {
    CommandEntry {
        metadata: CommandMetadata { name: "e4b_test_panic", kind: CommandKind::Shared,
            targets: &[RpcTarget::Http, RpcTarget::Local], capabilities: &[], optional_capabilities: &[] },
        handler: panic_handler,
    }
}

inventory::submit! {
    CommandEntry {
        metadata: CommandMetadata { name: "e4b_test_owner_work", kind: CommandKind::Shared,
            targets: &[RpcTarget::Local], capabilities: &[], optional_capabilities: &[] },
        handler: owner_work,
    }
}
