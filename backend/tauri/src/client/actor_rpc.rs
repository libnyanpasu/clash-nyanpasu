use ractor::{ActorRef, Message, RpcReplyPort, rpc::CallResult};
use std::time::Duration;

const STATE_WAIT: Duration = Duration::from_secs(180);

#[derive(Debug, thiserror::Error)]
pub enum ActorRpcError {
    #[error(
        "actor response timed out; operation outcome is unknown; inspect committed state and configuration status before retrying"
    )]
    OutcomeUnknown,
    #[error(
        "actor response unavailable; operation outcome is unknown; inspect committed state before retrying"
    )]
    Unavailable,
}

pub(crate) async fn call<T: Send + 'static, M: Message>(
    actor: &ActorRef<M>,
    make: impl FnOnce(RpcReplyPort<T>) -> M,
    timeout: Option<Duration>,
) -> Result<T, ActorRpcError> {
    match actor.call(make, Some(timeout.unwrap_or(STATE_WAIT))).await {
        Ok(CallResult::Success(result)) => Ok(result),
        Ok(CallResult::Timeout) => Err(ActorRpcError::OutcomeUnknown),
        _ => Err(ActorRpcError::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ractor::{Actor, ActorProcessingErr};

    struct Waiting;
    impl Actor for Waiting {
        type Msg = RpcReplyPort<()>;
        type State = Option<RpcReplyPort<()>>;
        type Arguments = ();
        async fn pre_start(
            &self,
            _: ActorRef<Self::Msg>,
            _: (),
        ) -> Result<Self::State, ActorProcessingErr> {
            Ok(None)
        }
        async fn handle(
            &self,
            _: ActorRef<Self::Msg>,
            reply: Self::Msg,
            state: &mut Self::State,
        ) -> Result<(), ActorProcessingErr> {
            *state = Some(reply);
            Ok(())
        }
    }
    #[tokio::test(start_paused = true)]
    async fn timeout_does_not_claim_the_operation_was_cancelled() {
        let (actor, task) = Actor::spawn(None, Waiting, ()).await.unwrap();
        let error = call(&actor, |reply| reply, Some(Duration::from_secs(1)))
            .await
            .unwrap_err();
        assert!(matches!(error, ActorRpcError::OutcomeUnknown));
        actor.stop(None);
        task.await.unwrap();
    }
}
