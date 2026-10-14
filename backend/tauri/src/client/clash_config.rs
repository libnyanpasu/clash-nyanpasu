use crate::state::mutation::MutationCoordinator;
use nyanpasu_core::migration::modules::clash_config::ClashConfigFormat;
use std::sync::Arc;

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::clash::config::{
    ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch,
};
use nyanpasu_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    client::application_workflow::mutation::ConfigDomain,
    state::{
        clash_config::{
            ClashConfigActor, ClashConfigActorArgs, ClashConfigActorMessage, ClashConfigSnapshot,
        },
        config_error::{ConfigError, OwnerStoppedSnafu},
    },
};

#[derive(Clone)]
pub struct ClashConfigClient {
    inner: Arc<ClashConfigClientInner>,
}

struct ClashConfigClientInner {
    actor_ref: ActorRef<ClashConfigActorMessage>,
    /// Committed state, read straight from the coordinator's store so a reader
    /// never queues behind a mutation the actor is still holding open.
    snapshot: StateSnapshot<ClashConfig>,
}

#[allow(dead_code)]
impl ClashConfigClient {
    pub(crate) async fn new(
        mutations: MutationCoordinator,
        config_path: Utf8PathBuf,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<ClashConfig, ClashConfigFormat>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load clash persistent state manager")?
        } else {
            setup
                .from_state(ClashConfig::default())
                .await
                .context("failed to initialize clash persistent state manager")?
        };

        let snapshot = manager.snapshot_handle();
        let actor_ref = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs {
                manager,
                mutations,
                shutdown: shutdown.clone(),
            },
        )
        .await
        .context("failed to spawn clash config actor")?
        .0;
        nyanpasu_core::tasks::drain_on_shutdown(tasks, shutdown, actor_ref.get_cell());

        Ok(Self {
            inner: Arc::new(ClashConfigClientInner {
                actor_ref,
                snapshot,
            }),
        })
    }

    /// The last committed clash config. Reads bypass the mailbox, so an
    /// in-flight transaction parked in `on_prepare` cannot delay them.
    pub fn snapshot(&self) -> ClashConfigSnapshot {
        ClashConfigSnapshot::from_versioned(&self.inner.snapshot.load())
    }

    /// Read-only handle for collaborators that must observe committed state
    /// without holding a client that could write it.
    pub(crate) fn snapshot_handle(&self) -> StateSnapshot<ClashConfig> {
        self.inner.snapshot.clone()
    }

    pub async fn patch(&self, patch: ClashConfigPatch) -> Result<ClashConfigSnapshot, ConfigError> {
        self.call(
            |reply| ClashConfigActorMessage::Patch { patch, reply },
            None,
        )
        .await
    }

    pub async fn patch_overrides(
        &self,
        patch: ClashGuardOverridesPatch,
    ) -> Result<ClashConfigSnapshot, ConfigError> {
        self.call(
            |reply| ClashConfigActorMessage::PatchOverrides { patch, reply },
            None,
        )
        .await
    }

    pub async fn replace(&self, state: ClashConfig) -> Result<ClashConfigSnapshot, ConfigError> {
        self.call(
            |reply| ClashConfigActorMessage::Replace { state, reply },
            None,
        )
        .await
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<std::time::Duration>,
    ) -> Result<ClashConfigSnapshot, ConfigError>
    where
        F: FnOnce(
            RpcReplyPort<Result<ClashConfigSnapshot, ConfigError>>,
        ) -> ClashConfigActorMessage,
    {
        match self.inner.actor_ref.call(make, timeout).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) | Err(_) => OwnerStoppedSnafu {
                domain: ConfigDomain::Clash,
            }
            .fail(),
            Ok(CallResult::Timeout) => {
                unreachable!("clash config calls are made without a timeout")
            }
        }
    }
}

impl Drop for ClashConfigClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    fn temp_config_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml"))
            .expect("temp path should be UTF-8")
    }

    async fn test_client() -> (ClashConfigClient, TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let client = ClashConfigClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("clash config client should be created");
        (client, dir)
    }

    #[tokio::test]
    async fn get_patch_and_replace_clash_config() {
        let (client, _dir) = test_client().await;

        let initial = client.snapshot();
        assert!(!initial.state.enable_tun_mode);

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let patched = client.patch(patch).await.expect("patch should succeed");
        assert!(patched.state.enable_tun_mode);

        let replaced = client
            .replace(ClashConfig::default())
            .await
            .expect("replace should succeed");
        assert!(!replaced.state.enable_tun_mode);
    }

    #[tokio::test]
    async fn concurrent_override_patches_preserve_unrelated_fields() {
        let (client, _dir) = test_client().await;
        let before = serde_json::to_value(client.snapshot().state.overrides).unwrap();
        let left = serde_json::from_value(serde_json::json!({"mode":"script"})).unwrap();
        let right = serde_json::from_value(serde_json::json!({"allow-lan":true})).unwrap();
        let (left, right) =
            tokio::join!(client.patch_overrides(left), client.patch_overrides(right));
        left.unwrap();
        right.unwrap();
        let after = serde_json::to_value(client.snapshot().state.overrides).unwrap();
        assert_eq!(after["mode"], "script");
        assert_eq!(after["allow-lan"], true);
        assert_eq!(after["secret"], before["secret"]);
        assert_eq!(after["ipv6"], before["ipv6"]);
    }

    /// Racing patches of sibling sub-fields of one composite field all land:
    /// each nested patch is merged into the latest committed value.
    #[tokio::test]
    async fn concurrent_nested_patches_preserve_sibling_sub_fields() {
        use nyanpasu_config::clash::config::clash_strategy::{
            break_connection::ProxyChangeBreakMode, port::PortStrategyKind,
        };

        let (client, _dir) = test_client().await;
        let before = client.snapshot().state;
        let patch = |value| serde_json::from_value::<ClashConfigPatch>(value).unwrap();
        let (proxy, profile, kind, port) = tokio::join!(
            client.patch(patch(serde_json::json!({
                "break_connection": { "on_proxy_change": "off" }
            }))),
            client.patch(patch(serde_json::json!({
                "break_connection": { "on_profile_change": false }
            }))),
            client.patch(patch(serde_json::json!({
                "mixed_port": { "kind": "random" }
            }))),
            client.patch(patch(serde_json::json!({
                "mixed_port": { "start_port": 7899 }
            }))),
        );
        proxy.unwrap();
        profile.unwrap();
        kind.unwrap();
        port.unwrap();

        let after = client.snapshot().state;
        assert_eq!(
            after.break_connection.on_proxy_change,
            ProxyChangeBreakMode::Off
        );
        assert!(!after.break_connection.on_profile_change);
        assert_eq!(
            after.break_connection.on_mode_change,
            before.break_connection.on_mode_change
        );
        assert_eq!(after.mixed_port.kind, PortStrategyKind::Random);
        assert_eq!(after.mixed_port.start_port, 7899);
    }
}
