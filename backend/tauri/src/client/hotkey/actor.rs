//! The actor that owns the OS shortcut registrations.
//!
//! It is an actor rather than a pure service because the set of accelerators
//! the OS currently holds for this process is state no caller can keep: it only
//! advances when a grab is actually accepted. Serialising reconciles through
//! one mailbox is also what keeps two overlapping config changes from
//! registering and unregistering the same accelerator at the same time.

use std::{collections::BTreeMap, sync::Arc};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use super::{
    HotkeyStatus,
    ports::{HotkeyAction, HotkeyActionSink, HotkeyBindings, HotkeyOp, ShortcutRegistrar},
};
use crate::client::effects::{
    plan::EffectKind,
    status::{EffectHealth, EffectRevision, EffectStatus},
};

pub(super) enum Message {
    Reconcile {
        revision: EffectRevision,
        desired: HotkeyBindings,
        reply: RpcReplyPort<EffectStatus>,
    },
    #[cfg_attr(not(test), allow(dead_code))]
    Status(RpcReplyPort<HotkeyStatus>),
    /// Exit path: hand every accelerator back to the OS.
    UnregisterAll(RpcReplyPort<EffectStatus>),
}

pub struct Args {
    pub registrar: Arc<dyn ShortcutRegistrar>,
    pub sink: Arc<dyn HotkeyActionSink>,
}

pub(super) struct State {
    registrar: Arc<dyn ShortcutRegistrar>,
    sink: Arc<dyn HotkeyActionSink>,
    /// Highest revision acted on. Protects the reconcile entry points that do
    /// not pass through the facade's gate, such as the startup full plan.
    applied_revision: EffectRevision,
    health: EffectHealth,
    /// Only accelerators the OS confirmed. A failed grab stays out, so the next
    /// reconcile retries it instead of believing it is in place.
    registered: BTreeMap<String, HotkeyAction>,
}

pub(super) struct HotkeyActor;

impl Actor for HotkeyActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(State {
            registrar: args.registrar,
            sink: args.sink,
            applied_revision: EffectRevision::default(),
            health: EffectHealth::Healthy,
            registered: BTreeMap::new(),
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Reconcile {
                revision,
                desired,
                reply,
            } => {
                let status = state.reconcile(revision, desired).await;
                let _ = reply.send(status);
            }
            Message::Status(reply) => {
                let _ = reply.send(state.status());
            }
            Message::UnregisterAll(reply) => {
                let status = state.unregister_all().await;
                let _ = reply.send(status);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // A process that exits without giving the grabs back leaves the
        // accelerators dead for every other application.
        state.unregister_all().await;
        Ok(())
    }
}

impl State {
    async fn reconcile(
        &mut self,
        revision: EffectRevision,
        desired: HotkeyBindings,
    ) -> EffectStatus {
        // A reconcile no newer than what is in place carries an older desired
        // state by definition, so applying it would undo the newer one.
        if revision <= self.applied_revision {
            tracing::debug!(
                requested = revision.get(),
                applied = self.applied_revision.get(),
                "dropping a superseded hotkey reconcile"
            );
            return EffectStatus {
                kind: EffectKind::Hotkeys,
                desired_revision: revision,
                applied_revision: self.applied_revision,
                health: EffectHealth::Superseded,
            };
        }
        self.applied_revision = revision;

        // Every accelerator is checked before a single grab is released. The
        // old check sat inside `register`, which runs after the releases, so
        // one unparsable binding tore down the shortcuts that did work and then
        // failed. Nothing here is retryable: the list has to change first.
        let rejected: Vec<String> = desired
            .accelerators()
            .filter(|accelerator| self.registrar.validate(accelerator).is_err())
            .map(ToOwned::to_owned)
            .collect();
        if !rejected.is_empty() {
            let status = self.degraded(
                revision,
                "hotkey_invalid_bindings",
                format!(
                    "the platform refused {}: {}",
                    rejected.len(),
                    rejected.join("; ")
                ),
                false,
            );
            self.health = status.health.clone();
            return status;
        }

        let current = HotkeyBindings::from(self.registered.clone());
        let ops = current.diff(&desired);
        if ops.is_empty() {
            self.health = EffectHealth::Healthy;
            return self.healthy(revision);
        }

        let total = ops.len();
        let mut failures = Vec::new();

        // Every release before any grab: swapping two accelerators, or moving
        // one function onto another's key, only works if the OS is holding
        // neither of them when the new grabs are requested.
        for op in &ops {
            match op {
                HotkeyOp::Unbind { accelerator } => {
                    match self.unregister(accelerator).await {
                        Ok(()) => {
                            self.registered.remove(accelerator);
                        }
                        // Left in place so the next reconcile tries again.
                        Err(error) => failures.push(format!("{accelerator}: {error}")),
                    }
                }
                HotkeyOp::Rebind { accelerator, .. } => {
                    if let Err(error) = self.unregister(accelerator).await {
                        tracing::debug!(%error, accelerator, "rebind could not release the old grab first");
                    }
                    self.registered.remove(accelerator);
                }
                HotkeyOp::Bind { .. } => {}
            }
        }

        for op in &ops {
            let (accelerator, action) = match op {
                HotkeyOp::Bind {
                    accelerator,
                    action,
                }
                | HotkeyOp::Rebind {
                    accelerator,
                    action,
                } => (accelerator, *action),
                HotkeyOp::Unbind { .. } => continue,
            };
            match self.register(accelerator, action).await {
                Ok(()) => {
                    self.registered.insert(accelerator.clone(), action);
                }
                Err(error) => failures.push(format!("{accelerator}: {error}")),
            }
        }

        let status = if failures.is_empty() {
            self.healthy(revision)
        } else {
            // The successful grabs stay: partial hotkeys beat none, and the
            // caller is told exactly which ones are missing.
            self.degraded(
                revision,
                "hotkey_partial_registration",
                format!(
                    "{} of {} shortcuts failed: {}",
                    failures.len(),
                    total,
                    failures.join("; ")
                ),
                true,
            )
        };
        self.health = status.health.clone();
        status
    }

    async fn unregister_all(&mut self) -> EffectStatus {
        self.registered.clear();
        let registrar = self.registrar.clone();
        let revision = self.applied_revision;
        match blocking(move || registrar.unregister_all()).await {
            Ok(()) => self.healthy(revision),
            Err(error) => self.degraded(
                revision,
                "hotkey_unregister_all_failed",
                error.to_string(),
                true,
            ),
        }
    }

    async fn register(&self, accelerator: &str, action: HotkeyAction) -> anyhow::Result<()> {
        // No validation here: `reconcile` cleared the whole desired set before
        // it released anything.
        let registrar = self.registrar.clone();
        let sink = self.sink.clone();
        let accelerator = accelerator.to_owned();
        blocking(move || registrar.register(&accelerator, action, sink)).await
    }

    async fn unregister(&self, accelerator: &str) -> anyhow::Result<()> {
        let registrar = self.registrar.clone();
        let accelerator = accelerator.to_owned();
        blocking(move || registrar.unregister(&accelerator)).await
    }

    fn status(&self) -> HotkeyStatus {
        HotkeyStatus {
            applied_revision: self.applied_revision,
            health: self.health.clone(),
            registered: self.registered.clone(),
        }
    }

    fn healthy(&self, revision: EffectRevision) -> EffectStatus {
        EffectStatus {
            kind: EffectKind::Hotkeys,
            desired_revision: revision,
            applied_revision: self.applied_revision,
            health: EffectHealth::Healthy,
        }
    }

    fn degraded(
        &self,
        revision: EffectRevision,
        code: &'static str,
        message: String,
        retryable: bool,
    ) -> EffectStatus {
        tracing::warn!(code, %message, "a hotkey effect failed after the config was committed");
        EffectStatus {
            kind: EffectKind::Hotkeys,
            desired_revision: revision,
            applied_revision: self.applied_revision,
            health: EffectHealth::Degraded {
                code,
                message,
                retryable,
            },
        }
    }
}

/// The platform shortcut API blocks. Running it on the mailbox turn would stall
/// every other message behind a window-server round trip.
async fn blocking<F>(work: F) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => Err(anyhow::anyhow!("the hotkey worker panicked: {error}")),
    }
}
