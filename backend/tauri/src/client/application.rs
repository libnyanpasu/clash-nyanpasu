use crate::state::mutation::MutationCoordinator;
use std::sync::Arc;

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::{PersistentStateManager, PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    client::application_workflow::mutation::ConfigDomain,
    core::migration::modules::application::ApplicationFormat,
    state::{
        application::{
            ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
        },
        config_error::{ConfigError, OwnerStoppedSnafu},
    },
};

#[derive(Clone)]
pub struct ApplicationClient {
    inner: Arc<ApplicationClientInner>,
}

struct ApplicationClientInner {
    actor_ref: ActorRef<ApplicationActorMessage>,
    /// Committed state, read straight from the coordinator's store so a reader
    /// never queues behind a mutation the actor is still holding open.
    snapshot: StateSnapshot<NyanpasuAppConfig>,
    settings_changes: tokio::sync::watch::Receiver<NyanpasuAppConfig>,
}

#[allow(dead_code)]
impl ApplicationClient {
    pub(crate) async fn new(
        mutations: MutationCoordinator,
        build_channel: crate::bundle::Channel,
        config_path: Utf8PathBuf,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let mut seed = NyanpasuAppConfig::default();
        seed.release_channel = Some(build_channel.resolve(seed.release_channel));
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<NyanpasuAppConfig, ApplicationFormat>::builder()
            .config_path(config_path)
            .assemble();
        let mut manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load application persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize application persistent state manager")?
        };

        let mut initial = manager.snapshot_handle().load().state.clone();
        let channel = build_channel.resolve(initial.release_channel);
        if initial.release_channel != Some(channel) {
            initial.release_channel = Some(channel);
            manager
                .upsert(initial)
                .await
                .context("failed to persist release channel")?;
        }

        Self::from_manager(mutations, manager, build_channel, shutdown, tasks).await
    }

    /// Takes ownership of an already loaded manager. Separate from [`Self::new`]
    /// so a caller can register state subscribers before the actor claims it.
    pub(crate) async fn from_manager(
        mutations: MutationCoordinator,
        manager: PersistentStateManager<NyanpasuAppConfig, ApplicationFormat>,
        build_channel: crate::bundle::Channel,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        nyanpasu_config::application::validate_update_sources(
            &manager.snapshot_handle().load().state.update_sources,
        )
        .map_err(anyhow::Error::msg)?;
        manager
            .snapshot_handle()
            .load()
            .state
            .core_logs
            .validate()
            .map_err(anyhow::Error::msg)?;
        let snapshot = manager.snapshot_handle();
        let (settings_tx, settings_changes) =
            tokio::sync::watch::channel(snapshot.load().state.clone());
        let actor_ref = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs {
                manager,
                mutations,
                build_channel,
                shutdown: shutdown.clone(),
                settings_changes: settings_tx,
            },
        )
        .await
        .context("failed to spawn application actor")?
        .0;
        crate::client::drain_on_shutdown(tasks, shutdown, actor_ref.get_cell());

        Ok(Self {
            inner: Arc::new(ApplicationClientInner {
                actor_ref,
                snapshot,
                settings_changes,
            }),
        })
    }

    /// The last committed application config. Reads bypass the mailbox, so an
    /// in-flight transaction parked in `on_prepare` cannot delay them.
    pub fn snapshot(&self) -> ApplicationSnapshot {
        ApplicationSnapshot::from_versioned(&self.inner.snapshot.load())
    }

    /// Read-only handle for collaborators that must observe committed state
    /// without holding a client that could write it.
    pub(crate) fn snapshot_handle(&self) -> StateSnapshot<NyanpasuAppConfig> {
        self.inner.snapshot.clone()
    }

    /// Receives committed application settings from their serial owner.
    pub(crate) fn subscribe_settings_changes(
        &self,
    ) -> tokio::sync::watch::Receiver<NyanpasuAppConfig> {
        self.inner.settings_changes.clone()
    }

    pub async fn patch(
        &self,
        patch: NyanpasuAppConfigPatch,
    ) -> Result<ApplicationSnapshot, ConfigError> {
        self.call(
            |reply| ApplicationActorMessage::Patch { patch, reply },
            None,
        )
        .await
    }

    pub async fn replace(
        &self,
        state: NyanpasuAppConfig,
    ) -> Result<ApplicationSnapshot, ConfigError> {
        self.call(
            |reply| ApplicationActorMessage::Replace { state, reply },
            None,
        )
        .await
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<std::time::Duration>,
    ) -> Result<ApplicationSnapshot, ConfigError>
    where
        F: FnOnce(
            RpcReplyPort<Result<ApplicationSnapshot, ConfigError>>,
        ) -> ApplicationActorMessage,
    {
        match self.inner.actor_ref.call(make, timeout).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) | Err(_) => OwnerStoppedSnafu {
                domain: ConfigDomain::Application,
            }
            .fail(),
            Ok(CallResult::Timeout) => {
                unreachable!("application config calls are made without a timeout")
            }
        }
    }
}

impl Drop for ApplicationClientInner {
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
        Utf8PathBuf::from_path_buf(dir.path().join("application.yaml"))
            .expect("temp path should be UTF-8")
    }

    async fn test_client() -> (ApplicationClient, TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let client = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            crate::bundle::Channel::Stable,
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("application client should be created");
        (client, dir)
    }

    #[tokio::test]
    async fn get_patch_and_replace_application_config() {
        let (client, _dir) = test_client().await;

        let initial = client.snapshot();
        assert!(!initial.state.enable_system_proxy);

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let patched = client.patch(patch).await.expect("patch should succeed");
        assert!(patched.state.enable_system_proxy);

        let mut replacement = NyanpasuAppConfig::default();
        replacement.enable_silent_start = true;
        let replaced = client
            .replace(replacement)
            .await
            .expect("replace should succeed");
        assert!(replaced.state.enable_silent_start);
    }

    #[tokio::test]
    async fn a_write_after_shutdown_is_refused_as_shutting_down() {
        let dir = tempdir().expect("tempdir should be created");
        let shutdown = tokio_util::sync::CancellationToken::new();
        let client = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            crate::bundle::Channel::Stable,
            temp_config_path(&dir),
            shutdown.clone(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("application client should be created");
        shutdown.cancel();

        assert!(matches!(
            client.patch(NyanpasuAppConfig::new_empty_patch()).await,
            Err(ConfigError::ShuttingDown {
                domain: ConfigDomain::Application
            })
        ));
    }

    #[tokio::test]
    async fn update_sources_preserve_order_and_reject_invalid_writes() {
        use nyanpasu_config::application::UpdateSource;
        let (client, dir) = test_client().await;
        let selected = vec![
            UpdateSource::Ghfast,
            UpdateSource::Sourceforge,
            UpdateSource::Github,
            UpdateSource::Nyanpasu,
        ];
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.update_sources = Some(selected.clone());
        assert_eq!(
            client.patch(patch).await.unwrap().state.update_sources,
            selected
        );
        let committed_version = client.snapshot().version;
        for invalid in [
            vec![],
            vec![UpdateSource::Github, UpdateSource::Github],
            vec![UpdateSource::Ghfast, UpdateSource::Ghfast],
            vec![UpdateSource::Sourceforge, UpdateSource::Sourceforge],
        ] {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.update_sources = Some(invalid.clone());
            assert!(matches!(
                client.patch(patch).await,
                Err(ConfigError::InvalidUpdateSources { .. })
            ));
            let mut replacement = client.snapshot().state;
            replacement.update_sources = invalid;
            assert!(matches!(
                client.replace(replacement).await,
                Err(ConfigError::InvalidUpdateSources { .. })
            ));
            assert_eq!(client.snapshot().version, committed_version);
            assert_eq!(client.snapshot().state.update_sources, selected);
        }
        drop(client);
        let reloaded = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            crate::bundle::Channel::Stable,
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        assert_eq!(reloaded.snapshot().state.update_sources, selected);
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.update_sources = Some(vec![UpdateSource::Github]);
        assert_eq!(
            reloaded.patch(patch).await.unwrap().state.update_sources,
            vec![UpdateSource::Github]
        );
    }

    #[tokio::test]
    async fn latency_timeout_outside_range_is_rejected() {
        let (client, _dir) = test_client().await;
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.default_latency_timeout_ms = Some(500);
        assert!(matches!(
            client.patch(patch).await,
            Err(ConfigError::InvalidLatencyTimeout { .. })
        ));
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.default_latency_timeout_ms = Some(8000);
        assert_eq!(
            client
                .patch(patch)
                .await
                .unwrap()
                .state
                .default_latency_timeout_ms,
            8000
        );
    }

    #[tokio::test]
    async fn release_channel_persists_and_non_nightly_builds_can_leave_nightly() {
        use crate::bundle::Channel;
        let (client, dir) = test_client().await;
        for channel in [Channel::Beta, Channel::Stable, Channel::Nightly] {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.release_channel = Some(Some(channel));
            assert_eq!(
                client.patch(patch).await.unwrap().state.release_channel,
                Some(channel)
            );
        }
        drop(client);
        // A stable or beta build installed over a Nightly preference keeps it
        // until the user leaves, and is allowed to leave.
        for build in [Channel::Stable, Channel::Beta] {
            let reloaded = ApplicationClient::new(
                crate::state::mutation::MutationCoordinator::isolated(),
                build,
                temp_config_path(&dir),
                tokio_util::sync::CancellationToken::new(),
                &tokio_util::task::TaskTracker::new(),
            )
            .await
            .unwrap();
            assert_eq!(
                reloaded.snapshot().state.release_channel,
                Some(Channel::Nightly)
            );
            for channel in [build, Channel::Nightly] {
                let mut patch = NyanpasuAppConfig::new_empty_patch();
                patch.release_channel = Some(Some(channel));
                assert_eq!(
                    reloaded.patch(patch).await.unwrap().state.release_channel,
                    Some(channel)
                );
            }
        }
    }

    #[tokio::test]
    async fn nightly_build_cannot_leave_nightly() {
        use crate::bundle::Channel;
        let dir = tempdir().unwrap();
        let nightly = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Channel::Nightly,
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        for channel in [Channel::Stable, Channel::Beta] {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.release_channel = Some(Some(channel));
            assert!(matches!(
                nightly.patch(patch).await,
                Err(ConfigError::LeaveNightlyChannel { to }) if to == channel
            ));
            let mut replacement = NyanpasuAppConfig::default();
            replacement.release_channel = Some(channel);
            assert!(matches!(
                nightly.replace(replacement).await,
                Err(ConfigError::LeaveNightlyChannel { to }) if to == channel
            ));
        }
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.release_channel = Some(None);
        assert_eq!(
            nightly.patch(patch).await.unwrap().state.release_channel,
            Some(Channel::Nightly)
        );
    }

    #[tokio::test]
    async fn release_channel_compiled_nightly_overrides_saved_stable() {
        use crate::bundle::Channel;
        let (client, dir) = test_client().await;
        assert_eq!(
            client.snapshot().state.release_channel,
            Some(Channel::Stable)
        );
        let nightly = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Channel::Nightly,
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            nightly.snapshot().state.release_channel,
            Some(Channel::Nightly)
        );
    }
    #[tokio::test]
    async fn window_close_patches_merge_by_field_and_reach_no_runtime() {
        use nyanpasu_config::application::{WindowCloseBehavior, WindowCloseOverride};
        let (client, _dir) = test_client().await;
        let mut first = NyanpasuAppConfig::new_empty_patch();
        first.window_close.global = Some(WindowCloseBehavior::Hide);
        client.patch(first).await.unwrap();

        let mut second = NyanpasuAppConfig::new_empty_patch();
        second.window_close.tray_menu = Some(WindowCloseOverride::Hide);
        let snapshot = client.patch(second).await.unwrap();

        assert_eq!(
            snapshot.state.window_close.global,
            WindowCloseBehavior::Hide
        );
        assert_eq!(
            snapshot.state.window_close.tray_menu,
            WindowCloseOverride::Hide
        );
        // No runtime transaction was opened for it.
        assert_eq!(snapshot.receipt.unwrap().operation_id, None);
    }

    #[tokio::test]
    async fn release_channel_migrates_old_beta_config_and_keeps_explicit_stable() {
        use crate::bundle::Channel;
        let dir = tempdir().unwrap();
        let manager =
            PersistentStateManagerSetup::<NyanpasuAppConfig, ApplicationFormat>::builder()
                .config_path(temp_config_path(&dir))
                .assemble()
                .from_state(NyanpasuAppConfig::default())
                .await
                .unwrap();
        drop(manager);
        let beta = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Channel::Beta,
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        assert_eq!(beta.snapshot().state.release_channel, Some(Channel::Beta));
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.release_channel = Some(Some(Channel::Stable));
        beta.patch(patch).await.unwrap();
        drop(beta);
        let reloaded = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Channel::Beta,
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            reloaded.snapshot().state.release_channel,
            Some(Channel::Stable)
        );
    }
}
