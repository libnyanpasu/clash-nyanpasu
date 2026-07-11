//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use std::{collections::HashMap, sync::Arc};

use nyanpasu_config::profile::{
    ConfigDefinition, ExternalMode, ExternalProfilePath, LocalBinding, ManagedProfilePath,
    ProfileDefinition, ProfileDependencyIndex, ProfileId, ProfileItem, ProfileMetadata,
    ProfileMetadataPatch, ProfileSource, ProfileValidationError, Profiles,
    RemoteProfileOptionsPatch, ScriptRuntime, SubscriptionInfo, TransformDefinition,
};
use nyanpasu_core::state::PersistentStateManager;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use super::{
    ports::{ProfileFsPort, RebuildNotifier, SubscriptionFetcher},
    scheduler::{ExternalWatchers, RemoteUpdateScheduler},
};

#[derive(Debug, thiserror::Error)]
pub enum ProfilesError {
    #[error("profile not found: {0}")]
    ProfileNotFound(ProfileId),
    #[error(
        "profile is referenced and cannot be deleted (referrers: {referrers:?}, current: {current}, global_transforms: {global_transforms})"
    )]
    ProfileInUse {
        referrers: Vec<ProfileId>,
        /// Referenced by the document-level `current` selection.
        current: bool,
        /// Referenced by the document-level `global_transforms` list.
        global_transforms: bool,
    },
    #[error("profile has no materialized file")]
    ProfileHasNoFile,
    #[error("validation failed: {0:?}")]
    ValidationFailed(Vec<ProfileValidationError>),
    #[error("invalid reorder list: {reason}")]
    InvalidReorderList { reason: String },
    #[error("profile is not a remote profile")]
    NotARemoteProfile,
    #[error("file not writable: {reason}")]
    FileNotWritable { reason: String },
    #[error("refresh failed: {message}")]
    RefreshFailed { message: String },
    #[error("failed to persist profiles: {0}")]
    Persist(String),
    #[error("profiles actor rpc failed: {0}")]
    Rpc(String),
}

#[derive(Debug, Clone)]
pub struct CommitReport {
    pub snapshot: Arc<Profiles>,
    /// Dependency-closure judgement per the T04 affects_current rule table.
    pub affects_current: bool,
    /// Post-commit side-effect failures are degraded, not rolled back.
    pub warnings: Vec<String>,
    /// Server-generated uid (D13); set only by Add, consumed by import
    /// auto-activation (design §9).
    pub created: Option<ProfileId>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct NewProfileRequest {
    pub metadata: ProfileMetadata,
    /// Add rewrites the materialized path to `{uid}.{ext}`.
    pub definition: ProfileDefinition,
}

#[derive(Debug, Clone)]
pub enum ReorderOp {
    Move { active: ProfileId, over: ProfileId },
    ByList(Vec<ProfileId>),
}

pub struct ProfilesActorArgs {
    pub manager: PersistentStateManager<Profiles>,
    pub fs: Arc<dyn ProfileFsPort>,
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    pub notifier: Arc<dyn RebuildNotifier>,
}

pub struct ProfilesActorState {
    manager: PersistentStateManager<Profiles>,
    #[allow(dead_code)]
    index: ProfileDependencyIndex,
    fs: Arc<dyn ProfileFsPort>,
    fetcher: Arc<dyn SubscriptionFetcher>,
    notifier: Arc<dyn RebuildNotifier>,
    pending_refresh: HashMap<ProfileId, PendingRefresh>,
    scheduler: RemoteUpdateScheduler,
    external_watchers: ExternalWatchers,
}

struct PendingRefresh {
    reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    apply_suggested_interval: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RefreshOrigin {
    Import { update_interval_explicit: bool },
    Manual,
    Scheduled,
}

impl RefreshOrigin {
    fn apply_suggested_interval(self) -> bool {
        matches!(
            self,
            Self::Import {
                update_interval_explicit: false
            }
        )
    }
}

#[derive(Debug)]
pub enum RefreshOutcome {
    Succeeded {
        subscription: nyanpasu_config::profile::SubscriptionInfo,
        suggested_update_interval_minutes: Option<u64>,
        /// Validated payload; written to the materialized file inside the
        /// commit handler so a stale download can be fenced before any write.
        content: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug)]
pub enum ProfilesActorMessage {
    Get(RpcReplyPort<Result<Arc<Profiles>, ProfilesError>>),
    SetCurrent {
        current: Option<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    /// Activate `uid` only if nothing is currently selected. The reply is
    /// `Some(report)` when it activated, `None` when a current already existed.
    SetCurrentIfNone {
        uid: ProfileId,
        reply: RpcReplyPort<Result<Option<CommitReport>, ProfilesError>>,
    },
    SetGlobalTransforms {
        ids: Vec<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    SetValidFields {
        fields: Vec<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Replace {
        profiles: Profiles,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Add {
        request: NewProfileRequest,
        initial_file: Option<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Delete {
        uid: ProfileId,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Reorder {
        op: ReorderOp,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    PatchMetadata {
        uid: ProfileId,
        patch: ProfileMetadataPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    PatchRemoteOptions {
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    RefreshRemote {
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
        origin: RefreshOrigin,
        reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    },
    CommitRefreshed {
        uid: ProfileId,
        /// The URL the download was started for; the commit is discarded when
        /// the definition no longer points at it (replaced mid-download).
        url: url::Url,
        outcome: RefreshOutcome,
    },
    ExternalFileChanged {
        uid: ProfileId,
    },
    ReplaceDefinition {
        uid: ProfileId,
        definition: ProfileDefinition,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    #[cfg(test)]
    DebugInsertPendingRefresh {
        uid: ProfileId,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
        inserted: tokio::sync::oneshot::Sender<()>,
    },
}

pub struct ProfilesActor;

pub(super) enum PostCommitOp {
    WriteInitial {
        path: ManagedProfilePath,
        content: String,
    },
    Remove {
        path: ManagedProfilePath,
    },
    EnsureSymlink {
        path: ManagedProfilePath,
        target: ExternalProfilePath,
    },
}

pub(super) struct WriteOutcome {
    pub affects: AffectsRule,
    pub post_ops: Vec<PostCommitOp>,
}

pub(super) enum AffectsRule {
    Never,
    CurrentChanged,
    GlobalChanged,
    Touched(ProfileId),
    Always,
}

impl ProfilesActor {
    fn current_state(state: &ProfilesActorState) -> Profiles {
        state.manager.snapshot_handle().load().state.clone()
    }

    fn current_closure(profiles: &Profiles) -> indexmap::IndexSet<ProfileId> {
        let mut closure: indexmap::IndexSet<ProfileId> =
            profiles.global_transforms.iter().cloned().collect();
        let Some(current) = &profiles.current else {
            return closure;
        };

        closure.insert(current.clone());
        let mut configs = vec![current.clone()];
        if let Some(item) = profiles.items.get(current) {
            if let ProfileDefinition::Config {
                config: ConfigDefinition::Composition(composition),
            } = &item.definition
            {
                if let Some(base) = &composition.base {
                    closure.insert(base.clone());
                    configs.push(base.clone());
                }
                for member in &composition.extend_proxies_from {
                    closure.insert(member.clone());
                    configs.push(member.clone());
                }
            }
        }

        for config in configs {
            if let Some(item) = profiles.items.get(&config) {
                if let ProfileDefinition::Config { config } = &item.definition {
                    for transform in config.transforms() {
                        closure.insert(transform.clone());
                    }
                }
            }
        }

        closure
    }

    fn evaluate_affects(rule: &AffectsRule, before: &Profiles, after: &Profiles) -> bool {
        match rule {
            AffectsRule::Never => false,
            AffectsRule::Always => true,
            AffectsRule::CurrentChanged => before.current != after.current,
            AffectsRule::GlobalChanged => before.global_transforms != after.global_transforms,
            AffectsRule::Touched(uid) => {
                let closure_before = Self::current_closure(before);
                let closure_after = Self::current_closure(after);
                closure_before != closure_after
                    || closure_before.contains(uid)
                    || closure_after.contains(uid)
            }
        }
    }

    async fn run_write<F>(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        mutate: F,
    ) -> Result<CommitReport, ProfilesError>
    where
        F: FnOnce(&mut Profiles) -> Result<WriteOutcome, ProfilesError>,
    {
        let before = Self::current_state(state);
        let mut next = before.clone();
        let outcome = mutate(&mut next)?;
        next.validate().map_err(ProfilesError::ValidationFailed)?;
        state
            .manager
            .upsert(next.clone())
            .await
            .map_err(|e| ProfilesError::Persist(e.to_string()))?;
        state.index = ProfileDependencyIndex::build(&next);
        state.scheduler.reconcile(&next, myself, false);
        state.external_watchers.reconcile(&next, myself);

        let mut warnings = Vec::new();
        for op in outcome.post_ops {
            let result = match &op {
                PostCommitOp::WriteInitial { path, content } => {
                    state.fs.write_atomic(path, content)
                }
                PostCommitOp::Remove { path } => state.fs.remove(path),
                PostCommitOp::EnsureSymlink { path, target } => {
                    state.fs.ensure_symlink(path, target)
                }
            };
            if let Err(error) = result {
                warnings.push(format!("post-commit file operation failed: {error}"));
            }
        }

        let affects_current = Self::evaluate_affects(&outcome.affects, &before, &next);
        Ok(CommitReport {
            snapshot: Arc::new(next),
            affects_current,
            warnings,
            created: None,
        })
    }

    fn generate_uid(definition: &ProfileDefinition, existing: &Profiles) -> ProfileId {
        let prefix = match definition {
            ProfileDefinition::Config { .. } => 'c',
            ProfileDefinition::Transform { .. } => 't',
        };
        loop {
            let candidate = ProfileId(format!("{prefix}{}", nanoid::nanoid!(11)));
            if existing.items.get(&candidate).is_none() {
                return candidate;
            }
        }
    }

    fn canonical_extension(definition: &ProfileDefinition) -> &'static str {
        match definition {
            ProfileDefinition::Config { .. } => "yaml",
            ProfileDefinition::Transform { transform } => match transform {
                TransformDefinition::Overlay(_) => "yaml",
                TransformDefinition::Script(script) => match script.runtime {
                    ScriptRuntime::JavaScript => "js",
                    ScriptRuntime::Lua => "lua",
                },
            },
        }
    }

    fn validate_fetched_content(
        definition: &ProfileDefinition,
        content: &str,
    ) -> Result<(), String> {
        let needs_yaml = match definition {
            ProfileDefinition::Config { .. } => true,
            ProfileDefinition::Transform { transform } => {
                matches!(transform, TransformDefinition::Overlay(_))
            }
        };
        if needs_yaml {
            let mapping = serde_yaml::from_str::<serde_yaml::Mapping>(content)
                .map_err(|e| format!("downloaded content is not a YAML mapping: {e}"))?;
            // Legacy subscription semantics (remote.rs BC): a Config
            // subscription must actually carry proxies, otherwise arbitrary
            // mappings (e.g. `{}`) get persisted and can be auto-activated.
            if matches!(definition, ProfileDefinition::Config { .. })
                && !mapping.contains_key("proxies")
                && !mapping.contains_key("proxy-providers")
            {
                return Err("subscription does not contain `proxies` or `proxy-providers`".into());
            }
            Ok(())
        } else if content.trim().is_empty() {
            Err("downloaded script is empty".into())
        } else {
            Ok(())
        }
    }

    /// design §17 five reference categories. Item-level referrers plus the two
    /// document-level flags (current / global_transforms) so the IPC layer can
    /// render an unambiguous message even when the referrer list is empty.
    fn referrers_of(
        state: &ProfilesActorState,
        profiles: &Profiles,
        uid: &ProfileId,
    ) -> Option<(Vec<ProfileId>, bool, bool)> {
        let mut referrers: indexmap::IndexSet<ProfileId> = Default::default();
        if let Some(set) = state.index.composition_base_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.extend_proxies_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.transform_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }

        let current = profiles.current.as_ref() == Some(uid);
        let global_transforms = state.index.global_transform_ids.contains(uid);
        if referrers.is_empty() && !current && !global_transforms {
            None
        } else {
            Some((referrers.into_iter().collect(), current, global_transforms))
        }
    }

    /// Source slot discriminant used by ReplaceDefinition to decide whether the
    /// previously stored materialization metadata still describes the same file.
    fn source_kind(source: &ProfileSource) -> u8 {
        match source {
            ProfileSource::Local {
                binding: LocalBinding::Managed { .. },
            } => 0,
            ProfileSource::Local {
                binding: LocalBinding::External { .. },
            } => 1,
            ProfileSource::Remote { .. } => 2,
        }
    }
}

impl Actor for ProfilesActor {
    type Msg = ProfilesActorMessage;
    type State = ProfilesActorState;
    type Arguments = ProfilesActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let index = ProfileDependencyIndex::build(&args.manager.snapshot_handle().load().state);
        Ok(ProfilesActorState {
            manager: args.manager,
            index,
            fs: args.fs,
            fetcher: args.fetcher,
            notifier: args.notifier,
            pending_refresh: HashMap::new(),
            scheduler: RemoteUpdateScheduler::default(),
            external_watchers: ExternalWatchers::default(),
        })
    }

    async fn post_start(
        &self,
        myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let snapshot = Self::current_state(state);
        state.scheduler.reconcile(&snapshot, &myself, true);
        state.external_watchers.reconcile(&snapshot, &myself);
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ProfilesActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Arc::new(Self::current_state(state))));
            }
            ProfilesActorMessage::SetCurrent { current, reply } => {
                let result = Self::run_write(&myself, state, |profiles| {
                    profiles.set_current(current);
                    Ok(WriteOutcome {
                        affects: AffectsRule::CurrentChanged,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetCurrentIfNone { uid, reply } => {
                // Atomic conditional activation: select `uid` only when nothing
                // is currently selected. Serialized actor message handling makes
                // this read-then-write race-free without a second RPC, so a
                // concurrent SetCurrent cannot be silently overwritten.
                if Self::current_state(state).current.is_some() {
                    let _ = reply.send(Ok(None));
                } else {
                    let result = Self::run_write(&myself, state, |profiles| {
                        profiles.set_current(Some(uid));
                        Ok(WriteOutcome {
                            affects: AffectsRule::CurrentChanged,
                            post_ops: vec![],
                        })
                    })
                    .await;
                    let _ = reply.send(result.map(Some));
                }
            }
            ProfilesActorMessage::SetGlobalTransforms { ids, reply } => {
                let result = Self::run_write(&myself, state, |profiles| {
                    profiles.global_transforms = ids;
                    Ok(WriteOutcome {
                        affects: AffectsRule::GlobalChanged,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetValidFields { fields, reply } => {
                let result = Self::run_write(&myself, state, move |profiles| {
                    profiles.valid = fields;
                    // Whitelist changes reshape runtime extraction for the active
                    // config, so a rebuild is always required.
                    Ok(WriteOutcome {
                        affects: AffectsRule::Always,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Replace {
                profiles: next,
                reply,
            } => {
                let result = Self::run_write(&myself, state, |profiles| {
                    *profiles = next;
                    Ok(WriteOutcome {
                        affects: AffectsRule::Always,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Add {
                request,
                initial_file,
                reply,
            } => {
                let (uid, result) = {
                    let existing = Self::current_state(state);
                    let uid = Self::generate_uid(&request.definition, &existing);
                    let ext = Self::canonical_extension(&request.definition);
                    let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                        .expect("uid-derived path is always a valid managed path");
                    let mut definition = request.definition;
                    let mut post_ops = Vec::new();
                    if let Some(source) = definition.source_mut() {
                        {
                            // Server owns materialization metadata: new profiles
                            // start unmaterialized regardless of client input.
                            let materialized = source.materialized_mut();
                            materialized.file = canonical.clone();
                            materialized.updated_at = None;
                        }
                        if let ProfileSource::Remote { subscription, .. } = source {
                            *subscription = SubscriptionInfo::default();
                        }
                        match source {
                            ProfileSource::Local {
                                binding: LocalBinding::External { target, mode, .. },
                            } => {
                                if *mode == ExternalMode::Symlink {
                                    post_ops.push(PostCommitOp::EnsureSymlink {
                                        path: canonical.clone(),
                                        target: target.clone(),
                                    });
                                }
                                // T05 watcher reconcile owns the first mirror sync.
                            }
                            ProfileSource::Remote { .. } => {
                                // T05 RefreshRemote owns the first remote download.
                            }
                            _ => {
                                post_ops.push(PostCommitOp::WriteInitial {
                                    path: canonical.clone(),
                                    content: initial_file.clone().unwrap_or_default(),
                                });
                            }
                        }
                    }

                    let item = ProfileItem {
                        uid: uid.clone(),
                        metadata: request.metadata,
                        definition,
                    };
                    let result = Self::run_write(&myself, state, move |profiles| {
                        if !profiles.append_item(item) {
                            return Err(ProfilesError::Persist("uid collision".into()));
                        }
                        Ok(WriteOutcome {
                            affects: AffectsRule::Never,
                            post_ops,
                        })
                    })
                    .await;
                    (uid, result)
                };
                let result = result.map(|mut report| {
                    report.created = Some(uid.clone());
                    report
                });
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Delete { uid, reply } => {
                let result = {
                    let existing = Self::current_state(state);
                    if existing.items.get(&uid).is_none() {
                        Err(ProfilesError::ProfileNotFound(uid.clone()))
                    } else if let Some((referrers, current, global_transforms)) =
                        Self::referrers_of(state, &existing, &uid)
                    {
                        Err(ProfilesError::ProfileInUse {
                            referrers,
                            current,
                            global_transforms,
                        })
                    } else {
                        let removed = existing.items.get(&uid).cloned();
                        let post_ops = removed
                            .as_ref()
                            .and_then(|item| item.definition.source())
                            .map(|source| {
                                vec![PostCommitOp::Remove {
                                    path: source.materialized().file.clone(),
                                }]
                            })
                            .unwrap_or_default();
                        Self::run_write(&myself, state, move |profiles| {
                            profiles.remove_item_unchecked(&uid);
                            Ok(WriteOutcome {
                                affects: AffectsRule::Never,
                                post_ops,
                            })
                        })
                        .await
                    }
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Reorder { op, reply } => {
                let result = Self::run_write(&myself, state, move |profiles| {
                    match op {
                        ReorderOp::Move { active, over } => {
                            if profiles.items.get(&active).is_none() {
                                return Err(ProfilesError::ProfileNotFound(active));
                            }
                            if profiles.items.get(&over).is_none() {
                                return Err(ProfilesError::ProfileNotFound(over));
                            }
                            profiles.reorder(&active, &over);
                        }
                        ReorderOp::ByList(list) => {
                            if list.len() != profiles.items.len() {
                                return Err(ProfilesError::InvalidReorderList {
                                    reason: format!(
                                        "expected {} uids, got {}",
                                        profiles.items.len(),
                                        list.len()
                                    ),
                                });
                            }
                            let mut seen = indexmap::IndexSet::with_capacity(list.len());
                            for uid in &list {
                                if !seen.insert(uid.clone()) {
                                    return Err(ProfilesError::InvalidReorderList {
                                        reason: format!("duplicate uid {uid}"),
                                    });
                                }
                                if profiles.items.get(uid).is_none() {
                                    return Err(ProfilesError::ProfileNotFound(uid.clone()));
                                }
                            }
                            let mut reordered = indexmap::IndexMap::with_capacity(list.len());
                            for uid in list {
                                let item = profiles
                                    .items
                                    .shift_remove(&uid)
                                    .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
                                reordered.insert(uid, item);
                            }
                            profiles.items = reordered;
                        }
                    }
                    Ok(WriteOutcome {
                        affects: AffectsRule::Never,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchMetadata { uid, patch, reply } => {
                let result = Self::run_write(&myself, state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    item.apply_metadata_patch(patch);
                    Ok(WriteOutcome {
                        affects: AffectsRule::Never,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchRemoteOptions { uid, patch, reply } => {
                let result = Self::run_write(&myself, state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    match item.definition.source_mut() {
                        Some(ProfileSource::Remote { option, .. }) => {
                            use struct_patch::Patch as _;
                            option.apply(patch);
                            Ok(WriteOutcome {
                                affects: AffectsRule::Never,
                                post_ops: vec![],
                            })
                        }
                        _ => Err(ProfilesError::NotARemoteProfile),
                    }
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::RefreshRemote {
                uid,
                patch,
                origin,
                reply,
            } => {
                if state.pending_refresh.contains_key(&uid) {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "refresh already in progress".into(),
                        }));
                    }
                    return Ok(());
                }

                if let Some(patch) = patch {
                    let patched = Self::run_write(&myself, state, {
                        let uid = uid.clone();
                        move |profiles| {
                            let Some(item) = profiles.items.get_mut(&uid) else {
                                return Err(ProfilesError::ProfileNotFound(uid.clone()));
                            };
                            match item.definition.source_mut() {
                                Some(ProfileSource::Remote { option, .. }) => {
                                    use struct_patch::Patch as _;
                                    option.apply(patch);
                                    Ok(WriteOutcome {
                                        affects: AffectsRule::Never,
                                        post_ops: vec![],
                                    })
                                }
                                _ => Err(ProfilesError::NotARemoteProfile),
                            }
                        }
                    })
                    .await;
                    if let Err(err) = patched {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(err));
                        }
                        return Ok(());
                    }
                }

                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::ProfileNotFound(uid.clone())));
                    }
                    return Ok(());
                };
                let Some(ProfileSource::Remote {
                    url,
                    option,
                    materialized,
                    ..
                }) = item.definition.source()
                else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::NotARemoteProfile));
                    }
                    return Ok(());
                };

                let definition = item.definition.clone();
                let url = url.clone();
                let option = option.clone();
                state.pending_refresh.insert(
                    uid.clone(),
                    PendingRefresh {
                        reply,
                        apply_suggested_interval: origin.apply_suggested_interval(),
                    },
                );
                let fetcher = Arc::clone(&state.fetcher);
                let actor = myself.clone();
                tokio::spawn(async move {
                    // Download and validate only: the file write happens in the
                    // CommitRefreshed handler, after the stale-download fence,
                    // so an in-flight refresh can never clobber the file of a
                    // definition that was replaced meanwhile.
                    let outcome = async {
                        let fetched = fetcher
                            .fetch(&url, &option)
                            .await
                            .map_err(|e| format!("download failed: {e}"))?;
                        Self::validate_fetched_content(&definition, &fetched.content)?;
                        Ok::<_, String>((
                            fetched.subscription,
                            fetched.suggested_update_interval_minutes,
                            fetched.content,
                        ))
                    }
                    .await;
                    let outcome = match outcome {
                        Ok((subscription, suggested_update_interval_minutes, content)) => {
                            RefreshOutcome::Succeeded {
                                subscription,
                                suggested_update_interval_minutes,
                                content,
                            }
                        }
                        Err(message) => RefreshOutcome::Failed { message },
                    };
                    let _ = actor.cast(ProfilesActorMessage::CommitRefreshed { uid, url, outcome });
                });
            }
            ProfilesActorMessage::CommitRefreshed { uid, url, outcome } => {
                let pending = state
                    .pending_refresh
                    .remove(&uid)
                    .unwrap_or(PendingRefresh {
                        reply: None,
                        apply_suggested_interval: false,
                    });
                let reply = pending.reply;
                let snapshot = Self::current_state(state);
                // Nothing was written during the download phase, so a profile
                // deleted mid-download needs no file cleanup — just settle the
                // pending reply.
                let Some(item) = snapshot.items.get(&uid) else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "profile deleted during refresh".into(),
                        }));
                    }
                    return Ok(());
                };
                // Stale-download fence: the definition may have been replaced
                // while the download was in flight; committing would clobber
                // the new subscription's file and metadata with the old URL's
                // payload.
                let current_path = match item.definition.source() {
                    Some(ProfileSource::Remote {
                        url: current_url,
                        materialized,
                        ..
                    }) if *current_url == url => Some(materialized.file.clone()),
                    _ => None,
                };
                let Some(path) = current_path else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "subscription definition changed during refresh".into(),
                        }));
                    }
                    return Ok(());
                };

                let result = match outcome {
                    RefreshOutcome::Failed { message } => {
                        Err(ProfilesError::RefreshFailed { message })
                    }
                    RefreshOutcome::Succeeded {
                        subscription,
                        suggested_update_interval_minutes,
                        content,
                    } => {
                        // Re-validate against the CURRENT definition: the URL
                        // fence above cannot see a same-URL definition change
                        // (e.g. Overlay -> Config), whose content rules differ.
                        let written = match Self::validate_fetched_content(
                            &item.definition,
                            &content,
                        ) {
                            Err(message) => Err(anyhow::anyhow!(
                                "stale download no longer valid for the current definition: {message}"
                            )),
                            Ok(()) => {
                                // Keep the blocking write off the async actor
                                // thread (same pattern as the mirror sync); the
                                // handler awaits it, so message ordering is
                                // unchanged.
                                let fs = Arc::clone(&state.fs);
                                let write_path = path.clone();
                                tokio::task::spawn_blocking(move || {
                                    fs.ensure_not_symlink(&write_path)?;
                                    fs.write_atomic(&write_path, &content)
                                })
                                .await
                                .unwrap_or_else(|error| {
                                    Err(anyhow::anyhow!("refresh write task failed: {error}"))
                                })
                            }
                        };
                        match written {
                            Err(error) => Err(ProfilesError::RefreshFailed {
                                message: format!("failed to write refreshed content: {error}"),
                            }),
                            Ok(()) => {
                                Self::run_write(&myself, state, {
                                    let uid = uid.clone();
                                    move |profiles| {
                                        let Some(item) = profiles.items.get_mut(&uid) else {
                                            return Err(ProfilesError::ProfileNotFound(
                                                uid.clone(),
                                            ));
                                        };
                                        match item.definition.source_mut() {
                                            Some(ProfileSource::Remote {
                                                materialized,
                                                option,
                                                subscription: slot,
                                                ..
                                            }) => {
                                                materialized.updated_at =
                                                    Some(time::OffsetDateTime::now_utc());
                                                *slot = subscription;
                                                if pending.apply_suggested_interval {
                                                    if let Some(minutes) =
                                                        suggested_update_interval_minutes
                                                    {
                                                        option.update_interval_minutes = minutes;
                                                    }
                                                }
                                                Ok(WriteOutcome {
                                                    affects: AffectsRule::Touched(uid.clone()),
                                                    post_ops: vec![],
                                                })
                                            }
                                            _ => Err(ProfilesError::NotARemoteProfile),
                                        }
                                    }
                                })
                                .await
                            }
                        }
                    }
                };

                if reply.is_none() {
                    if let Ok(report) = &result {
                        if report.affects_current {
                            state.notifier.request_rebuild();
                        }
                    }
                }
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            ProfilesActorMessage::ExternalFileChanged { uid } => {
                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else {
                    return Ok(());
                };
                let Some(ProfileSource::Local {
                    binding:
                        LocalBinding::External {
                            materialized,
                            target,
                            mode,
                        },
                }) = item.definition.source()
                else {
                    return Ok(());
                };

                if *mode == ExternalMode::Mirror {
                    // T06A: keep the read→validate→write mirror sync off the
                    // async actor thread. The handler awaits the blocking task,
                    // so per-actor message ordering is unchanged.
                    let fs = state.fs.clone();
                    let target = target.clone();
                    let mirror_file = materialized.file.clone();
                    let definition = item.definition.clone();
                    let log_uid = uid.clone();
                    let synced = tokio::task::spawn_blocking(move || {
                        let content = match fs.read_external(&target) {
                            Ok(content) => content,
                            Err(error) => {
                                tracing::warn!(
                                    uid = %log_uid,
                                    target = %target,
                                    error = %error,
                                    "failed to read changed external profile"
                                );
                                return false;
                            }
                        };
                        if let Err(message) = Self::validate_fetched_content(&definition, &content)
                        {
                            tracing::warn!(
                                uid = %log_uid,
                                target = %target,
                                error = %message,
                                "changed external profile failed validation"
                            );
                            return false;
                        }
                        if let Err(error) = fs.write_atomic(&mirror_file, &content) {
                            tracing::warn!(
                                uid = %log_uid,
                                path = %mirror_file,
                                error = %error,
                                "failed to mirror changed external profile"
                            );
                            return false;
                        }
                        true
                    })
                    .await
                    .unwrap_or_else(|join_error| {
                        tracing::warn!(
                            uid = %uid,
                            error = %join_error,
                            "mirror sync task failed to run"
                        );
                        false
                    });
                    if !synced {
                        return Ok(());
                    }
                }

                let result = Self::run_write(&myself, state, {
                    let uid = uid.clone();
                    move |profiles| {
                        let Some(item) = profiles.items.get_mut(&uid) else {
                            return Err(ProfilesError::ProfileNotFound(uid.clone()));
                        };
                        match item.definition.source_mut() {
                            Some(ProfileSource::Local {
                                binding: LocalBinding::External { materialized, .. },
                            }) => {
                                materialized.updated_at = Some(time::OffsetDateTime::now_utc());
                                Ok(WriteOutcome {
                                    affects: AffectsRule::Touched(uid.clone()),
                                    post_ops: vec![],
                                })
                            }
                            _ => Err(ProfilesError::ProfileNotFound(uid.clone())),
                        }
                    }
                })
                .await;
                match result {
                    Ok(report) if report.affects_current => {
                        state.notifier.request_rebuild();
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(
                            uid = %uid,
                            error = %error,
                            "failed to commit external profile change"
                        );
                    }
                }
            }
            ProfilesActorMessage::ReplaceDefinition {
                uid,
                definition,
                reply,
            } => {
                let result = {
                    let existing = Self::current_state(state);
                    match existing.items.get(&uid) {
                        None => Err(ProfilesError::ProfileNotFound(uid.clone())),
                        Some(previous_item) => {
                            let mut definition = definition;
                            let ext = Self::canonical_extension(&definition);
                            let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                                .expect("uid-derived path is always a valid managed path");
                            let previous_source = previous_item.definition.source().cloned();
                            let mut post_ops = Vec::new();

                            // Server owns materialization metadata (same policy
                            // as Add): only an unchanged source slot keeps the
                            // previously stored updated_at/subscription. A
                            // Remote slot is only "unchanged" while it points at
                            // the same URL — a URL swap must reset updated_at
                            // and subscription so stale metadata is not carried
                            // over to a different subscription.
                            let same_slot = match (&previous_source, definition.source()) {
                                (Some(previous), Some(next)) => {
                                    Self::source_kind(previous) == Self::source_kind(next)
                                        && previous.materialized().file == canonical
                                        && match (previous, next) {
                                            (
                                                ProfileSource::Remote {
                                                    url: previous_url, ..
                                                },
                                                ProfileSource::Remote { url: next_url, .. },
                                            ) => previous_url == next_url,
                                            _ => true,
                                        }
                                }
                                _ => false,
                            };
                            if let Some(source) = definition.source_mut() {
                                {
                                    let materialized = source.materialized_mut();
                                    materialized.file = canonical.clone();
                                    materialized.updated_at = if same_slot {
                                        previous_source
                                            .as_ref()
                                            .and_then(|p| p.materialized().updated_at)
                                    } else {
                                        None
                                    };
                                }
                                if let ProfileSource::Remote { subscription, .. } = source {
                                    *subscription = match (same_slot, previous_source.as_ref()) {
                                        (
                                            true,
                                            Some(ProfileSource::Remote {
                                                subscription: previous,
                                                ..
                                            }),
                                        ) => previous.clone(),
                                        _ => SubscriptionInfo::default(),
                                    };
                                }
                            }
                            // Orphan cleanup: the old materialized file is
                            // unreachable once the path changes or the new
                            // definition has no source (Composition).
                            if let Some(previous) = previous_source.as_ref() {
                                let old_path = previous.materialized().file.clone();
                                if definition.source().is_none() || old_path != canonical {
                                    post_ops.push(PostCommitOp::Remove { path: old_path });
                                }
                            }
                            // Parity with Add: a newly introduced External
                            // Symlink binding must get its link created.
                            if !same_slot {
                                if let Some(ProfileSource::Local {
                                    binding: LocalBinding::External { target, mode, .. },
                                }) = definition.source()
                                {
                                    if *mode == ExternalMode::Symlink {
                                        post_ops.push(PostCommitOp::EnsureSymlink {
                                            path: canonical.clone(),
                                            target: target.clone(),
                                        });
                                    }
                                }
                            }

                            Self::run_write(&myself, state, move |profiles| {
                                let Some(item) = profiles.items.get_mut(&uid) else {
                                    return Err(ProfilesError::ProfileNotFound(uid.clone()));
                                };
                                item.set_definition(definition);
                                Ok(WriteOutcome {
                                    affects: AffectsRule::Touched(uid),
                                    post_ops,
                                })
                            })
                            .await
                        }
                    }
                };
                let _ = reply.send(result);
            }
            #[cfg(test)]
            ProfilesActorMessage::DebugInsertPendingRefresh {
                uid,
                reply,
                inserted,
            } => {
                state.pending_refresh.insert(
                    uid,
                    PendingRefresh {
                        reply: Some(reply),
                        apply_suggested_interval: false,
                    },
                );
                let _ = inserted.send(());
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.scheduler.shutdown();
        state.external_watchers.shutdown();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-2 review fix regression pins: Config subscriptions must carry
    /// proxies (legacy remote.rs semantics); overlays only need a mapping.
    #[test]
    fn config_content_requires_proxies_key() {
        let config = crate::enhance::golden_support::file_config("p1", "p1.yaml", &[]);
        assert!(ProfilesActor::validate_fetched_content(&config.definition, "{}\n").is_err());
        assert!(
            ProfilesActor::validate_fetched_content(&config.definition, "proxies: []\n").is_ok()
        );
        assert!(
            ProfilesActor::validate_fetched_content(&config.definition, "proxy-providers: {}\n")
                .is_ok()
        );
    }

    #[test]
    fn overlay_content_needs_only_a_mapping() {
        let overlay = crate::enhance::golden_support::overlay("t1", "t1.yaml");
        assert!(ProfilesActor::validate_fetched_content(&overlay.definition, "a: 1\n").is_ok());
    }
}
