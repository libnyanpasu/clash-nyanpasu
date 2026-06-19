use std::sync::Arc;

use anyhow::Context as _;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use crate::config::{IVerge, nyanpasu::is_hex_color};
use nyanpasu_core::state::PersistentStateManager;

/// Injected legacy mirror hook: production updates the in-memory `Config::verge()`,
/// tests capture the committed state. Keeps the actor decoupled from the global config.
pub type VergeMirror = Arc<dyn Fn(IVerge) -> anyhow::Result<()> + Send + Sync + 'static>;

#[derive(Debug, Clone)]
pub struct VergeStateSnapshot {
    pub state: IVerge,
    /// Monotonic state version. Surfaced for callers/tests to observe commits; the
    /// event-system PRs will consume it, so production reads are not present yet.
    #[allow(dead_code)]
    pub version: u64,
}

pub struct StateActorArgs {
    pub manager: PersistentStateManager<IVerge>,
    pub mirror: VergeMirror,
}

pub struct StateActorState {
    manager: PersistentStateManager<IVerge>,
    mirror: VergeMirror,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum StateActorMessage {
    GetVerge(RpcReplyPort<anyhow::Result<VergeStateSnapshot>>),
    PatchVerge {
        patch: IVerge,
        reply: RpcReplyPort<anyhow::Result<VergeStateSnapshot>>,
    },
    ReplaceVerge {
        state: IVerge,
        reply: RpcReplyPort<anyhow::Result<VergeStateSnapshot>>,
    },
}

pub struct StateActor;

impl StateActor {
    fn snapshot(state: &StateActorState) -> VergeStateSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        VergeStateSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
        }
    }

    /// Atomically persist the next state and sync the legacy mirror. Validation is the
    /// caller's responsibility (`PatchVerge` validates, trusted paths do not).
    async fn commit(
        state: &mut StateActorState,
        next: IVerge,
    ) -> anyhow::Result<VergeStateSnapshot> {
        state
            .manager
            .upsert(next.clone())
            .await
            .context("failed to persist verge state")?;
        (state.mirror)(next).context("failed to sync legacy verge mirror")?;
        Ok(Self::snapshot(state))
    }
}

impl Actor for StateActor {
    type Msg = StateActorMessage;
    type State = StateActorState;
    type Arguments = StateActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(StateActorState {
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
            StateActorMessage::GetVerge(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            // PatchVerge originates from untrusted IPC: validate, merge, then persist + mirror.
            StateActorMessage::PatchVerge { patch, reply } => {
                let result = async {
                    validate_verge_patch(&patch)?;
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.patch_config(patch);
                    Self::commit(state, next).await
                }
                .await;
                let _ = reply.send(result);
            }
            // ReplaceVerge originates from trusted internal state (legacy/startup): no validation.
            StateActorMessage::ReplaceVerge { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
        }
        Ok(())
    }
}

/// Preserves the `theme_color` validation from `feat::patch_verge`.
pub fn validate_verge_patch(verge: &IVerge) -> anyhow::Result<()> {
    if let Some(theme_color) = &verge.theme_color
        && !theme_color.is_empty()
        && !is_hex_color(theme_color)
    {
        anyhow::bail!("Invalid theme color: {}", theme_color);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_verge_patch_accepts_valid_theme_colors() {
        assert!(
            validate_verge_patch(&IVerge {
                theme_color: Some(String::new()),
                ..IVerge::default()
            })
            .is_ok()
        );
        assert!(
            validate_verge_patch(&IVerge {
                theme_color: Some("#0a1B2c".into()),
                ..IVerge::default()
            })
            .is_ok()
        );
    }

    #[test]
    fn validate_verge_patch_rejects_invalid_theme_colors() {
        let short = validate_verge_patch(&IVerge {
            theme_color: Some("#abc".into()),
            ..IVerge::default()
        })
        .expect_err("short hex should fail");
        assert!(short.to_string().contains("Invalid theme color"));

        let non_hex = validate_verge_patch(&IVerge {
            theme_color: Some("#GGGGGG".into()),
            ..IVerge::default()
        })
        .expect_err("non-hex color should fail");
        assert!(non_hex.to_string().contains("Invalid theme color"));
    }
}
