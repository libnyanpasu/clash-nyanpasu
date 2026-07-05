//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use std::{collections::HashMap, sync::Arc};

use nyanpasu_config::profile::{
    ConfigDefinition, ExternalMode, ExternalProfilePath, LocalBinding, ManagedProfilePath,
    ProfileDefinition, ProfileDependencyIndex, ProfileId, ProfileItem, ProfileMetadata,
    ProfileMetadataPatch, ProfileSource, ProfileValidationError, Profiles,
    RemoteProfileOptionsPatch, ScriptRuntime, TransformDefinition,
};
use nyanpasu_core::state::PersistentStateManager;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use super::ports::{ProfileFsPort, RebuildNotifier, SubscriptionFetcher};

#[derive(Debug, thiserror::Error)]
pub enum ProfilesError {
    #[error("profile not found: {0}")]
    ProfileNotFound(ProfileId),
    #[error("profile is referenced and cannot be deleted (referrers: {referrers:?})")]
    ProfileInUse { referrers: Vec<ProfileId> },
    #[error("profile has no materialized file")]
    ProfileHasNoFile,
    #[error("validation failed: {0:?}")]
    ValidationFailed(Vec<ProfileValidationError>),
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
}

#[derive(Debug, Clone)]
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
    pending_refresh: HashMap<ProfileId, Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>>,
}

#[derive(Debug)]
pub enum RefreshOutcome {
    Succeeded {
        subscription: nyanpasu_config::profile::SubscriptionInfo,
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
    SetGlobalTransforms {
        ids: Vec<ProfileId>,
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
        reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    },
    CommitRefreshed {
        uid: ProfileId,
        outcome: RefreshOutcome,
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
            serde_yaml::from_str::<serde_yaml::Mapping>(content)
                .map(|_| ())
                .map_err(|e| format!("downloaded content is not a YAML mapping: {e}"))
        } else if content.trim().is_empty() {
            Err("downloaded script is empty".into())
        } else {
            Ok(())
        }
    }

    fn referrers_of(
        state: &ProfilesActorState,
        profiles: &Profiles,
        uid: &ProfileId,
    ) -> Option<Vec<ProfileId>> {
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

        let document_level = profiles.current.as_ref() == Some(uid)
            || state.index.global_transform_ids.contains(uid);
        if referrers.is_empty() && !document_level {
            None
        } else {
            Some(referrers.into_iter().collect())
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
        })
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
                let result = Self::run_write(state, |profiles| {
                    profiles.set_current(current);
                    Ok(WriteOutcome {
                        affects: AffectsRule::CurrentChanged,
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetGlobalTransforms { ids, reply } => {
                let result = Self::run_write(state, |profiles| {
                    profiles.global_transforms = ids;
                    Ok(WriteOutcome {
                        affects: AffectsRule::GlobalChanged,
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
                let result = Self::run_write(state, |profiles| {
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
                let result = {
                    let existing = Self::current_state(state);
                    let uid = Self::generate_uid(&request.definition, &existing);
                    let ext = Self::canonical_extension(&request.definition);
                    let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                        .expect("uid-derived path is always a valid managed path");
                    let mut definition = request.definition;
                    let mut post_ops = Vec::new();
                    if let Some(source) = definition.source_mut() {
                        source.materialized_mut().file = canonical.clone();
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
                    Self::run_write(state, move |profiles| {
                        if !profiles.append_item(item) {
                            return Err(ProfilesError::Persist("uid collision".into()));
                        }
                        Ok(WriteOutcome {
                            affects: AffectsRule::Never,
                            post_ops,
                        })
                    })
                    .await
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Delete { uid, reply } => {
                let result = {
                    let existing = Self::current_state(state);
                    if existing.items.get(&uid).is_none() {
                        Err(ProfilesError::ProfileNotFound(uid.clone()))
                    } else if let Some(referrers) = Self::referrers_of(state, &existing, &uid) {
                        Err(ProfilesError::ProfileInUse { referrers })
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
                        Self::run_write(state, move |profiles| {
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
                let result = Self::run_write(state, move |profiles| {
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
                                return Err(ProfilesError::ValidationFailed(vec![]));
                            }
                            let mut seen = indexmap::IndexSet::with_capacity(list.len());
                            for uid in &list {
                                if !seen.insert(uid.clone()) {
                                    return Err(ProfilesError::ValidationFailed(vec![]));
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
                let result = Self::run_write(state, move |profiles| {
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
                let result = Self::run_write(state, move |profiles| {
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
            ProfilesActorMessage::RefreshRemote { uid, patch, reply } => {
                if state.pending_refresh.contains_key(&uid) {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "refresh already in progress".into(),
                        }));
                    }
                    return Ok(());
                }

                if let Some(patch) = patch {
                    let patched = Self::run_write(state, {
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
                let path = materialized.file.clone();
                state.pending_refresh.insert(uid.clone(), reply);
                let fetcher = Arc::clone(&state.fetcher);
                let fs = Arc::clone(&state.fs);
                let actor = myself.clone();
                tokio::spawn(async move {
                    let outcome = async {
                        let fetched = fetcher
                            .fetch(&url, &option)
                            .await
                            .map_err(|e| format!("download failed: {e}"))?;
                        Self::validate_fetched_content(&definition, &fetched.content)?;
                        fs.ensure_not_symlink(&path).map_err(|e| e.to_string())?;
                        fs.write_atomic(&path, &fetched.content)
                            .map_err(|e| e.to_string())?;
                        Ok::<_, String>(fetched.subscription)
                    }
                    .await;
                    let outcome = match outcome {
                        Ok(subscription) => RefreshOutcome::Succeeded { subscription },
                        Err(message) => RefreshOutcome::Failed { message },
                    };
                    let _ = actor.cast(ProfilesActorMessage::CommitRefreshed { uid, outcome });
                });
            }
            ProfilesActorMessage::CommitRefreshed { uid, outcome } => {
                let reply = state.pending_refresh.remove(&uid).flatten();
                let snapshot = Self::current_state(state);
                if snapshot.items.get(&uid).is_none() {
                    if let RefreshOutcome::Succeeded { .. } = outcome {
                        for ext in ["yaml", "js", "lua"] {
                            if let Ok(path) = ManagedProfilePath::new(format!("{uid}.{ext}")) {
                                let _ = state.fs.remove(&path);
                            }
                        }
                    }
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "profile deleted during refresh".into(),
                        }));
                    }
                    return Ok(());
                }

                let result = match outcome {
                    RefreshOutcome::Failed { message } => {
                        Err(ProfilesError::RefreshFailed { message })
                    }
                    RefreshOutcome::Succeeded { subscription } => {
                        Self::run_write(state, {
                            let uid = uid.clone();
                            move |profiles| {
                                let Some(item) = profiles.items.get_mut(&uid) else {
                                    return Err(ProfilesError::ProfileNotFound(uid.clone()));
                                };
                                match item.definition.source_mut() {
                                    Some(ProfileSource::Remote {
                                        materialized,
                                        subscription: slot,
                                        ..
                                    }) => {
                                        materialized.updated_at =
                                            Some(time::OffsetDateTime::now_utc());
                                        *slot = subscription;
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
            ProfilesActorMessage::ReplaceDefinition {
                uid,
                definition,
                reply,
            } => {
                let result = Self::run_write(state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid.clone()));
                    };
                    item.set_definition(definition);
                    Ok(WriteOutcome {
                        affects: AffectsRule::Touched(uid),
                        post_ops: vec![],
                    })
                })
                .await;
                let _ = reply.send(result);
            }
            #[cfg(test)]
            ProfilesActorMessage::DebugInsertPendingRefresh {
                uid,
                reply,
                inserted,
            } => {
                state.pending_refresh.insert(uid, Some(reply));
                let _ = inserted.send(());
            }
        }
        Ok(())
    }
}
