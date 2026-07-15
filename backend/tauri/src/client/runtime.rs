//! Runtime derived state (PR-4): the read model the facade holds after each
//! rebuild, plus the product/candidate config file locations. Runtime is a
//! pure derivation — there is no writable runtime state anywhere else.

use std::{
    fs::OpenOptions,
    io::Write,
    time::{Duration, SystemTime},
};

use camino::{Utf8Path, Utf8PathBuf};
use nyanpasu_core::state::{SimpleStateManager, SimpleStateManagerSetup};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use sha2::{Digest, Sha256};

use crate::{enhance::PostProcessingOutput, utils::path::PathResolver};

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG: &str = "clash-config.yaml";

/// Read model of the current runtime derivation (replaces the old
/// draft-based config type, minus the draft machinery). Derived once per
/// rebuild while the profiles snapshot is in hand; the four runtime read
/// commands serve straight from this.
///
/// Semantics (spec §5.1, r2): the latest TARGET config that passed the core
/// binary's check and was promoted to the product. It does NOT promise the
/// running core accepted it — a failed apply is reported as
/// `RebuildOutcome::Degraded`, not reflected here.
#[derive(Debug, Clone, Default)]
pub struct RuntimeState {
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
}

/// Facade-held runtime store. The RwLock is a narrowly scoped implementation
/// detail (CLAUDE.md §8 exception): `upsert` needs `&mut`, writers are already
/// serialized by the facade `rebuild_gate`, readers take `snapshot()`.
/// SimpleStateManager (not a bare RwLock<Option<..>>) is deliberate: its
/// StateCoordinator ack subscribers are the landing point for the
/// TODO(post-PR-7) ack-driven rollback direction (spec D2).
pub type RuntimeStateStore = tokio::sync::RwLock<SimpleStateManager<Option<RuntimeState>>>;

pub async fn new_runtime_state_store() -> anyhow::Result<RuntimeStateStore> {
    let manager = SimpleStateManagerSetup::builder()
        .initial_state(None)
        .assemble()
        .initialize()
        .await
        .map_err(|error| anyhow::anyhow!("failed to initialize runtime state store: {error:?}"))?;
    Ok(tokio::sync::RwLock::new(manager))
}

/// D6 (spec §6.4): previous values of the keys a clash patch touches, taken
/// from the published runtime state. Used to push the running core BACK when
/// the post-patch rebuild fails — the IPC applies the patch API-first, so a
/// failed rebuild would otherwise leave the core ahead of the persisted state.
pub(crate) fn compensation_for(patch: &Mapping, prev: Option<&Mapping>) -> Option<Mapping> {
    let prev = prev?;
    let comp: Mapping = patch
        .iter()
        .filter_map(|(k, _)| prev.get(k).map(|v| (k.clone(), v.clone())))
        .collect();
    (!comp.is_empty()).then_some(comp)
}

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    product: Utf8PathBuf,
    candidate_dir: Utf8PathBuf,
}

impl RuntimePaths {
    pub fn from_resolver(paths: &PathResolver) -> anyhow::Result<Self> {
        let runtime_dir = utf8_path(paths.app_config_dir().join(RUNTIME_CONFIG_DIR))?;
        Ok(Self {
            product: runtime_dir.join(RUNTIME_CONFIG),
            candidate_dir: runtime_dir.join(".candidates"),
        })
    }

    pub fn new(product: Utf8PathBuf, candidate_dir: Utf8PathBuf) -> Self {
        Self {
            product,
            candidate_dir,
        }
    }

    pub fn product(&self) -> &Utf8Path {
        &self.product
    }

    pub fn candidate_dir(&self) -> &Utf8Path {
        &self.candidate_dir
    }

    pub async fn create_candidate(&self, bytes: &[u8]) -> anyhow::Result<CandidateFile> {
        let names = (0..16)
            .map(|_| nanoid::nanoid!(16, &nanoid::alphabet::SAFE))
            .collect();
        self.create_candidate_with_names(bytes, names).await
    }

    async fn create_candidate_with_names(
        &self,
        bytes: &[u8],
        names: Vec<String>,
    ) -> anyhow::Result<CandidateFile> {
        prepare_private_dir(&self.candidate_dir).await?;
        let candidate_dir = self.candidate_dir.clone();
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            for name in names {
                let path = candidate_dir.join(format!("candidate-{name}.yaml"));
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&path) {
                    Ok(mut file) => {
                        file.write_all(&bytes)?;
                        file.sync_all()?;
                        let bytes_sha256 = Sha256::digest(&bytes).into();
                        return Ok(CandidateFile {
                            path,
                            bytes_sha256,
                            cleaned: false,
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error.into()),
                }
            }
            anyhow::bail!("failed to allocate a unique runtime candidate after 16 attempts")
        })
        .await?
    }

    pub async fn cleanup_stale_candidates(&self, max_age: Duration) -> anyhow::Result<usize> {
        prepare_private_dir(&self.candidate_dir).await?;
        let now = SystemTime::now();
        let mut removed = 0;
        let mut entries = tokio::fs::read_dir(&self.candidate_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("candidate-") {
                continue;
            }
            let metadata = tokio::fs::symlink_metadata(entry.path()).await?;
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                continue;
            }
            let is_stale = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= max_age);
            if is_stale {
                tokio::fs::remove_file(entry.path()).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[derive(Debug)]
pub struct CandidateFile {
    path: Utf8PathBuf,
    bytes_sha256: [u8; 32],
    cleaned: bool,
}

impl CandidateFile {
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub fn bytes_sha256(&self) -> [u8; 32] {
        self.bytes_sha256
    }

    pub async fn cleanup(mut self) -> anyhow::Result<()> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => {
                self.cleaned = true;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cleaned = true;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for CandidateFile {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

async fn prepare_private_dir(path: &Utf8Path) -> anyhow::Result<()> {
    if let Ok(metadata) = tokio::fs::symlink_metadata(path).await
        && is_symlink_or_reparse(&metadata)
    {
        anyhow::bail!("runtime candidate directory is a symlink or reparse point: {path}");
    }
    tokio::fs::create_dir_all(path).await?;
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
        anyhow::bail!("runtime candidate path is not a private directory: {path}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(())
}

#[cfg(unix)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn utf8_path(path: std::path::PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("runtime path is not UTF-8: {}", path.display()))
}

/// Post-commit rebuild result for mutation IPC (spec §6.2, decision D2):
/// state is committed first; a failed rebuild degrades instead of erroring.
// TODO(post-PR-7): degraded outcome is transitional. State managers already
// expose async commit acks; the end-state is ack-driven rollback when config
// application fails, replacing this degraded-report model. Tracked in
// actor-migration-roadmap §6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    Ok,
    Degraded { error: String },
}

impl RebuildOutcome {
    /// Combine sequential outcomes; the first degradation wins.
    pub fn merge(self, other: RebuildOutcome) -> RebuildOutcome {
        match self {
            RebuildOutcome::Degraded { .. } => self,
            RebuildOutcome::Ok => other,
        }
    }
}

/// Mutation payload + rebuild outcome for data-carrying commands (import).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CommitOutcome<T> {
    pub value: T,
    pub rebuild: RebuildOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compensation_restores_previous_values_of_patched_keys() {
        let mut prev = Mapping::new();
        prev.insert("mode".into(), "rule".into());
        prev.insert("allow-lan".into(), false.into());
        let mut patch = Mapping::new();
        patch.insert("mode".into(), "direct".into());
        patch.insert("ipv6".into(), true.into()); // prev 无该键 → 略过
        let comp = compensation_for(&patch, Some(&prev)).expect("some");
        assert_eq!(comp.get("mode"), Some(&"rule".into()));
        assert!(comp.get("ipv6").is_none());
        assert!(compensation_for(&patch, None).is_none());
    }

    /// S01 contract (task §S01.8 / design D6): compensation must be able to
    /// `Remove` keys that exist only on the running core (absent from Applied).
    ///
    /// Current failure reason: `compensation_for` only copies previous values
    /// for keys present in `prev` and silently drops brand-new patch keys, so
    /// API-first patches cannot be fully rolled back. It also reads the single
    /// published runtime store (Promoted semantics) rather than a distinct
    /// Applied snapshot.
    #[test]
    fn s01_contract_compensation_cannot_remove_keys_absent_from_applied() {
        let mut applied = Mapping::new();
        applied.insert("mode".into(), "rule".into());

        let mut patch = Mapping::new();
        patch.insert("mode".into(), "direct".into());
        // New key introduced by the API-first patch — Applied has no prior value.
        patch.insert("ipv6".into(), true.into());

        let comp = compensation_for(&patch, Some(&applied))
            .expect("compensation must exist when Applied has at least one overlapping key");

        assert_eq!(
            comp.get("mode"),
            Some(&"rule".into()),
            "existing Applied keys must be restored via Set"
        );

        // Desired (S05): brand-new keys produce an explicit Remove op so the
        // running core drops them. Current helper cannot express Remove at all.
        assert!(
            comp.contains_key("ipv6"),
            "S01 FAILURE reason: compensation cannot Remove keys absent from Applied \
             (new patch keys are dropped; helper only emits Set-from-prev and reads \
             the single promoted runtime store rather than Applied)"
        );
    }

    fn temp_runtime_paths(dir: &tempfile::TempDir) -> RuntimePaths {
        let root = Utf8PathBuf::from_path_buf(dir.path().join("runtime")).unwrap();
        RuntimePaths::new(root.join(RUNTIME_CONFIG), root.join(".candidates"))
    }

    #[test]
    fn runtime_paths_are_derived_from_injected_config_root() {
        let dir = tempfile::tempdir().unwrap();
        let resolver =
            PathResolver::with_base_dirs(dir.path().join("config"), dir.path().join("data"));
        let paths = RuntimePaths::from_resolver(&resolver).unwrap();
        assert_eq!(
            paths.product(),
            Utf8PathBuf::from_path_buf(
                dir.path()
                    .join("config")
                    .join(RUNTIME_CONFIG_DIR)
                    .join(RUNTIME_CONFIG),
            )
            .unwrap()
        );
        assert_eq!(
            paths.candidate_dir(),
            Utf8PathBuf::from_path_buf(
                dir.path()
                    .join("config")
                    .join(RUNTIME_CONFIG_DIR)
                    .join(".candidates"),
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn candidate_is_private_hashed_and_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_runtime_paths(&dir);
        let candidate = paths.create_candidate(b"mode: rule\n").await.unwrap();
        let path = candidate.path().to_owned();
        assert_eq!(
            candidate.bytes_sha256(),
            <[u8; 32]>::from(Sha256::digest(b"mode: rule\n"))
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"mode: rule\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                tokio::fs::metadata(paths.candidate_dir())
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                tokio::fs::metadata(&path)
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        drop(candidate);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn candidate_collision_retries_with_exclusive_create() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_runtime_paths(&dir);
        prepare_private_dir(paths.candidate_dir()).await.unwrap();
        let collision = paths.candidate_dir().join("candidate-collision.yaml");
        tokio::fs::write(&collision, b"do not replace")
            .await
            .unwrap();

        let candidate = paths
            .create_candidate_with_names(b"new bytes", vec!["collision".into(), "fresh".into()])
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(&collision).await.unwrap(),
            b"do not replace"
        );
        assert_eq!(candidate.path().file_name(), Some("candidate-fresh.yaml"));
    }

    #[tokio::test]
    async fn explicit_cleanup_and_stale_cleanup_remove_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_runtime_paths(&dir);
        let explicit = paths.create_candidate(b"explicit").await.unwrap();
        let explicit_path = explicit.path().to_owned();
        explicit.cleanup().await.unwrap();
        assert!(!explicit_path.exists());

        let stale = paths.create_candidate(b"stale").await.unwrap();
        let stale_path = stale.path().to_owned();
        std::mem::forget(stale);
        assert_eq!(
            paths
                .cleanup_stale_candidates(Duration::ZERO)
                .await
                .unwrap(),
            1
        );
        assert!(!stale_path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn candidate_directory_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let paths = temp_runtime_paths(&dir);
        let target = dir.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(paths.candidate_dir().parent().unwrap()).unwrap();
        symlink(target, paths.candidate_dir()).unwrap();

        let error = paths.create_candidate(b"blocked").await.unwrap_err();
        assert!(error.to_string().contains("symlink or reparse point"));
    }
}
