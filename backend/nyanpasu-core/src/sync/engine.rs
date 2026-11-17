//! One sync cycle: scan, fetch, plan, download, apply, upload, commit.
//!
//! The engine is a value, not an actor: the actor owns the settings, the
//! status and the one-cycle-at-a-time rule, and calls this with them. Every
//! phase is safe to abandon because planning is by content, not by remembered
//! intent, so a crash simply replans on the next cycle.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use nyanpasu_config::sync::{
    ConflictKind, ObjectId, PortableDocument, canonical_text, project_application, project_clash,
    project_profiles,
};
use nyanpasu_contracts::sync::{PutCondition, RemoteError, RemoteStore, RemoteStoreFactory};
use nyanpasu_paths::{ItemCodec, LogicalPath, SyncEnumeration, SyncItem};
use nyanpasu_runtime_values::sync::{
    BlockedItem, SyncBlockedReason, SyncConflict, SyncConflictKind, SyncOutcome, SyncPhase,
    SyncProgress, SyncStatus,
};
use snafu::{ResultExt as _, Snafu};

use super::{
    commit::{
        ChangedAt, CommitCodec, CommitDevice, CommitItem, CommitManifest, CommitNaming, DeviceId,
        LiveItem, VaultEncryption, VaultFormat, VaultId,
    },
    plan::{ItemAction, Plan, PortableState, SideItem, plan, remote_state},
    ports::{APPLICATION_ITEM, ApplyError, CLASH_ITEM, PROFILES_ITEM, SyncPorts},
    state::{BaselineItem, SyncMeta, baseline_state},
};

/// The directory a vault occupies below the provider root.
const VAULT_DIRECTORY: &str = "nyanpasu-sync";
const FORMAT_KEY: &str = "format.json";
const COMMITS_PREFIX: &str = "commits/";
const OBJECTS_PREFIX: &str = "objects/";

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum CycleError {
    #[snafu(display("the remote is unreachable"))]
    Remote { source: RemoteError },
    #[snafu(display("failed to read or write the sync baseline"))]
    State { source: super::state::StateError },
    #[snafu(display("failed to read or apply a local item"))]
    Apply { source: ApplyError },
    #[snafu(display("failed to plan the cycle"))]
    Plan { source: super::plan::PlanError },
    #[snafu(display("the vault was written by a newer protocol"))]
    VaultTooNew { found: u32, supported: u32 },
    #[snafu(display("the vault belongs to a different baseline"))]
    VaultMismatch { found: String, expected: String },
    #[snafu(display("failed to read or write a commit"))]
    Commit { source: super::commit::CommitError },
    #[snafu(display("the cycle was cancelled"))]
    Cancelled,
    #[snafu(display("failed to project or parse a configuration document"))]
    Document {
        source: nyanpasu_config::sync::DocumentError,
    },
    #[snafu(display("failed to read or write an object id"))]
    ObjectId {
        source: nyanpasu_config::sync::CanonicalError,
    },
}

/// What one cycle did, for the report and the status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleReport {
    pub outcome: Option<SyncOutcome>,
    pub pushed: u32,
    pub pulled: u32,
    pub merged: u32,
    pub blocked: Vec<BlockedItem>,
    pub conflicts: Vec<SyncConflict>,
    pub head: Option<nyanpasu_runtime_values::sync::SyncHead>,
}

/// One cycle's inputs, all owned by the actor that calls it.
pub struct CycleInput<'a> {
    pub ports: &'a SyncPorts,
    pub store: Arc<dyn RemoteStore>,
    pub meta: &'a SyncMeta,
    pub settings: &'a nyanpasu_contracts::sync::SyncSettings,
    pub status: &'a mut SyncStatus,
}

/// Runs one cycle and returns what it did.
///
/// # Errors
///
/// [`CycleError`] when a phase cannot complete; the caller reports it as the
/// cycle's failure and leaves the baseline untouched.
pub async fn run_cycle(input: CycleInput<'_>) -> Result<CycleReport, CycleError> {
    let CycleInput {
        ports,
        store,
        meta,
        settings,
        status,
    } = input;
    if ports.shutdown.is_cancelled() {
        return Err(CycleError::Cancelled);
    }
    let policy = settings.conflict_policy;
    status.phase = SyncPhase::Scanning;
    let mut report = CycleReport::default();
    let loaded = ports.state.load().context(StateSnafu)?;
    let resolutions = resolution_map(&loaded.conflicts);
    let scan = scan(ports).await?;
    report.blocked.clone_from(&scan.blocked);
    status.blocked_items.clone_from(&report.blocked);

    status.phase = SyncPhase::Fetching;
    let head = fetch_head(store.as_ref(), &loaded.meta).await?;
    let remote_head = remote_state(head.as_ref());
    let mut remote = remote_head.clone();
    // A document merge needs both projections, including the remote one. A
    // device joining a vault has no baseline document, so it must fetch the
    // head's documents here rather than treat them as opaque files.
    let remote_documents = fetch_remote_documents(store.as_ref(), head.as_ref()).await?;
    for (logical, document) in &remote_documents {
        if let Some(side) = remote.get_mut(logical) {
            side.document = Some(document.clone());
        }
    }
    // A pair is needed for every document both sides hold; an item only one
    // side holds is a plain push or pull and needs no merge.
    let documents = scan
        .documents
        .iter()
        .filter_map(|(logical, local)| {
            remote_documents
                .get(logical)
                .map(|remote| (logical.clone(), (local.clone(), remote.clone())))
        })
        .collect::<BTreeMap<_, _>>();

    status.phase = SyncPhase::Planning;
    let baseline_state = baseline_state(&loaded.baseline);
    let pinned = pinned(&documents);
    let plan = plan(super::plan::PlanInput {
        baseline: &baseline_state,
        local: &scan.local,
        remote: &remote,
        documents: &documents,
        base_documents: &loaded.baseline_documents,
        policy,
        pinned: &pinned,
        resolutions: &resolutions,
    })
    .context(PlanSnafu)?;
    report.conflicts = conflicts_from(&plan);
    status.open_conflicts = report.conflicts.len() as u32;
    if plan.items.is_empty() && report.blocked.is_empty() {
        report.outcome = Some(SyncOutcome::UpToDate);
        status.phase = SyncPhase::Idle;
        status.last_result = report.outcome;
        return Ok(report);
    }
    status.progress = Some(SyncProgress {
        done: 0,
        total: plan.items.len() as u32,
    });

    status.phase = SyncPhase::Snapshotting;
    let changed: Vec<_> = plan
        .items
        .iter()
        .filter(|item| matches!(item.action, ItemAction::Pull | ItemAction::Merge))
        .map(|item| item.logical.clone())
        .collect();
    if !changed.is_empty() {
        ports.snapshots.capture(&changed).context(ApplySnafu)?;
    }

    status.phase = SyncPhase::Downloading;
    let downloaded = download(
        store.as_ref(),
        &plan,
        &remote_head,
        &scan.local,
        &mut report,
    )
    .await?;

    // A merge result is new content on both sides: it is applied locally and
    // uploaded as a new object. Computing it once keeps the two in step.
    let merged = merged_objects(&plan, &downloaded, &scan);

    status.phase = SyncPhase::Applying;
    apply_local(ports, &plan, &downloaded, &merged, &scan, &mut report).await?;

    status.phase = SyncPhase::Uploading;
    upload(store.as_ref(), &plan, &scan.objects, &merged).await?;

    status.phase = SyncPhase::Committing;
    let commit = commit(
        store.as_ref(),
        meta,
        settings,
        CommitInput {
            plan: &plan,
            objects: &scan.objects,
            merged: &merged,
            codecs: &scan.codecs,
            head: head.as_ref(),
        },
    )
    .await?;
    report.head = Some(super::status::head_of(&commit));
    status.head = report.head.clone();

    status.phase = SyncPhase::Confirming;
    let confirmed = list_commits(store.as_ref()).await?;
    let settled = confirmed.iter().any(|candidate| candidate.id == commit.id);
    status.phase = SyncPhase::Finalizing;
    advance_baseline(ports, &plan, &commit, &loaded, settled, &scan.codecs)?;
    ports
        .state
        .save_conflicts(&report.conflicts)
        .context(StateSnafu)?;

    report.pushed = count(&plan, ItemAction::Push);
    report.pulled = count(&plan, ItemAction::Pull);
    report.merged = count(&plan, ItemAction::Merge);
    report.outcome = Some(if report.blocked.is_empty() {
        SyncOutcome::Synced
    } else {
        SyncOutcome::Degraded
    });
    status.phase = SyncPhase::Idle;
    status.progress = None;
    status.last_result = report.outcome;
    Ok(report)
}

fn count(plan: &Plan, action: ItemAction) -> u32 {
    plan.items
        .iter()
        .filter(|item| item.action == action)
        .count() as u32
}

struct Scan {
    local: PortableState,
    /// Every document this device holds, keyed by logical path.
    documents: BTreeMap<LogicalPath, PortableDocument>,
    /// The canonical bytes of every local item, keyed by logical path.
    objects: BTreeMap<LogicalPath, Vec<u8>>,
    /// The codec of every enumerated file, for commit and baseline rows.
    codecs: BTreeMap<LogicalPath, ItemCodec>,
    blocked: Vec<BlockedItem>,
}

async fn scan(ports: &SyncPorts) -> Result<Scan, CycleError> {
    let application = ports.portable.application().await.context(ApplySnafu)?;
    let clash = ports.portable.clash().await.context(ApplySnafu)?;
    let profiles = ports.portable.profiles().await.context(ApplySnafu)?;
    let selected: BTreeSet<LogicalPath> =
        nyanpasu_config::sync::portable_profile_files(&profiles.state)
            .into_iter()
            .filter_map(|file| {
                LogicalPath::parse("profiles")
                    .and_then(|base| base.join(&file))
                    .ok()
            })
            .collect();
    let enumeration: SyncEnumeration =
        ports
            .paths
            .enumerate_sync_items(&selected)
            .map_err(|error| CycleError::Apply {
                source: ApplyError::File {
                    path: String::new(),
                    reason: error.to_string(),
                },
            })?;

    let mut local = PortableState::new();
    let mut documents = BTreeMap::new();
    let mut objects = BTreeMap::new();
    let mut codecs = BTreeMap::new();
    let projections = [
        (
            APPLICATION_ITEM,
            project_application(&application.state).context(DocumentSnafu)?,
        ),
        (
            CLASH_ITEM,
            project_clash(&clash.state).context(DocumentSnafu)?,
        ),
        (
            PROFILES_ITEM,
            project_profiles(&profiles.state).context(DocumentSnafu)?,
        ),
    ];
    for (key, document) in projections {
        let logical = fixed_key(key);
        let bytes = document.to_canonical_bytes();
        objects.insert(logical.clone(), bytes);
        codecs.insert(logical.clone(), scan_codec(&logical));
        local.insert(logical.clone(), SideItem::document(document.clone()));
        documents.insert(logical, document);
    }
    let mut blocked = blocked_from(&enumeration);
    for item in &enumeration.items {
        match read_item(ports, item) {
            Ok(Some(bytes)) => {
                local.insert(
                    item.logical.clone(),
                    SideItem::present(ObjectId::of(&bytes).as_str()),
                );
                codecs.insert(item.logical.clone(), item.codec);
                objects.insert(item.logical.clone(), bytes);
            }
            Ok(None) => {}
            Err(reason) => blocked.push(BlockedItem {
                item: item.logical.to_string(),
                reason,
            }),
        }
    }
    Ok(Scan {
        local,
        documents,
        objects,
        codecs,
        blocked,
    })
}

/// The canonical bytes of one local file, or `None` when it is absent.
fn read_item(ports: &SyncPorts, item: &SyncItem) -> Result<Option<Vec<u8>>, SyncBlockedReason> {
    let bytes = match item.codec {
        ItemCodec::Text => ports.files.read_text(&item.logical),
        ItemCodec::Binary | ItemCodec::Document(_) => ports.files.read_binary(&item.logical),
    }
    .map_err(|_| SyncBlockedReason::UnportableName)?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    match item.codec {
        ItemCodec::Text => canonical_text(&bytes)
            .map(Some)
            .map_err(|_| SyncBlockedReason::NotUtf8),
        ItemCodec::Binary | ItemCodec::Document(_) => Ok(Some(bytes)),
    }
}

async fn fetch_head(
    store: &dyn RemoteStore,
    meta: &SyncMeta,
) -> Result<Option<CommitManifest>, CycleError> {
    match store.get(FORMAT_KEY).await {
        Ok(bytes) => {
            let format = VaultFormat::parse(&bytes).context(CommitSnafu)?;
            if let Some(expected) = &meta.vault_id
                && expected != &format.vault_id
            {
                return Err(CycleError::VaultMismatch {
                    found: format.vault_id.as_str().to_owned(),
                    expected: expected.as_str().to_owned(),
                });
            }
            if format.format > super::commit::COMMIT_FORMAT {
                return Err(CycleError::VaultTooNew {
                    found: format.format,
                    supported: super::commit::COMMIT_FORMAT,
                });
            }
        }
        Err(RemoteError::NotFound { .. }) => return Ok(None),
        Err(source) => return Err(CycleError::Remote { source }),
    }
    Ok(list_commits(store).await?.into_iter().next())
}

/// The documents a head commit references, decoded.
async fn fetch_remote_documents(
    store: &dyn RemoteStore,
    head: Option<&CommitManifest>,
) -> Result<BTreeMap<LogicalPath, PortableDocument>, CycleError> {
    let mut documents = BTreeMap::new();
    let Some(head) = head else {
        return Ok(documents);
    };
    for logical in document_keys() {
        let Some(CommitItem::Live(item)) = head.items.get(&logical) else {
            continue;
        };
        if !matches!(item.codec, CommitCodec::Document) {
            continue;
        }
        let bytes = store
            .get(&object_key(item.sha256.as_str()))
            .await
            .map_err(|source| CycleError::Remote { source })?;
        if let Ok(document) = PortableDocument::from_bytes(&bytes) {
            documents.insert(logical, document);
        }
    }
    Ok(documents)
}

async fn list_commits(store: &dyn RemoteStore) -> Result<Vec<CommitManifest>, CycleError> {
    let entries = store
        .list(COMMITS_PREFIX)
        .await
        .map_err(|source| CycleError::Remote { source })?;
    let mut commits = Vec::new();
    for entry in entries {
        let bytes = store
            .get(&entry.key)
            .await
            .map_err(|source| CycleError::Remote { source })?;
        // A torn or foreign object fails verification and is ignored.
        if let Ok(commit) = CommitManifest::parse(&bytes) {
            commits.push(commit);
        }
    }
    commits.sort_by(|left, right| {
        right
            .generation
            .cmp(&left.generation)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(commits)
}

async fn download(
    store: &dyn RemoteStore,
    plan: &Plan,
    remote_head: &PortableState,
    local: &PortableState,
    report: &mut CycleReport,
) -> Result<BTreeMap<LogicalPath, Vec<u8>>, CycleError> {
    let mut downloaded: BTreeMap<LogicalPath, Vec<u8>> = BTreeMap::new();
    for item in &plan.items {
        if !matches!(item.action, ItemAction::Pull | ItemAction::Merge) {
            continue;
        }
        // The object to fetch is what the head holds, not a merge result this
        // device has not uploaded yet.
        let Some(sha) = remote_head
            .get(&item.logical)
            .and_then(|side| side.sha256.as_ref())
        else {
            continue;
        };
        if local
            .get(&item.logical)
            .and_then(|side| side.sha256.as_ref())
            == Some(sha)
        {
            continue;
        }
        let bytes = store
            .get(&object_key(sha))
            .await
            .map_err(|source| CycleError::Remote { source })?;
        if ObjectId::of(&bytes).as_str() != sha {
            report.blocked.push(BlockedItem {
                item: item.logical.to_string(),
                reason: SyncBlockedReason::NotUtf8,
            });
            continue;
        }
        downloaded.insert(item.logical.clone(), bytes.to_vec());
    }
    Ok(downloaded)
}

/// The bytes of each merged item: the local value wins for an open conflict,
/// the merged value otherwise.
fn merged_objects(
    plan: &Plan,
    downloaded: &BTreeMap<LogicalPath, Vec<u8>>,
    scan: &Scan,
) -> BTreeMap<LogicalPath, Vec<u8>> {
    let mut merged = BTreeMap::new();
    for item in &plan.items {
        if item.action != ItemAction::Merge {
            continue;
        }
        if let Some(document) = &item.remote.document {
            merged.insert(item.logical.clone(), document.to_canonical_bytes());
        } else if let Some(bytes) = downloaded.get(&item.logical) {
            merged.insert(item.logical.clone(), bytes.clone());
        } else if let Some(bytes) = scan.objects.get(&item.logical) {
            merged.insert(item.logical.clone(), bytes.clone());
        }
    }
    merged
}

async fn apply_local(
    ports: &SyncPorts,
    plan: &Plan,
    downloaded: &BTreeMap<LogicalPath, Vec<u8>>,
    merged: &BTreeMap<LogicalPath, Vec<u8>>,
    scan: &Scan,
    report: &mut CycleReport,
) -> Result<(), CycleError> {
    let application = ports.portable.application().await.context(ApplySnafu)?;
    let clash = ports.portable.clash().await.context(ApplySnafu)?;
    let profiles = ports.portable.profiles().await.context(ApplySnafu)?;
    // A document item is applied from its merged projection; a file from the
    // downloaded bytes. A merge that changed nothing local is not applied.
    for item in &plan.items {
        let bytes = match item.action {
            ItemAction::Pull => downloaded.get(&item.logical),
            ItemAction::Merge => merged.get(&item.logical),
            ItemAction::Push | ItemAction::None => None,
        };
        let Some(bytes) = bytes else {
            continue;
        };
        let document = PortableDocument::from_bytes(bytes);
        let result = match (item.logical.as_str(), document) {
            (APPLICATION_ITEM, Ok(document)) => {
                ports
                    .portable
                    .apply_application(application.version, &document)
                    .await
            }
            (CLASH_ITEM, Ok(document)) => {
                ports.portable.apply_clash(clash.version, &document).await
            }
            (PROFILES_ITEM, Ok(document)) => {
                ports
                    .portable
                    .apply_profiles(profiles.version, &document)
                    .await
            }
            (APPLICATION_ITEM | CLASH_ITEM | PROFILES_ITEM, Err(_)) => {
                report.blocked.push(BlockedItem {
                    item: item.logical.to_string(),
                    reason: SyncBlockedReason::SchemaTooNew {
                        remote_revision: 0,
                        supported: nyanpasu_config::sync::document::PORTABLE_REVISION,
                    },
                });
                continue;
            }
            _ => {
                if item.logical.as_str().starts_with("profiles/") {
                    ports.files.write_text(&item.logical, bytes)
                } else {
                    ports.files.write_binary(&item.logical, bytes)
                }
            }
        };
        if let Err(ApplyError::VersionConflict { .. }) = result {
            // A local edit landed during the cycle; the item defers to the next one.
            continue;
        }
        result.context(ApplySnafu)?;
    }
    // A remote deletion applies only to a file, and only when the plan chose it.
    for item in &plan.items {
        if item.remote.is_present() || !item.local.is_present() {
            continue;
        }
        if matches!(item.action, ItemAction::Pull | ItemAction::Merge) {
            let _ = scan;
            if item.logical.as_str().starts_with("profiles/") {
                ports.files.remove_text(&item.logical).context(ApplySnafu)?;
            } else {
                ports
                    .files
                    .remove_binary(&item.logical)
                    .context(ApplySnafu)?;
            }
        }
    }
    Ok(())
}

async fn upload(
    store: &dyn RemoteStore,
    plan: &Plan,
    objects: &BTreeMap<LogicalPath, Vec<u8>>,
    merged: &BTreeMap<LogicalPath, Vec<u8>>,
) -> Result<(), CycleError> {
    for item in &plan.items {
        let bytes = match item.action {
            ItemAction::Push => objects.get(&item.logical),
            ItemAction::Merge => merged.get(&item.logical),
            ItemAction::Pull | ItemAction::None => None,
        };
        // A push uploads the local value; a merge uploads the merged value.
        let sha = match item.action {
            ItemAction::Push => item.local.sha256.as_ref(),
            ItemAction::Merge => item.remote.sha256.as_ref(),
            ItemAction::Pull | ItemAction::None => None,
        };
        let (Some(bytes), Some(sha)) = (bytes, sha) else {
            continue;
        };
        let key = object_key(sha);
        if store.get(&key).await.is_ok() {
            continue;
        }
        match store
            .put(&key, bytes.clone().into(), PutCondition::Any)
            .await
        {
            Ok(()) | Err(RemoteError::AlreadyExists { .. }) => {}
            Err(source) => return Err(CycleError::Remote { source }),
        }
    }
    Ok(())
}

/// Writes the complete portable state at the next generation.
///
/// The manifest is a full table, not a delta: a device that reads it needs no
/// history to know what the vault holds.
struct CommitInput<'a> {
    plan: &'a Plan,
    objects: &'a BTreeMap<LogicalPath, Vec<u8>>,
    merged: &'a BTreeMap<LogicalPath, Vec<u8>>,
    codecs: &'a BTreeMap<LogicalPath, ItemCodec>,
    head: Option<&'a CommitManifest>,
}

async fn commit(
    store: &dyn RemoteStore,
    meta: &SyncMeta,
    settings: &nyanpasu_contracts::sync::SyncSettings,
    input: CommitInput<'_>,
) -> Result<CommitManifest, CycleError> {
    let CommitInput {
        plan,
        objects,
        merged,
        codecs,
        head,
    } = input;
    let device = CommitDevice {
        id: meta
            .device_id
            .clone()
            .unwrap_or_else(|| DeviceId::new("local")),
        name: settings.device_name.clone(),
    };
    ensure_vault(store).await?;
    let generation = head.map_or(1, CommitManifest::next_generation);

    // Start from the head's table so an item this cycle did not touch keeps
    // its row, then overwrite the items the plan decided.
    let mut items: BTreeMap<LogicalPath, CommitItem> =
        head.map(|head| head.items.clone()).unwrap_or_default();
    for item in &plan.items {
        if item.action == ItemAction::None && item.baseline.sha256 == item.remote.sha256 {
            continue;
        }
        // A push commits the local value; every other action commits the
        // remote value the plan settled on.
        let next = match item.action {
            ItemAction::Push => &item.local,
            ItemAction::Pull | ItemAction::Merge | ItemAction::None => &item.remote,
        };
        match &next.sha256 {
            Some(sha) => {
                let size = merged
                    .get(&item.logical)
                    .or_else(|| objects.get(&item.logical))
                    .map_or(0, |bytes| bytes.len() as u64);
                let revision = next
                    .document
                    .as_ref()
                    .map(|document| document.portable_revision)
                    .or_else(|| {
                        item.local
                            .document
                            .as_ref()
                            .map(|document| document.portable_revision)
                    });
                items.insert(
                    item.logical.clone(),
                    CommitItem::Live(LiveItem {
                        codec: CommitCodec::from(
                            codecs
                                .get(&item.logical)
                                .copied()
                                .unwrap_or_else(|| scan_codec(&item.logical)),
                        ),
                        sha256: ObjectId::parse(sha).context(ObjectIdSnafu)?,
                        size,
                        portable_revision: revision,
                        changed: ChangedAt {
                            generation,
                            device: device.id.clone(),
                        },
                    }),
                );
            }
            None => {
                if let Some(previous) = &item.local.sha256 {
                    items.insert(
                        item.logical.clone(),
                        CommitItem::Deleted(super::commit::Tombstone {
                            deleted: super::commit::TombstoneBody {
                                generation,
                                device: device.id.clone(),
                                at: super::clock::now_iso8601(),
                                last_sha256: ObjectId::parse(previous).context(ObjectIdSnafu)?,
                            },
                        }),
                    );
                } else {
                    items.remove(&item.logical);
                }
            }
        }
    }
    let manifest = CommitManifest::new(
        generation,
        head.map(|head| head.id.clone()),
        head.map(CommitManifest::child_ancestors)
            .unwrap_or_default(),
        device,
        super::clock::now_iso8601(),
        None,
        items,
    )
    .context(CommitSnafu)?;
    let key = format!("{COMMITS_PREFIX}{:012}-{}.json", generation, manifest.id);
    let bytes = manifest.to_bytes().context(CommitSnafu)?;
    store
        .put(&key, bytes.into(), PutCondition::Any)
        .await
        .map_err(|source| CycleError::Remote { source })?;
    Ok(manifest)
}

async fn ensure_vault(store: &dyn RemoteStore) -> Result<VaultFormat, CycleError> {
    match store.get(FORMAT_KEY).await {
        Ok(bytes) => VaultFormat::parse(&bytes).context(CommitSnafu),
        Err(RemoteError::NotFound { .. }) => {
            let format = VaultFormat {
                format: super::commit::COMMIT_FORMAT,
                vault_id: VaultId::new("local"),
                created_at: super::clock::now_iso8601(),
                commit_naming: CommitNaming::Content,
                encryption: VaultEncryption::None,
            };
            store
                .put(
                    FORMAT_KEY,
                    format.to_bytes().context(CommitSnafu)?.into(),
                    PutCondition::IfAbsent,
                )
                .await
                .map_err(|source| CycleError::Remote { source })?;
            Ok(format)
        }
        Err(source) => Err(CycleError::Remote { source }),
    }
}

fn advance_baseline(
    ports: &SyncPorts,
    plan: &Plan,
    commit: &CommitManifest,
    loaded: &super::state::LoadedState,
    settled: bool,
    codecs: &BTreeMap<LogicalPath, ItemCodec>,
) -> Result<(), CycleError> {
    let mut baseline: BTreeMap<LogicalPath, BaselineItem> = loaded.baseline.clone();
    let mut documents = loaded.baseline_documents.clone();
    for item in &plan.items {
        match &item.baseline.sha256 {
            Some(sha) => {
                baseline.insert(
                    item.logical.clone(),
                    BaselineItem {
                        sha256: ObjectId::parse(sha).context(ObjectIdSnafu)?,
                        codec: codecs
                            .get(&item.logical)
                            .copied()
                            .unwrap_or_else(|| scan_codec(&item.logical)),
                    },
                );
                if let Some(document) = &item.baseline.document {
                    documents.insert(item.logical.clone(), document.clone());
                }
            }
            None => {
                baseline.remove(&item.logical);
                documents.remove(&item.logical);
            }
        }
    }
    let meta = SyncMeta {
        baseline_generation: commit.generation,
        baseline_commit: Some(commit.id.clone()),
        baseline_ancestors: commit.child_ancestors(),
        last_success_at: settled.then(super::clock::now_unix),
        ..loaded.meta.clone()
    };
    ports
        .state
        .save_baseline(&meta, &baseline, &documents)
        .context(StateSnafu)
}

fn fixed_key(key: &str) -> LogicalPath {
    LogicalPath::parse(key).expect("a fixed key is valid")
}

fn scan_codec(logical: &LogicalPath) -> ItemCodec {
    match logical.as_str() {
        APPLICATION_ITEM => ItemCodec::Document(nyanpasu_paths::SyncDocumentKind::Application),
        CLASH_ITEM => ItemCodec::Document(nyanpasu_paths::SyncDocumentKind::Clash),
        PROFILES_ITEM => ItemCodec::Document(nyanpasu_paths::SyncDocumentKind::Profiles),
        key if key.starts_with("icons/") => ItemCodec::Binary,
        _ => ItemCodec::Text,
    }
}

fn object_key(sha: &str) -> String {
    format!("{OBJECTS_PREFIX}{}/{}", &sha[..2], sha)
}

/// Recorded resolutions, keyed by conflict id, as the planner consumes them.
fn resolution_map(
    conflicts: &[SyncConflict],
) -> BTreeMap<String, nyanpasu_config::sync::ConflictSide> {
    conflicts
        .iter()
        .filter_map(|conflict| {
            conflict.resolution.map(|choice| {
                (
                    conflict.id.clone(),
                    match choice {
                        nyanpasu_runtime_values::sync::ConflictChoice::Local => {
                            nyanpasu_config::sync::ConflictSide::Local
                        }
                        nyanpasu_runtime_values::sync::ConflictChoice::Remote
                        | nyanpasu_runtime_values::sync::ConflictChoice::KeepBoth => {
                            nyanpasu_config::sync::ConflictSide::Remote
                        }
                    },
                )
            })
        })
        .collect()
}

fn pinned(
    documents: &BTreeMap<LogicalPath, (PortableDocument, PortableDocument)>,
) -> BTreeSet<LogicalPath> {
    documents.keys().cloned().collect()
}

fn blocked_from(enumeration: &SyncEnumeration) -> Vec<BlockedItem> {
    enumeration
        .skipped
        .iter()
        .filter_map(|skipped| match skipped {
            nyanpasu_paths::Skipped::UnportableName { physical } => Some(BlockedItem {
                item: physical.to_string(),
                reason: SyncBlockedReason::UnportableName,
            }),
            nyanpasu_paths::Skipped::CaseCollision { logical, .. } => Some(BlockedItem {
                item: logical.to_string(),
                reason: SyncBlockedReason::CaseCollision,
            }),
            nyanpasu_paths::Skipped::Link { .. } | nyanpasu_paths::Skipped::NotSelected { .. } => {
                None
            }
        })
        .collect()
}

fn conflicts_from(plan: &Plan) -> Vec<SyncConflict> {
    plan.conflicts
        .iter()
        .map(|conflict| {
            let (item, unit) = conflict
                .unit
                .split_once('#')
                .map_or((conflict.unit.as_str(), ""), |(item, unit)| (item, unit));
            SyncConflict {
                id: conflict.unit.clone(),
                item: item.to_owned(),
                unit: unit.to_owned(),
                kind: match conflict.kind {
                    ConflictKind::ModifyModify => SyncConflictKind::ModifyModify,
                    ConflictKind::ModifyDelete { .. } => SyncConflictKind::ModifyDelete,
                    ConflictKind::DeleteActive => SyncConflictKind::DeleteActive,
                },
                local: conflict.local.as_ref().map(serde_json::Value::to_string),
                remote: conflict.remote.as_ref().map(serde_json::Value::to_string),
                // A fresh plan carries no choice; the actor merges recorded ones.
                resolution: None,
            }
        })
        .collect()
}

/// Opens the configured provider's store.
///
/// # Errors
///
/// [`CycleError::Remote`] when no provider is configured or the factory refuses it.
pub fn open_store(
    factory: &Arc<dyn RemoteStoreFactory>,
    settings: &nyanpasu_contracts::sync::SyncSettings,
    secrets: &nyanpasu_contracts::sync::ProviderSecrets,
) -> Result<Arc<dyn RemoteStore>, CycleError> {
    let provider = settings
        .provider
        .as_ref()
        .ok_or_else(|| CycleError::Remote {
            source: RemoteError::MissingSetting { field: "provider" },
        })?;
    factory
        .open(provider, secrets)
        .map_err(|source| CycleError::Remote { source })
}

/// The remote directory name of the vault.
#[must_use]
pub fn vault_name() -> &'static str {
    VAULT_DIRECTORY
}

/// The three document logical paths.
#[must_use]
pub fn document_keys() -> [LogicalPath; super::ports::PORTABLE_ITEMS] {
    [
        fixed_key(APPLICATION_ITEM),
        fixed_key(CLASH_ITEM),
        fixed_key(PROFILES_ITEM),
    ]
}
