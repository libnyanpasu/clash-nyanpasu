//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use std::sync::Arc;

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
    #[allow(dead_code)]
    fetcher: Arc<dyn SubscriptionFetcher>,
    #[allow(dead_code)]
    notifier: Arc<dyn RebuildNotifier>,
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
    ReplaceDefinition {
        uid: ProfileId,
        definition: ProfileDefinition,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
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
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
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
        }
        Ok(())
    }
}
