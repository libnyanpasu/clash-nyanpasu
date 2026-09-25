//! Runtime derived state (PR-4): the read model the facade holds after each
//! rebuild, plus the product/candidate config file locations. Runtime is a
//! pure derivation — there is no writable runtime state anywhere else.

use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};

use camino::{Utf8Path, Utf8PathBuf};
use nyanpasu_config::application::ClashCore;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use sha2::{Digest, Sha256};

use crate::{enhance::PostProcessingOutput, utils::path::PathResolver};

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG: &str = "clash-config.yaml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeRevision(u64);

impl RuntimeRevision {
    pub fn get(self) -> u64 {
        self.0
    }
}

pub(crate) struct RuntimeRevisionAllocator(u64);

impl RuntimeRevisionAllocator {
    pub(crate) fn new() -> Self {
        Self(0)
    }

    pub(crate) fn allocate(&mut self) -> anyhow::Result<RuntimeRevision> {
        self.0 = self
            .0
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("runtime revision space exhausted"))?;
        Ok(RuntimeRevision(self.0))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSnapshotData {
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
    pub(crate) inspection: Arc<super::runtime_inspection::RuntimeInspectionData>,
}

#[derive(Debug, Clone)]
pub struct RuntimeSnapshot {
    pub(crate) inspection_id: String,
    pub(crate) applied_binding: Option<crate::core::actor_v2::facade::AppliedConfigBinding>,
    pub(crate) effective_host: Option<(crate::core::actor_v2::endpoint::ExecutionHost, u64)>,
    pub(crate) effective: Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>,
    pub revision: RuntimeRevision,
    pub target_core: ClashCore,
    pub product_sha256: [u8; 32],
    product_bytes: Arc<[u8]>,
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
    pub(crate) inspection: Arc<super::runtime_inspection::RuntimeInspectionData>,
}

impl RuntimeSnapshot {
    pub(crate) fn from_data(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Arc<[u8]>,
        data: RuntimeSnapshotData,
    ) -> Self {
        let product_sha256 = Sha256::digest(&product_bytes).into();
        Self {
            inspection_id: nanoid::nanoid!(),
            applied_binding: None,
            effective: None,
            effective_host: None,
            revision,
            target_core,
            product_sha256,
            product_bytes,
            config: data.config,
            exists_keys: data.exists_keys,
            postprocessing_output: data.postprocessing_output,
            inspection: data.inspection,
        }
    }

    pub(crate) fn product_bytes(&self) -> &[u8] {
        &self.product_bytes
    }

    pub(crate) fn identity_eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.target_core == other.target_core
            && self.product_sha256 == other.product_sha256
    }
}

/// What the core actually accepted, recorded the moment it confirms an apply
/// and never derived from a file or an inspection (v2 §5.3/§5.4, C3).
///
/// It is deliberately decoupled from [`RuntimeSnapshot`]: `promoted` tracks a
/// derived product file and `applied` waits for an effective-config
/// inspection that may arrive late or not at all, so neither can serve as the
/// baseline a recovery has to restore. This can.
#[derive(Debug, Clone)]
pub(in crate::client) struct RuntimeApplyReceipt {
    /// Which build produced these bytes. Diagnostic: a restore targets the
    /// document, not the revision that happened to carry it.
    #[allow(dead_code)]
    pub revision: RuntimeRevision,
    /// The exact document the core accepted — the same bytes the validator
    /// checked, never a re-serialization of the snapshot.
    pub config_text: Arc<str>,
    /// `nyanpasu_core_manager::payload_digest` of `config_text`: the change
    /// identity the core verified on receipt.
    pub config_digest: String,
    pub target_core: ClashCore,
    pub core_spec: nyanpasu_core_manager::CoreSpec,
    pub host: crate::core::actor_v2::endpoint::ExecutionHost,
    /// Whether the app wants this runtime running at all. A core the user
    /// stopped is a target too, and a recovery must not start it.
    pub run_intent: super::application_workflow::policy::CoreRunIntent,
    pub local_ipc: nyanpasu_core_manager::LocalIpcSettings,
    /// The binding this apply produced. A recovery may legitimately land on a
    /// newer instance generation, so only its content identity is a target.
    pub binding: crate::core::actor_v2::facade::AppliedConfigBinding,
    /// The ports this apply bound. Confirming them is gated on this receipt.
    pub ports: super::ports::CandidatePortBindings,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeLifecycleState {
    pub promoted: Option<Arc<RuntimeSnapshot>>,
    pub applied: Option<Arc<RuntimeSnapshot>>,
    /// A successful application whose effective inspection is not yet available.
    pub(crate) pending: Option<Arc<RuntimeSnapshot>>,
}

#[derive(Debug, Clone)]
pub(crate) enum InspectionState {
    Pending,
    Ready,
    Unavailable,
}

#[derive(Debug, Clone)]
pub(in crate::client) struct ConfirmedRuntime {
    pub receipt: Arc<RuntimeApplyReceipt>,
    pub artifact: Option<Arc<RuntimeSnapshot>>,
    pub inspection: InspectionState,
    pub available: bool,
}

#[derive(Debug, Clone, Default)]
struct RuntimeStoreState {
    promoted: Option<Arc<RuntimeSnapshot>>,
    confirmed: Option<ConfirmedRuntime>,
    transition: bool,
}

/// One associated record publishes the artifact, receipt and inspection state.
#[derive(Clone)]
pub(crate) struct RuntimeSnapshotStore(tokio::sync::watch::Sender<RuntimeStoreState>);

impl Default for RuntimeSnapshotStore {
    fn default() -> Self {
        Self(tokio::sync::watch::Sender::new(RuntimeStoreState::default()))
    }
}

impl RuntimeSnapshotStore {
    pub(crate) fn read(&self) -> RuntimeLifecycleState {
        let state = self.0.borrow();
        let confirmed = state.confirmed.as_ref();
        RuntimeLifecycleState {
            promoted: state.promoted.clone(),
            applied: confirmed
                .filter(|c| c.available && matches!(c.inspection, InspectionState::Ready))
                .and_then(|c| c.artifact.clone()),
            pending: confirmed
                .filter(|c| c.available && matches!(c.inspection, InspectionState::Pending))
                .and_then(|c| c.artifact.clone()),
        }
    }

    pub(crate) fn generated(&self, snapshot: Arc<RuntimeSnapshot>) {
        self.0.send_modify(|state| state.promoted = Some(snapshot));
    }

    pub(in crate::client) fn generated_confirmed(&self, product: Arc<RuntimeSnapshot>) {
        self.0.send_modify(|state| {
            let inspected = state.confirmed.as_ref().and_then(|record| {
                matches!(record.inspection, InspectionState::Ready)
                    .then(|| record.artifact.clone())
                    .flatten()
            });
            state.promoted = Some(
                inspected
                    .filter(|artifact| {
                        artifact.identity_eq(&product)
                            && artifact.applied_binding == product.applied_binding
                    })
                    .unwrap_or(product),
            );
        });
    }

    pub(in crate::client) fn record_confirmed_apply(
        &self,
        artifact: Option<Arc<RuntimeSnapshot>>,
        receipt: Arc<RuntimeApplyReceipt>,
    ) {
        let artifact = artifact.map(|artifact| {
            let mut rebound = artifact.without_effective_config();
            rebound.applied_binding = Some(receipt.binding.clone());
            Arc::new(rebound)
        });
        let inspection = if artifact.is_some() {
            InspectionState::Pending
        } else {
            InspectionState::Unavailable
        };
        self.0.send_modify(|state| {
            state.confirmed = Some(ConfirmedRuntime {
                receipt,
                artifact,
                inspection,
                available: true,
            })
        });
    }

    pub(in crate::client) fn begin_transition(&self) {
        self.0.send_modify(|state| state.transition = true);
    }

    pub(in crate::client) fn accept_transition(&self) {
        self.0.send_modify(|state| {
            state.transition = false;
        });
    }

    pub(in crate::client) fn confirmed(&self) -> Option<ConfirmedRuntime> {
        self.0.borrow().confirmed.clone()
    }

    pub(in crate::client) fn accepted_binding(&self) -> Option<Arc<RuntimeApplyReceipt>> {
        let state = self.0.borrow();
        (!state.transition)
            .then(|| state.confirmed.as_ref())
            .flatten()
            .filter(|record| record.available)
            .map(|record| record.receipt.clone())
    }

    pub(in crate::client) fn last_confirmed_runtime_receipt(
        &self,
    ) -> Option<Arc<RuntimeApplyReceipt>> {
        self.confirmed().map(|record| record.receipt)
    }

    pub(crate) fn invalidate(&self) {
        self.0.send_modify(|state| {
            if let Some(record) = &mut state.confirmed {
                record.available = false;
            }
        });
    }

    pub(crate) fn applied(&self, source_id: &str, snapshot: Arc<RuntimeSnapshot>) {
        self.0.send_modify(|state| {
            let Some(record) = &mut state.confirmed else {
                return;
            };
            if !record.available
                || !record
                    .artifact
                    .as_ref()
                    .is_some_and(|artifact| artifact.inspection_id == source_id)
                || snapshot.applied_binding.as_ref() != Some(&record.receipt.binding)
            {
                return;
            }
            if state
                .promoted
                .as_ref()
                .is_some_and(|source| source.inspection_id == source_id)
            {
                state.promoted = Some(snapshot.clone());
            }
            record.artifact = Some(snapshot);
            record.inspection = InspectionState::Ready;
        });
    }

    #[cfg(test)]
    pub(crate) fn bind_applied(&self, snapshot: Arc<RuntimeSnapshot>) {
        let Some(binding) = &snapshot.applied_binding else {
            return;
        };
        let mut receipt = tests::receipt(snapshot.revision.get());
        receipt.binding = binding.clone();
        self.record_confirmed_apply(Some(snapshot), Arc::new(receipt));
    }

    #[cfg(test)]
    pub(in crate::client) fn confirm_applied(&self, receipt: Arc<RuntimeApplyReceipt>) {
        self.record_confirmed_apply(None, receipt);
    }
}

pub(crate) async fn write_product(product: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = product.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let product = product.to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        atomicwrites::AtomicFile::new(&product, atomicwrites::OverwriteBehavior::AllowOverwrite)
            .write(|file| std::io::Write::write_all(file, &bytes))
    })
    .await?
    .map_err(|error| anyhow::anyhow!("failed to promote runtime config: {error}"))?;
    Ok(())
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

    #[allow(dead_code)]
    pub fn new(product: Utf8PathBuf, candidate_dir: Utf8PathBuf) -> Self {
        Self {
            product,
            candidate_dir,
        }
    }

    pub fn product(&self) -> &Utf8Path {
        &self.product
    }

    #[allow(dead_code)]
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

/// A source commit and its critical runtime result. Peripheral owners settle separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CommitReceipt {
    pub operation_id: Option<String>,
    pub domain: String,
    pub source_version: u64,
    pub runtime: RuntimeCommitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommitStatus {
    Applied,
    Deferred,
    SavedInactive,
    Unchanged,
    Pending,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Committed {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
    },
    CommittedDegraded {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
        degradations: Vec<Degradation>,
    },
}

impl<T> MutationOutcome<T> {
    pub fn from_parts(value: T, degradations: Vec<Degradation>) -> Self {
        if degradations.is_empty() {
            Self::Committed {
                value,
                commits: Vec::new(),
                notifications_pending: true,
            }
        } else {
            Self::CommittedDegraded {
                value,
                commits: Vec::new(),
                notifications_pending: true,
                degradations,
            }
        }
    }

    pub fn with_commit(mut self, receipt: CommitReceipt) -> Self {
        match &mut self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.push(receipt)
            }
        }
        self
    }

    pub fn append_commit_result(self, result: MutationOutcome<()>) -> Self {
        let commits = match &result {
            MutationOutcome::Committed { commits, .. }
            | MutationOutcome::CommittedDegraded { commits, .. } => commits.clone(),
        };
        let mut outcome = self.extend_degradations(result.into_parts().1);
        for commit in commits {
            outcome = outcome.with_commit(commit);
        }
        outcome
    }

    pub fn value(&self) -> &T {
        match self {
            Self::Committed { value, .. } | Self::CommittedDegraded { value, .. } => value,
        }
    }

    pub fn into_value(self) -> T {
        self.into_parts().0
    }

    pub fn degradations(&self) -> &[Degradation] {
        match self {
            Self::Committed { .. } => &[],
            Self::CommittedDegraded { degradations, .. } => degradations,
        }
    }

    pub fn into_parts(self) -> (T, Vec<Degradation>) {
        match self {
            Self::Committed { value, .. } => (value, Vec::new()),
            Self::CommittedDegraded {
                value,
                degradations,
                ..
            } => (value, degradations),
        }
    }

    pub fn extend_degradations(self, extra: Vec<Degradation>) -> Self {
        let commits = match &self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.clone()
            }
        };
        let (value, mut degradations) = self.into_parts();
        degradations.extend(extra);
        let mut outcome = Self::from_parts(value, degradations);
        for receipt in commits {
            outcome = outcome.with_commit(receipt);
        }
        outcome
    }
}

/// Structured committed-degraded detail surfaced over IPC / Specta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Degradation {
    pub phase: DegradationPhase,
    /// Stable snake_case code string (not a free-form English phrase).
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Public degradation phases for mutation outcomes. Serde/Specta use snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DegradationPhase {
    LegacyMirror,
    ProfileMaterialization,
    RuntimeBuild,
    RuntimeCheck,
    RuntimePromote,
    RuntimePublish,
    RuntimeApply,
    CoreRollback,
    SystemEffect,
    UiEffect,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A revision value for fixtures that need one but do not exercise the
    /// allocator.
    pub(crate) fn test_revision() -> RuntimeRevision {
        RuntimeRevision(1)
    }

    #[test]
    fn runtime_revision_allocator_is_monotonic() {
        let mut allocator = RuntimeRevisionAllocator::new();
        let first = allocator.allocate().expect("first revision");
        let second = allocator.allocate().expect("second revision");

        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        assert!(second > first);
    }

    #[test]
    fn mutation_outcome_applied_iff_degradations_empty() {
        let applied = MutationOutcome::from_parts("uid", Vec::new());
        assert!(
            matches!(applied, MutationOutcome::Committed { .. }),
            "empty degradations must be Applied"
        );
        assert_eq!(applied.value(), &"uid");

        let degraded = MutationOutcome::from_parts(
            "uid",
            vec![Degradation {
                phase: DegradationPhase::RuntimeBuild,
                code: "runtime_rebuild_failed".into(),
                message: "boom".into(),
                retryable: true,
            }],
        );
        assert!(
            matches!(degraded, MutationOutcome::CommittedDegraded { .. }),
            "non-empty degradations must be CommittedDegraded"
        );
        assert_eq!(degraded.degradations().len(), 1);

        let merged =
            MutationOutcome::from_parts((), Vec::new()).extend_degradations(vec![Degradation {
                phase: DegradationPhase::ProfileMaterialization,
                code: "cleanup_deferred".into(),
                message: "left behind".into(),
                retryable: true,
            }]);
        assert!(
            matches!(merged, MutationOutcome::CommittedDegraded { .. }),
            "extend_degradations with extra must be CommittedDegraded"
        );
        assert_eq!(merged.degradations()[0].code, "cleanup_deferred");
    }

    #[test]
    fn degradation_phase_serde_is_snake_case() {
        let json = serde_json::to_string(&DegradationPhase::ProfileMaterialization).unwrap();
        assert_eq!(json, "\"profile_materialization\"");
        let json = serde_json::to_string(&DegradationPhase::RuntimeApply).unwrap();
        assert_eq!(json, "\"runtime_apply\"");
    }

    #[test]
    fn mutation_outcome_wire_uses_applied_and_committed_degraded() {
        let applied = MutationOutcome::from_parts((), Vec::new());
        let applied_json = serde_json::to_value(&applied).unwrap();
        assert_eq!(applied_json["status"], "committed");
        assert!(applied_json.get("value").is_some());

        let degraded = MutationOutcome::from_parts(
            "p1",
            vec![Degradation {
                phase: DegradationPhase::RuntimeBuild,
                code: "runtime_rebuild_failed".into(),
                message: "check boom".into(),
                retryable: true,
            }],
        );
        let degraded_json = serde_json::to_value(&degraded).unwrap();
        assert_eq!(degraded_json["status"], "committed_degraded");
        assert_eq!(degraded_json["value"], "p1");
        assert_eq!(
            degraded_json["degradations"][0]["code"],
            "runtime_rebuild_failed"
        );
        assert_eq!(degraded_json["degradations"][0]["phase"], "runtime_build");
        assert_eq!(degraded_json["degradations"][0]["retryable"], true);
    }

    #[tokio::test]
    async fn write_product_creates_the_runtime_directory() {
        let dir = tempfile::tempdir().unwrap();
        let product = dir.path().join("nested/runtime.yaml");
        write_product(&product, b"mode: rule\n").await.unwrap();
        assert_eq!(tokio::fs::read(product).await.unwrap(), b"mode: rule\n");
    }

    #[tokio::test]
    async fn write_product_replaces_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let product = dir.path().join("runtime.yaml");
        write_product(&product, b"mode: rule\n").await.unwrap();
        write_product(&product, b"mode: direct\n").await.unwrap();
        assert_eq!(tokio::fs::read(product).await.unwrap(), b"mode: direct\n");
    }

    #[test]
    fn runtime_lifecycle_store_tracks_only_the_promoted_product() {
        let lifecycle = RuntimeLifecycleState::default();
        assert!(lifecycle.promoted.is_none());
    }

    fn binding() -> crate::core::actor_v2::facade::AppliedConfigBinding {
        crate::core::actor_v2::facade::AppliedConfigBinding {
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 3,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            },
            host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
            generation: 0,
        }
    }

    fn snapshot(revision: u64) -> Arc<RuntimeSnapshot> {
        Arc::new(RuntimeSnapshot::from_data(
            RuntimeRevision(revision),
            ClashCore::default(),
            Arc::from(&b"mode: rule\n"[..]),
            RuntimeSnapshotData {
                config: Mapping::new(),
                exists_keys: Vec::new(),
                postprocessing_output: PostProcessingOutput::default(),
                inspection: Arc::new(super::super::runtime_inspection::tests::inspection_data()),
            },
        ))
    }

    /// v2 §5.6: the public runtime YAML is a derived product. An apply the
    /// core confirmed is a fact about the core, so a product that was never
    /// published must not erase it.
    #[test]
    fn an_apply_binds_even_when_no_product_was_promoted() {
        let store = RuntimeSnapshotStore::default();
        let mut bound = snapshot(1).as_ref().clone();
        bound.applied_binding = Some(binding());
        let bound = Arc::new(bound);

        store.bind_applied(bound.clone());

        let state = store.read();
        assert!(
            state.promoted.is_none(),
            "nothing was published, so nothing is promoted"
        );
        assert_eq!(
            state.pending.as_ref().map(|s| s.inspection_id.as_str()),
            Some(bound.inspection_id.as_str()),
            "the apply is recorded regardless of the product file"
        );
    }

    #[test]
    fn binding_a_snapshot_the_core_never_applied_records_nothing() {
        let store = RuntimeSnapshotStore::default();
        store.bind_applied(snapshot(1));
        assert!(store.read().pending.is_none());
    }

    /// V09 at the store level: the recovery baseline follows the confirmed
    /// apply, never the inspection. `applied` stays empty while the receipt
    /// advances twice.
    #[test]
    fn the_recovery_baseline_advances_with_each_confirmed_apply() {
        let store = RuntimeSnapshotStore::default();
        assert!(store.last_confirmed_runtime_receipt().is_none());

        let first = Arc::new(receipt(1));
        store.confirm_applied(first.clone());
        let second = Arc::new(receipt(2));
        store.confirm_applied(second);

        assert_eq!(
            store
                .last_confirmed_runtime_receipt()
                .expect("a confirmed apply")
                .revision
                .get(),
            2,
        );
        assert!(
            store.read().applied.is_none(),
            "no inspection arrived, and the baseline did not wait for one"
        );
        assert_eq!(first.revision.get(), 1);
    }

    pub(super) fn receipt(revision: u64) -> RuntimeApplyReceipt {
        RuntimeApplyReceipt {
            revision: RuntimeRevision(revision),
            config_text: Arc::from("mode: rule\n"),
            config_digest: nyanpasu_core_manager::payload_digest(b"mode: rule\n"),
            target_core: ClashCore::default(),
            core_spec: nyanpasu_core_manager::CoreSpec {
                kind: nyanpasu_core_manager::CoreKind::Mihomo,
                binary_path: camino::Utf8PathBuf::from("fake-core"),
                version: None,
                features: Vec::new(),
            },
            host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
            run_intent: crate::client::application_workflow::policy::CoreRunIntent::Running,
            local_ipc: nyanpasu_core_manager::LocalIpcSettings {
                policy: nyanpasu_core_manager::LocalIpcPolicy::Disable,
                keep_http_controller: true,
            },
            binding: binding(),
            ports: crate::client::ports::SessionPortResolver::default()
                .resolve_candidate(&nyanpasu_config::clash::config::ClashConfig::default())
                .expect("default port strategies resolve"),
        }
    }

    #[test]
    fn runtime_snapshot_identity_uses_revision_core_and_product_hash() {
        let data = || RuntimeSnapshotData {
            config: Mapping::new(),
            exists_keys: Vec::new(),
            postprocessing_output: PostProcessingOutput::default(),
            inspection: Arc::new(super::super::runtime_inspection::tests::inspection_data()),
        };
        let first = RuntimeSnapshot::from_data(
            RuntimeRevision(1),
            ClashCore::default(),
            Arc::from(&b"mode: rule\n"[..]),
            data(),
        );
        let same = RuntimeSnapshot::from_data(
            RuntimeRevision(1),
            ClashCore::default(),
            Arc::from(&b"mode: rule\n"[..]),
            data(),
        );
        assert!(first.identity_eq(&same));
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
