//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use std::sync::Arc;

use nyanpasu_config::profile::{
    ConfigDefinition, ExternalProfilePath, ManagedProfilePath, ProfileDefinition,
    ProfileDependencyIndex, ProfileId, ProfileMetadata, ProfileValidationError, Profiles,
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
        }
        Ok(())
    }
}
