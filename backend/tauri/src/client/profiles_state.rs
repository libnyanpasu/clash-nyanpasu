use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::{
    config::Profiles,
    state::profiles::{
        ProfilesMirror, ProfilesStateActor, ProfilesStateActorArgs, ProfilesStateMessage,
        ProfilesStateSnapshot,
    },
    utils::dirs,
};
use nyanpasu_core::state::PersistentStateManagerSetup;

/// Reads only touch the in-memory snapshot, so a short timeout is enough.
const PROFILES_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Commits perform a local `upsert` (disk write) + mirror; allow more headroom. A
/// `Timeout` is treated as unknown by the caller — the next absolute commit corrects it.
const PROFILES_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Must match the prefix used by the legacy `Profiles::save_file` so both writers
/// produce an identical header for `profiles.yaml`. The trailing `\n` ensures
/// `YamlFormat` (which calls `writeln!`) emits a blank line between the prefix and the
/// YAML body, matching `save_yaml`'s `"{prefix}\n\n{data}"` layout.
const PROFILES_CONFIG_PREFIX: &str = "# Profiles Config for Clash Nyanpasu\n";

#[derive(Clone)]
pub struct ProfilesStateClient {
    inner: Arc<ProfilesStateClientInner>,
}

struct ProfilesStateClientInner {
    actor_ref: ActorRef<ProfilesStateMessage>,
}

impl ProfilesStateClient {
    /// Production entry point: real `profiles.yaml` + injected legacy mirror.
    pub fn new(initial: Profiles, mirror: ProfilesMirror) -> anyhow::Result<Self> {
        let path = Utf8PathBuf::from_path_buf(dirs::profiles_path()?).map_err(|path| {
            anyhow::anyhow!("profiles config path is not UTF-8: {}", path.display())
        })?;
        Self::new_with_path(path, initial, mirror)
    }

    /// Test/internal entry point: explicit config path (e.g. a tempdir).
    pub(crate) fn new_with_path(
        config_path: Utf8PathBuf,
        initial: Profiles,
        mirror: ProfilesMirror,
    ) -> anyhow::Result<Self> {
        // Mirror `StateClient::new_with_path`: block_on initialization + spawn in a
        // synchronous context so the client can be constructed during Tauri setup.
        let manager = tauri::async_runtime::block_on(
            PersistentStateManagerSetup::<Profiles>::builder()
                .config_path(config_path)
                .config_prefix(PROFILES_CONFIG_PREFIX.to_string())
                .assemble()
                .from_state(initial),
        )
        .context("failed to initialize profiles persistent state manager")?;

        // Spawn anonymously: the client holds the `ActorRef` directly and never resolves
        // the actor by name, avoiding registry collisions across tests / re-spawns.
        let actor_ref = tauri::async_runtime::block_on(Actor::spawn(
            None,
            ProfilesStateActor,
            ProfilesStateActorArgs { manager, mirror },
        ))
        .context("failed to spawn nyanpasu profiles state actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ProfilesStateClientInner { actor_ref }),
        })
    }

    pub async fn get_profiles(&self) -> anyhow::Result<Profiles> {
        Ok(self
            .call(ProfilesStateMessage::GetProfiles, PROFILES_READ_TIMEOUT)
            .await?
            .state)
    }

    pub async fn commit(&self, state: Profiles) -> anyhow::Result<ProfilesStateSnapshot> {
        self.call(
            |reply| ProfilesStateMessage::CommitProfiles { state, reply },
            PROFILES_WRITE_TIMEOUT,
        )
        .await
    }

    async fn call<F>(&self, make: F, timeout: Duration) -> anyhow::Result<ProfilesStateSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<ProfilesStateSnapshot>>) -> ProfilesStateMessage,
    {
        match self.inner.actor_ref.call(make, Some(timeout)).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("profiles state actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("profiles state actor call timed out"),
        }
    }
}

impl Drop for ProfilesStateClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::{TempDir, tempdir};

    fn temp_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml"))
            .expect("temp path should be UTF-8")
    }

    fn capture_mirror() -> (ProfilesMirror, Arc<Mutex<Option<Profiles>>>) {
        let captured = Arc::new(Mutex::new(None::<Profiles>));
        let mirror_capture = captured.clone();
        let mirror: ProfilesMirror = Arc::new(move |state| {
            *mirror_capture
                .lock()
                .expect("mirror lock should not poison") = Some(state);
        });
        (mirror, captured)
    }

    fn test_client(
        initial: Profiles,
    ) -> (
        ProfilesStateClient,
        TempDir,
        Utf8PathBuf,
        Arc<Mutex<Option<Profiles>>>,
    ) {
        let dir = tempdir().expect("tempdir should be created");
        let path = temp_path(&dir);
        let (mirror, captured) = capture_mirror();
        let client = ProfilesStateClient::new_with_path(path.clone(), initial, mirror)
            .expect("profiles state client should be created");
        (client, dir, path, captured)
    }

    #[test]
    fn get_profiles_returns_initial_state() {
        let mut initial = Profiles::default();
        initial.set_current(vec!["seed".into()]);
        let (client, _dir, _path, _captured) = test_client(initial);

        tauri::async_runtime::block_on(async {
            let profiles = client.get_profiles().await.expect("get should succeed");
            assert_eq!(profiles.current, vec!["seed".to_string()]);
        });
    }

    #[test]
    fn commit_persists_mirrors_and_round_trips() {
        let (client, _dir, path, captured) = test_client(Profiles::default());

        tauri::async_runtime::block_on(async {
            let mut next = Profiles::default();
            next.set_current(vec!["active".into()]);
            let snapshot = client.commit(next).await.expect("commit should succeed");
            assert_eq!(snapshot.state.current, vec!["active".to_string()]);
            assert!(snapshot.version > 0);
        });

        let contents =
            std::fs::read_to_string(path.as_std_path()).expect("config should be written");
        assert!(contents.contains("# Profiles Config for Clash Nyanpasu"));

        // The actor's output parses back through the legacy reader (semantic round-trip).
        let parsed: Profiles = crate::utils::help::read_yaml(path.as_std_path())
            .expect("legacy reader should parse the actor output");
        assert_eq!(parsed.current, vec!["active".to_string()]);

        let mirrored = captured
            .lock()
            .expect("mirror lock should not poison")
            .clone()
            .expect("mirror should be called");
        assert_eq!(mirrored.current, vec!["active".to_string()]);
    }
}
