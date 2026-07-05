//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use std::sync::Arc;

use nyanpasu_config::profile::{
    ProfileDefinition, ProfileDependencyIndex, ProfileId, ProfileMetadata, ProfileValidationError,
    Profiles,
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
    #[allow(dead_code)]
    fs: Arc<dyn ProfileFsPort>,
    #[allow(dead_code)]
    fetcher: Arc<dyn SubscriptionFetcher>,
    #[allow(dead_code)]
    notifier: Arc<dyn RebuildNotifier>,
}

#[derive(Debug)]
pub enum ProfilesActorMessage {
    Get(RpcReplyPort<Result<Arc<Profiles>, ProfilesError>>),
}

pub struct ProfilesActor;

impl ProfilesActor {
    fn current_state(state: &ProfilesActorState) -> Profiles {
        state.manager.snapshot_handle().load().state.clone()
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
        }
        Ok(())
    }
}
