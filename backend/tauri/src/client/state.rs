use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::{
    config::IVerge,
    state::verge::{
        StateActor, StateActorArgs, StateActorMessage, VergeMirror, VergeStateSnapshot,
    },
    utils::dirs,
};
use nyanpasu_core::state::PersistentStateManagerSetup;

const STATE_RPC_TIMEOUT: Duration = Duration::from_secs(5);
/// Must match the prefix used by the legacy `IVerge::save_file` so both writers
/// produce an identical header for `nyanpasu-config.yaml`.
/// The trailing `\n` ensures that `YamlFormat` (which calls `writeln!`) emits a
/// blank line between the prefix and the YAML body, matching `save_yaml`'s
/// `"{prefix}\n\n{data}"` layout.
const VERGE_CONFIG_PREFIX: &str = "# Clash Nyanpasu Config\n";

#[derive(Clone)]
pub struct StateClient {
    inner: Arc<StateClientInner>,
}

struct StateClientInner {
    actor_ref: ActorRef<StateActorMessage>,
}

impl StateClient {
    /// Production entry point: real `nyanpasu-config.yaml` + injected legacy mirror.
    pub fn new(initial: IVerge, mirror: VergeMirror) -> anyhow::Result<Self> {
        let path = Utf8PathBuf::from_path_buf(dirs::nyanpasu_config_path()?).map_err(|path| {
            anyhow::anyhow!("nyanpasu config path is not UTF-8: {}", path.display())
        })?;
        Self::new_with_path(path, initial, mirror)
    }

    /// Test/internal entry point: explicit config path (e.g. a tempdir).
    pub(crate) fn new_with_path(
        config_path: Utf8PathBuf,
        initial: IVerge,
        mirror: VergeMirror,
    ) -> anyhow::Result<Self> {
        // Mirror `ClashConnectionsConnector::new`: block_on initialization + spawn in a
        // synchronous context so the client can be constructed during Tauri setup.
        let manager = tauri::async_runtime::block_on(
            PersistentStateManagerSetup::<IVerge>::builder()
                .config_path(config_path)
                .config_prefix(VERGE_CONFIG_PREFIX.to_string())
                .assemble()
                .from_state(initial),
        )
        .context("failed to initialize verge persistent state manager")?;

        // Spawn anonymously: the client holds the `ActorRef` directly and never resolves
        // the actor by name, so a globally-registered name would only risk registry
        // collisions (e.g. across parallel tests or a client re-spawn).
        let actor_ref = tauri::async_runtime::block_on(Actor::spawn(
            None,
            StateActor,
            StateActorArgs { manager, mirror },
        ))
        .context("failed to spawn nyanpasu state actor")?
        .0;

        Ok(Self {
            inner: Arc::new(StateClientInner { actor_ref }),
        })
    }

    pub async fn get_verge(&self) -> anyhow::Result<IVerge> {
        Ok(self.call(StateActorMessage::GetVerge).await?.state)
    }

    pub async fn patch_verge(&self, patch: IVerge) -> anyhow::Result<VergeStateSnapshot> {
        self.call(|reply| StateActorMessage::PatchVerge { patch, reply })
            .await
    }

    pub async fn replace_verge(&self, state: IVerge) -> anyhow::Result<VergeStateSnapshot> {
        self.call(|reply| StateActorMessage::ReplaceVerge { state, reply })
            .await
    }

    async fn call<F>(&self, make: F) -> anyhow::Result<VergeStateSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<VergeStateSnapshot>>) -> StateActorMessage,
    {
        match self
            .inner
            .actor_ref
            .call(make, Some(STATE_RPC_TIMEOUT))
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("state actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("state actor call timed out"),
        }
    }
}

impl Drop for StateClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VergePatchRoute {
    PureConfig,
    LegacySideEffects,
}

/// Pure classifier (infallible). Validation is delegated to the actor (`PatchVerge`)
/// or to `feat::patch_verge`. The side-effect field set mirrors `feat::patch_verge`.
pub fn route_verge_patch(patch: &IVerge) -> VergePatchRoute {
    let legacy = patch.enable_service_mode.is_some()
        || patch.enable_tun_mode.is_some()
        || patch.enable_auto_launch.is_some()
        || patch.enable_system_proxy.is_some()
        || patch.system_proxy_bypass.is_some()
        || patch.enable_proxy_guard.is_some()
        || patch.hotkeys.is_some()
        || patch.language.is_some()
        || patch.app_log_level.is_some()
        || patch.max_log_files.is_some()
        || patch.clash_tray_selector.is_some()
        || patch.enable_tray_text.is_some()
        || patch.tray_menu_mode.is_some()
        || patch.network_statistic_widget.is_some();

    if legacy {
        VergePatchRoute::LegacySideEffects
    } else {
        VergePatchRoute::PureConfig
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::nyanpasu::{
        ClashCore, LoggingLevel, NetworkStatisticWidgetConfig, ProxiesSelectorMode, TrayMenuMode,
    };
    use std::sync::Mutex;
    use tempfile::{TempDir, tempdir};

    fn temp_config_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("nyanpasu-config.yaml"))
            .expect("temp path should be UTF-8")
    }

    fn capture_mirror() -> (VergeMirror, Arc<Mutex<Option<IVerge>>>) {
        let captured = Arc::new(Mutex::new(None::<IVerge>));
        let mirror_capture = captured.clone();
        let mirror: VergeMirror = Arc::new(move |state| {
            *mirror_capture
                .lock()
                .expect("mirror lock should not poison") = Some(state);
            Ok(())
        });
        (mirror, captured)
    }

    fn test_client(
        initial: IVerge,
    ) -> (
        StateClient,
        TempDir,
        Utf8PathBuf,
        Arc<Mutex<Option<IVerge>>>,
    ) {
        let dir = tempdir().expect("tempdir should be created");
        let path = temp_config_path(&dir);
        let (mirror, captured) = capture_mirror();
        let client = StateClient::new_with_path(path.clone(), initial, mirror)
            .expect("state client should be created");
        (client, dir, path, captured)
    }

    #[test]
    fn get_verge_returns_initial_state() {
        let initial = IVerge {
            theme_color: Some("#010203".into()),
            ..IVerge::default()
        };
        let (client, _dir, _path, _captured) = test_client(initial);

        tauri::async_runtime::block_on(async {
            let verge = client.get_verge().await.expect("get should succeed");
            assert_eq!(verge.theme_color.as_deref(), Some("#010203"));
        });
    }

    #[test]
    fn pure_patch_persists_and_mirrors() {
        let (client, _dir, path, captured) = test_client(IVerge::default());

        tauri::async_runtime::block_on(async {
            let snapshot = client
                .patch_verge(IVerge {
                    theme_color: Some("#112233".into()),
                    ..IVerge::default()
                })
                .await
                .expect("patch should succeed");

            assert_eq!(snapshot.state.theme_color.as_deref(), Some("#112233"));
            assert!(snapshot.version > 0);
        });

        let contents =
            std::fs::read_to_string(path.as_std_path()).expect("config should be written");
        assert!(contents.contains(VERGE_CONFIG_PREFIX));
        assert!(contents.contains("#112233"));

        let mirrored = captured
            .lock()
            .expect("mirror lock should not poison")
            .clone()
            .expect("mirror should be called");
        assert_eq!(mirrored.theme_color.as_deref(), Some("#112233"));
    }

    #[test]
    fn invalid_theme_color_does_not_commit() {
        let (client, _dir, path, captured) = test_client(IVerge::default());

        let err = tauri::async_runtime::block_on(async {
            client
                .patch_verge(IVerge {
                    theme_color: Some("red".into()),
                    ..IVerge::default()
                })
                .await
                .expect_err("invalid color should fail")
        });

        assert!(err.to_string().contains("Invalid theme color"));
        assert!(!path.as_std_path().exists());
        assert!(
            captured
                .lock()
                .expect("mirror lock should not poison")
                .is_none()
        );
    }

    #[test]
    fn replace_verge_persists_and_mirrors() {
        let (client, _dir, path, captured) = test_client(IVerge::default());

        tauri::async_runtime::block_on(async {
            let snapshot = client
                .replace_verge(IVerge {
                    theme_mode: Some("dark".into()),
                    ..IVerge::default()
                })
                .await
                .expect("replace should succeed");

            assert_eq!(snapshot.state.theme_mode.as_deref(), Some("dark"));
            assert!(snapshot.version > 0);
        });

        let contents =
            std::fs::read_to_string(path.as_std_path()).expect("config should be written");
        assert!(contents.contains("dark"));

        let mirrored = captured
            .lock()
            .expect("mirror lock should not poison")
            .clone()
            .expect("mirror should be called");
        assert_eq!(mirrored.theme_mode.as_deref(), Some("dark"));
    }

    /// Regression: a legacy writer (e.g. `change_clash_core`, window-state save, or a
    /// tray/hotkey toggle) reseeds the actor via `replace_verge`; a subsequent pure patch
    /// must NOT revert the reseeded field. This is the invariant the `run_legacy_verge_mutation`
    /// / `patch_verge_entrypoint` reseed paths rely on.
    #[test]
    fn reseed_then_pure_patch_preserves_reseeded_fields() {
        let (client, _dir, _path, _captured) = test_client(IVerge::default());

        tauri::async_runtime::block_on(async {
            // Out-of-band legacy mutation pushed into the actor.
            client
                .replace_verge(IVerge {
                    clash_core: Some(ClashCore::Mihomo),
                    ..IVerge::default()
                })
                .await
                .expect("reseed should succeed");

            // A pure patch on an unrelated field.
            let snapshot = client
                .patch_verge(IVerge {
                    theme_color: Some("#445566".into()),
                    ..IVerge::default()
                })
                .await
                .expect("patch should succeed");

            assert_eq!(snapshot.state.theme_color.as_deref(), Some("#445566"));
            // The reseeded clash_core must survive the pure patch.
            assert_eq!(snapshot.state.clash_core, Some(ClashCore::Mihomo));

            let verge = client.get_verge().await.expect("get should succeed");
            assert_eq!(verge.clash_core, Some(ClashCore::Mihomo));
        });
    }

    #[test]
    fn route_verge_patch_classifies_pure_fields() {
        macro_rules! assert_pure {
            ($field:ident: $value:expr) => {{
                let mut patch = IVerge::default();
                patch.$field = Some($value);
                assert_eq!(
                    route_verge_patch(&patch),
                    VergePatchRoute::PureConfig,
                    stringify!($field)
                );
            }};
        }

        assert_pure!(theme_color: "#112233".to_string());
        assert_pure!(traffic_graph: true);
        assert_pure!(theme_mode: "dark".to_string());
    }

    #[test]
    fn route_verge_patch_classifies_side_effect_fields() {
        macro_rules! assert_legacy {
            ($field:ident: $value:expr) => {{
                let mut patch = IVerge::default();
                patch.$field = Some($value);
                assert_eq!(
                    route_verge_patch(&patch),
                    VergePatchRoute::LegacySideEffects,
                    stringify!($field)
                );
            }};
        }

        assert_legacy!(enable_service_mode: true);
        assert_legacy!(enable_tun_mode: true);
        assert_legacy!(enable_auto_launch: true);
        assert_legacy!(enable_system_proxy: true);
        assert_legacy!(system_proxy_bypass: "localhost".to_string());
        assert_legacy!(enable_proxy_guard: true);
        assert_legacy!(hotkeys: Vec::<String>::new());
        assert_legacy!(language: "en".to_string());
        assert_legacy!(app_log_level: LoggingLevel::default());
        assert_legacy!(max_log_files: 7usize);
        assert_legacy!(clash_tray_selector: ProxiesSelectorMode::default());
        assert_legacy!(enable_tray_text: true);
        assert_legacy!(tray_menu_mode: TrayMenuMode::default());
        assert_legacy!(network_statistic_widget: NetworkStatisticWidgetConfig::default());
    }
}
