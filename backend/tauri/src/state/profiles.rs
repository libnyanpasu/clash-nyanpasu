use std::sync::Arc;

use anyhow::Context as _;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use crate::config::Profiles;
use nyanpasu_core::state::PersistentStateManager;

/// Injected legacy mirror hook: production overwrites the in-memory `Config::profiles()`,
/// tests capture the committed state. Keeps the actor decoupled from the global config.
///
/// Unlike the verge mirror, this hook is infallible: the production mirror only swaps the
/// in-memory draft (`*draft() = state; apply()`), which cannot fail. Making it infallible
/// rules out the "upsert succeeded but mirror errored" window where the actor/disk would be
/// committed while `Config::profiles()` stayed stale.
pub type ProfilesMirror = Arc<dyn Fn(Profiles) + Send + Sync + 'static>;

#[derive(Debug, Clone)]
pub struct ProfilesStateSnapshot {
    pub state: Profiles,
    /// Monotonic state version. Surfaced for callers/tests to observe commits; the
    /// event-system PRs will consume it, so production reads are not present yet.
    #[allow(dead_code)]
    pub version: u64,
}

pub struct ProfilesStateActorArgs {
    pub manager: PersistentStateManager<Profiles>,
    pub mirror: ProfilesMirror,
}

pub struct ProfilesStateActorState {
    manager: PersistentStateManager<Profiles>,
    mirror: ProfilesMirror,
}

#[derive(Debug)]
pub enum ProfilesStateMessage {
    GetProfiles(RpcReplyPort<anyhow::Result<ProfilesStateSnapshot>>),
    /// Absolute, idempotent commit of the whole profiles index: `upsert` (disk) + mirror
    /// (sets `Config::profiles()`). The client is the sole persister of the index.
    CommitProfiles {
        state: Profiles,
        reply: RpcReplyPort<anyhow::Result<ProfilesStateSnapshot>>,
    },
}

pub struct ProfilesStateActor;

impl ProfilesStateActor {
    fn snapshot(state: &ProfilesStateActorState) -> ProfilesStateSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        ProfilesStateSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
        }
    }

    /// Atomically persist the absolute next index, then sync the legacy mirror.
    /// If `upsert` fails the mirror is never invoked, so `Config::profiles()` and disk
    /// both keep their previous value ("all or nothing").
    async fn commit(
        state: &mut ProfilesStateActorState,
        next: Profiles,
    ) -> anyhow::Result<ProfilesStateSnapshot> {
        state
            .manager
            .upsert(next.clone())
            .await
            .context("failed to persist profiles state")?;
        (state.mirror)(next);
        Ok(Self::snapshot(state))
    }
}

impl Actor for ProfilesStateActor {
    type Msg = ProfilesStateMessage;
    type State = ProfilesStateActorState;
    type Arguments = ProfilesStateActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ProfilesStateActorState {
            manager: args.manager,
            mirror: args.mirror,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ProfilesStateMessage::GetProfiles(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            ProfilesStateMessage::CommitProfiles { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
        }
        Ok(())
    }
}
