//! The actor that owns network automation's session state.
//!
//! It is an actor rather than a pure service because it holds what no caller
//! can: the poll timer, the last settled network and what the policy last
//! applied. One mailbox also keeps a poll, a policy edit and an explicit
//! re-apply from interleaving while the switches are being written.

use std::{sync::Arc, time::Duration};

use nyanpasu_config::application::{NetworkAutomation, NetworkEvaluation, NetworkState};
use nyanpasu_contracts::network_environment::NetworkEnvironmentPort;
use nyanpasu_runtime_values::network_automation::{
    NetworkAutomationApplication, NetworkAutomationStatus,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, concurrency::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::ports::CaptureSwitches;

/// How often the network is read while the policy is enabled. A network
/// settles when two consecutive readings agree, so a change is acted on
/// within two periods and a burst of link events within one is ignored.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub(super) enum Message {
    /// The committed policy, sent by the application settings watcher.
    PolicyChanged(NetworkAutomation),
    /// Delivered by the poll timer, or by a test in place of one.
    Poll,
    Status(RpcReplyPort<NetworkAutomationStatus>),
    /// Reads the network now and applies its actions again, even when the
    /// match has not changed.
    ApplyNow(RpcReplyPort<NetworkAutomationStatus>),
}

pub struct Args {
    pub(crate) environment: Arc<dyn NetworkEnvironmentPort>,
    pub(crate) switches: Arc<dyn CaptureSwitches>,
    pub(crate) policy: NetworkAutomation,
    /// Production builds a real interval timer. Tests pass `false` and deliver
    /// `Poll` themselves, so an assertion never waits on a clock.
    pub(crate) schedule_polls: bool,
    /// Once cancelled, nothing reads the network or writes a switch.
    pub(crate) shutdown: CancellationToken,
}

pub(super) struct State {
    environment: Arc<dyn NetworkEnvironmentPort>,
    switches: Arc<dyn CaptureSwitches>,
    schedule_polls: bool,
    shutdown: CancellationToken,
    policy: NetworkAutomation,
    /// The latest reading, settled or not.
    latest: Option<NetworkState>,
    /// The last reading two consecutive polls agreed on.
    settled: Option<NetworkState>,
    /// What the policy last decided and what applying it did. A new
    /// evaluation equal to this one writes nothing, which is what lets a
    /// manual switch stand until the machine changes networks.
    applied: Option<(NetworkEvaluation, NetworkAutomationApplication)>,
    poll_job: Option<JoinHandle<()>>,
}

pub(super) struct NetworkAutomationActor;

impl Actor for NetworkAutomationActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let mut state = State {
            environment: args.environment,
            switches: args.switches,
            schedule_polls: args.schedule_polls,
            shutdown: args.shutdown,
            policy: args.policy,
            latest: None,
            settled: None,
            applied: None,
            poll_job: None,
        };
        state.refresh_polling(&myself);
        Ok(state)
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::PolicyChanged(policy) => state.set_policy(&myself, policy).await,
            Message::Poll => state.poll().await,
            Message::Status(reply) => {
                let _ = reply.send(state.status().await);
            }
            Message::ApplyNow(reply) => {
                state.apply_now().await;
                let _ = reply.send(state.status().await);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.stop_polling();
        Ok(())
    }
}

impl State {
    async fn set_policy(&mut self, myself: &ActorRef<Message>, policy: NetworkAutomation) {
        if policy == self.policy {
            return;
        }
        self.policy = policy;
        self.refresh_polling(myself);

        if self.policy.enabled {
            // An edit takes effect on the network the machine is already on.
            self.apply_settled(Trigger::Changed).await;
        } else {
            // A later enable starts from a fresh reading and applies again.
            self.latest = None;
            self.settled = None;
            self.applied = None;
        }
    }

    async fn poll(&mut self) {
        if !self.policy.enabled || self.shutdown.is_cancelled() {
            return;
        }
        let reading = Some(self.read().await);
        let agrees = reading == self.latest;
        self.latest = reading;
        if agrees && self.settled != self.latest {
            self.settled = self.latest.clone();
            self.apply_settled(Trigger::Changed).await;
        }
    }

    async fn apply_now(&mut self) {
        if !self.policy.enabled || self.shutdown.is_cancelled() {
            return;
        }
        self.latest = Some(self.read().await);
        self.settled = self.latest.clone();
        self.apply_settled(Trigger::Explicit).await;
    }

    /// Applies the policy's answer for the settled network. Offline has no
    /// answer: the switches stay as they are until a network is back.
    async fn apply_settled(&mut self, trigger: Trigger) {
        let Some(NetworkState::Online { network }) = &self.settled else {
            return;
        };
        let evaluation = self.policy.evaluate(network);
        let already_applied = self
            .applied
            .as_ref()
            .is_some_and(|(applied, _)| *applied == evaluation);
        if already_applied && trigger == Trigger::Changed {
            return;
        }

        tracing::info!(matched = ?evaluation.matched, "applying network automation");
        let outcomes = self.switches.apply(evaluation.actions).await;
        let application = NetworkAutomationApplication {
            matched: evaluation.matched.clone(),
            outcomes,
        };
        self.applied = Some((evaluation, application));
    }

    async fn status(&mut self) -> NetworkAutomationStatus {
        // A disabled policy does not poll, but the rule editor still shows
        // and pre-fills the current network.
        let network = if self.policy.enabled {
            self.latest.clone()
        } else {
            Some(self.read().await)
        };

        NetworkAutomationStatus {
            supported: true,
            network,
            applied: self
                .applied
                .as_ref()
                .map(|(_, application)| application.clone()),
        }
    }

    /// The observer blocks on the platform's tables, so it runs off the
    /// mailbox turn's worker.
    async fn read(&self) -> NetworkState {
        let environment = self.environment.clone();
        nyanpasu_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || environment.current()).await,
        )
    }

    fn refresh_polling(&mut self, myself: &ActorRef<Message>) {
        if !self.policy.enabled {
            self.stop_polling();
        } else if self.poll_job.is_none() && self.schedule_polls {
            // The first reading is taken at once, so the policy applies one
            // period after it is enabled or the application starts.
            let _ = myself.cast(Message::Poll);
            self.poll_job = Some(myself.send_interval(POLL_INTERVAL, || Message::Poll));
        }
    }

    fn stop_polling(&mut self) {
        if let Some(job) = self.poll_job.take() {
            job.abort();
        }
    }
}

/// Why the settled network is being evaluated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// The network or the policy changed: write only a new answer.
    Changed,
    /// The user asked for the rules to be applied again.
    Explicit,
}
