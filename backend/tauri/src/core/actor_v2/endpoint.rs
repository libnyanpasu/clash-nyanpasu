//! The endpoint port of CoreActor v2: one narrow trait over "a host's
//! `CoreControl`", implemented by the in-process Local adapter and the IPC v2
//! Service adapter.
//!
//! Design note (deviation from the integration design's concrete
//! `EndpointHandle` enum, recorded): the router consumes a trait object plus
//! an [`ExecutionHost`] tag instead of a two-variant enum. Same shape, one
//! test seam — the fake endpoint in the actor tests is the third
//! implementation the enum could not have carried.
//!
//! Everything here is *reading or forwarding*; the router never synthesizes
//! lifecycle state (invariant I-R3). The normalized snapshot below is a
//! field-by-field projection of what the host published, nothing more.

use std::{borrow::Cow, future::Future, pin::Pin, sync::Arc, time::Duration};

use nyanpasu_core_manager::{
    ApplyOutcome, CoreCommandEnvelope, CoreControl, CoreError, CoreErrorKind, OperationId,
    OperationOutput, OperationState,
};
use nyanpasu_ipc::api::{
    core::v2::{
        CoreCommandInfo, CoreOperationReq, CoreSubmitReq, OperationInfo, OperationOutputInfo,
        OperationPhase, ReconcileOutcomeInfo, ReconcileOutcomeKind,
    },
    status::{CoreInfos, CoreStateDetail},
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Which controller owns the runtime. The app perceives the difference in
/// exactly two places: this tag on the endpoint slot, and the handoff
/// protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionHost {
    Local,
    Service,
}

/// The host's canonical core status, projected field by field for the app.
/// No field here is ever derived by the router itself.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreStatusSnapshot {
    /// `None` means the host published no state this router can trust — an
    /// unmapped future variant, or a daemon that answered without a detail.
    /// No consumer may read it as `Stopped`: "we do not know" is the one
    /// answer a stop proof must never accept.
    pub state: Option<CoreStateDetail>,
    pub state_changed_at: i64,
    /// The applied revision's CAS identity, when one is running.
    pub revision: Option<nyanpasu_ipc::api::status::RevisionIdInfo>,
    /// Healthy / unhealthy, when the host reports it.
    pub healthy: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct CoreSubmission {
    pub envelope: CoreCommandEnvelope,
    pub core_type: Option<nyanpasu_utils::core::CoreType>,
}

/// One host's control plane, as the router consumes it. Submit is the only
/// mutating call and is always envelope-shaped; waiting on an operation is a
/// read and deliberately not routed through the actor mailbox.
pub trait ControlEndpoint: Send + Sync {
    fn host(&self) -> ExecutionHost;

    /// Admission into the host's executor. Returns the operation's
    /// admission-time snapshot; the transaction survives this future's drop.
    fn submit<'a>(
        &'a self,
        submission: CoreSubmission,
    ) -> BoxFuture<'a, Result<OperationInfo, CoreError>>;

    /// Long-poll the host's registry. `None` = unknown/evicted id (recover by
    /// re-reading status; the revision CAS blocks double application).
    fn wait_operation<'a>(
        &'a self,
        id: OperationId,
        timeout: Duration,
    ) -> BoxFuture<'a, Option<OperationInfo>>;

    fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>>;
}

// ---------------------------------------------------------------------------
// Local host: the in-process CoreControl.
// ---------------------------------------------------------------------------

pub struct LocalEndpoint {
    control: CoreControl,
}

impl LocalEndpoint {
    pub fn new(control: CoreControl) -> Self {
        Self { control }
    }
}

impl ControlEndpoint for LocalEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    fn submit<'a>(
        &'a self,
        submission: CoreSubmission,
    ) -> BoxFuture<'a, Result<OperationInfo, CoreError>> {
        Box::pin(async move {
            let handle = self.control.submit(submission.envelope)?;
            Ok(map_local_operation(handle.id(), handle.state()))
        })
    }

    fn wait_operation<'a>(
        &'a self,
        id: OperationId,
        timeout: Duration,
    ) -> BoxFuture<'a, Option<OperationInfo>> {
        Box::pin(async move {
            self.control
                .wait_operation(id, timeout)
                .await
                .map(|state| map_local_operation(id, state))
        })
    }

    fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>> {
        Box::pin(async move { Ok(map_local_status(&self.control.status())) })
    }
}

/// The local sibling of the daemon bridge's wire mapping. Duplicated on
/// purpose: the app must not depend on the daemon host crate, and the wire
/// DTO is the shared vocabulary both sides project into.
fn map_local_operation(id: OperationId, state: OperationState) -> OperationInfo {
    let (phase, output, error) = match state {
        OperationState::Queued => (OperationPhase::Queued, None, None),
        OperationState::Running => (OperationPhase::Running, None, None),
        OperationState::Succeeded(output) => (
            OperationPhase::Succeeded,
            Some(match output {
                OperationOutput::Reconciled(outcome) => {
                    OperationOutputInfo::Reconciled(map_local_outcome(&outcome))
                }
                OperationOutput::Stopped => OperationOutputInfo::Stopped,
                OperationOutput::Recovered => OperationOutputInfo::Recovered,
                OperationOutput::ShutDown => OperationOutputInfo::ShutDown,
            }),
            None,
        ),
        OperationState::Failed(failure) => (
            OperationPhase::Failed,
            None,
            Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                kind: failure.kind.map(|kind| Cow::Borrowed(kind.as_str())),
                message: failure.message,
                retryable: failure.retryable,
            }),
        ),
    };
    OperationInfo {
        id: id.to_string(),
        phase,
        output,
        error,
    }
}

fn map_local_outcome(outcome: &ApplyOutcome) -> ReconcileOutcomeInfo {
    let mut warnings = Vec::new();
    let mut current = outcome;
    while let ApplyOutcome::DurabilityUncertain { outcome, warning } = current {
        warnings.push(warning.clone());
        current = &**outcome;
    }
    let (kind, revision, failed_apply) = match current {
        ApplyOutcome::Started { revision } => (ReconcileOutcomeKind::Started, revision, None),
        ApplyOutcome::Noop { revision } => (ReconcileOutcomeKind::Noop, revision, None),
        ApplyOutcome::Patched { revision } => (ReconcileOutcomeKind::Patched, revision, None),
        ApplyOutcome::Reloaded { revision } => (ReconcileOutcomeKind::Reloaded, revision, None),
        ApplyOutcome::Restarted { revision } => (ReconcileOutcomeKind::Restarted, revision, None),
        ApplyOutcome::Switched { revision } => (ReconcileOutcomeKind::Switched, revision, None),
        ApplyOutcome::RolledBack {
            revision,
            failed_apply,
        } => (
            ReconcileOutcomeKind::RolledBack,
            revision,
            Some(failed_apply.clone()),
        ),
        ApplyOutcome::DurabilityUncertain { .. } => unreachable!("unwrapped by the loop above"),
    };
    ReconcileOutcomeInfo {
        outcome: kind,
        revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
            epoch: revision.epoch.get(),
            generation: revision.generation,
            source_hash: revision.source_hash.clone(),
            effective_hash: revision.effective_hash.clone(),
        },
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
        failed_apply,
    }
}

fn map_local_status(status: &nyanpasu_core_manager::CoreStatus) -> CoreStatusSnapshot {
    use nyanpasu_core_manager::CoreState as ManagerCoreState;
    let state = match &status.state {
        ManagerCoreState::Stopped { reason } => Some(CoreStateDetail::Stopped {
            reason: reason.as_ref().map(|reason| reason.to_string()),
        }),
        ManagerCoreState::Starting { epoch } => {
            Some(CoreStateDetail::Starting { epoch: epoch.get() })
        }
        ManagerCoreState::Running { epoch, pid } => Some(CoreStateDetail::Running {
            epoch: epoch.get(),
            pid: *pid,
        }),
        ManagerCoreState::Restarting { epoch, attempt } => Some(CoreStateDetail::Restarting {
            epoch: epoch.get(),
            attempt: *attempt,
        }),
        ManagerCoreState::Switching { from, to } => Some(CoreStateDetail::Switching {
            from: from.map(|epoch| epoch.get()),
            to: to.get(),
        }),
        ManagerCoreState::Stopping { epoch } => {
            Some(CoreStateDetail::Stopping { epoch: epoch.get() })
        }
        // `CoreState` is `#[non_exhaustive]`; an unknown future state stays
        // unknown. Folding it into `Stopped` is how a router invents a stop
        // proof it never received.
        other => {
            tracing::warn!("unmapped manager core state: {other:?}");
            None
        }
    };
    CoreStatusSnapshot {
        state,
        state_changed_at: status.changed_at,
        revision: status.revision.as_ref().map(|revision| {
            nyanpasu_ipc::api::status::RevisionIdInfo {
                epoch: revision.epoch.get(),
                generation: revision.generation,
                effective_hash: revision.effective_hash.clone(),
            }
        }),
        healthy: status
            .health
            .as_ref()
            .map(|health| matches!(health.state, nyanpasu_core_manager::HealthState::Healthy)),
    }
}

// ---------------------------------------------------------------------------
// Service host: the daemon's CoreControl behind the IPC v2 wire.
// ---------------------------------------------------------------------------

pub struct ServiceEndpoint {
    client: nyanpasu_ipc::client::Client,
}

impl ServiceEndpoint {
    pub fn new(client: nyanpasu_ipc::client::Client) -> Self {
        Self { client }
    }
}

/// The daemon's own classification, kept whole.
///
/// A `Server` failure is the daemon answering: its `error_kind` and
/// `retryable` are facts about that failure and are carried through unchanged,
/// including a kind this build has no variant for — which stays `None` rather
/// than becoming a guess. Everything else is the transport itself failing,
/// which no daemon classified and which is retryable by definition.
fn map_client_error(error: nyanpasu_ipc::client::ClientError) -> CoreError {
    use nyanpasu_ipc::client::ClientError;
    if !matches!(error, ClientError::Server { .. }) {
        return CoreError::new(
            CoreErrorKind::BackendUnavailable,
            format!("service endpoint unreachable: {error}"),
            true,
        );
    }
    CoreError {
        kind: error.core_error_kind(),
        retryable: error.retryable(),
        message: error.to_string(),
        operation_id: None,
    }
}

impl ControlEndpoint for ServiceEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }

    fn submit<'a>(
        &'a self,
        submission: CoreSubmission,
    ) -> BoxFuture<'a, Result<OperationInfo, CoreError>> {
        Box::pin(async move {
            let request = wire_submit_request(&submission)?;
            self.client
                .submit_core(&request)
                .await
                .map_err(map_client_error)
        })
    }

    fn wait_operation<'a>(
        &'a self,
        id: OperationId,
        timeout: Duration,
    ) -> BoxFuture<'a, Option<OperationInfo>> {
        Box::pin(async move {
            let id = id.to_string();
            let request = CoreOperationReq {
                operation_id: Cow::Borrowed(id.as_str()),
                wait_ms: Some(timeout.as_millis().min(u128::from(u64::MAX)) as u64),
            };
            // Transport failure and "unknown id" both surface as None here;
            // the caller's recovery path (re-read status + CAS) covers both.
            self.client
                .core_operation(&request)
                .await
                .inspect_err(|error| {
                    tracing::debug!("operation {id} could not be waited on: {error}");
                })
                .ok()
        })
    }

    fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>> {
        Box::pin(async move {
            let infos = self
                .client
                .core_status_v2()
                .await
                .map_err(map_client_error)?;
            Ok(map_service_status(&infos))
        })
    }
}

/// The wire form of a local envelope. Only `Reconcile`, `Stop` and `Recover`
/// travel; a local-only command (`Shutdown`) aimed at the daemon is a caller
/// bug reported as such rather than silently dropped.
pub(super) fn wire_submit_request(
    submission: &CoreSubmission,
) -> Result<CoreSubmitReq<'static>, CoreError> {
    use nyanpasu_core_manager::{ConfigInput, CoreCommand};
    let envelope = &submission.envelope;
    let command = match &envelope.command {
        CoreCommand::Reconcile(request) => {
            let ConfigInput::Inline {
                bytes,
                expected_digest,
            } = &request.config;
            let config = String::from_utf8(bytes.clone()).map_err(|_| {
                CoreError::new(
                    CoreErrorKind::InvalidConfig,
                    "config bytes are not UTF-8 text",
                    false,
                )
            })?;
            CoreCommandInfo::Reconcile {
                core_type: Cow::Owned(match &submission.core_type {
                    Some(core_type) => core_type.clone(),
                    None => app_core_kind_to_type(request.core.kind)?,
                }),
                config: Cow::Owned(config),
                expected_digest: expected_digest.clone().map(Cow::Owned),
                expected_applied: request.expected_applied.as_ref().map(|revision| {
                    nyanpasu_ipc::api::status::RevisionIdInfo {
                        epoch: revision.epoch.get(),
                        generation: revision.generation,
                        effective_hash: revision.effective_hash.clone(),
                    }
                }),
            }
        }
        CoreCommand::Stop => CoreCommandInfo::Stop,
        CoreCommand::Recover => CoreCommandInfo::Recover,
        CoreCommand::Shutdown => {
            return Err(CoreError::new(
                CoreErrorKind::OperationConflict,
                "the daemon's lifecycle is not controlled over the core wire",
                false,
            ));
        }
    };
    Ok(CoreSubmitReq {
        operation_id: Cow::Owned(envelope.operation_id.to_string()),
        command,
    })
}

/// Fallback used only when a caller did not carry the intent's full
/// `CoreType`. `CoreKind` collapses alpha channels, so facade submissions
/// always provide the original type. `Meow` has no fallback wire type.
fn app_core_kind_to_type(
    kind: nyanpasu_core_manager::CoreKind,
) -> Result<nyanpasu_utils::core::CoreType, CoreError> {
    use nyanpasu_utils::core::{ClashCoreType, CoreType};
    match kind {
        nyanpasu_core_manager::CoreKind::Mihomo => Ok(CoreType::Clash(ClashCoreType::Mihomo)),
        nyanpasu_core_manager::CoreKind::ClashRust => Ok(CoreType::Clash(ClashCoreType::ClashRust)),
        nyanpasu_core_manager::CoreKind::ClashPremium => {
            Ok(CoreType::Clash(ClashCoreType::ClashPremium))
        }
        other => Err(CoreError::new(
            CoreErrorKind::Internal,
            format!("core kind {other:?} has no wire core type"),
            false,
        )),
    }
}

fn map_service_status(infos: &CoreInfos) -> CoreStatusSnapshot {
    CoreStatusSnapshot {
        // A daemon too old to publish `detail` leaves this unknown. The coarse
        // `CoreInfos::state` cannot stand in: it collapses Starting and
        // Restarting into a Stopped shape, which would read as a stop proof.
        state: infos.detail.clone(),
        state_changed_at: infos.state_changed_at,
        revision: infos.revision.as_ref().map(|revision| revision.id()),
        healthy: infos.health.as_ref().map(|health| {
            matches!(
                health.state,
                nyanpasu_ipc::api::status::CoreHealthState::Healthy
            )
        }),
    }
}

/// Shared handle shape the router stores and hands out.
pub type EndpointHandle = Arc<dyn ControlEndpoint>;

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_ipc::{
        api::{ResponseCode, status::CoreState},
        client::ClientError,
    };

    fn infos(detail: Option<CoreStateDetail>) -> CoreInfos {
        CoreInfos {
            r#type: None,
            // Deliberately the shape a stopped core would publish: the point
            // is that the coarse state must not be consulted at all.
            state: CoreState::Stopped(None),
            state_changed_at: 7,
            config_path: None,
            controller: None,
            health: None,
            revision: None,
            detail,
        }
    }

    /// A daemon too old to publish `detail` leaves the state unknown. Reading
    /// the coarse `CoreState` instead would manufacture a `Stopped` that the
    /// handoff would then accept as proof.
    #[test]
    fn missing_service_detail_maps_to_unknown() {
        assert_eq!(map_service_status(&infos(None)).state, None);
        assert_eq!(
            map_service_status(&infos(Some(CoreStateDetail::Starting { epoch: 3 }))).state,
            Some(CoreStateDetail::Starting { epoch: 3 })
        );
    }

    fn server_error(kind: &str, retryable: Option<bool>) -> ClientError {
        ClientError::Server {
            operation: "/core/v2/submit",
            code: ResponseCode::OtherError,
            msg: "boom".into(),
            error_kind: Some(kind.to_owned()),
            retryable,
        }
    }

    #[test]
    fn a_classified_server_failure_keeps_its_kind_and_retryability() {
        for (kind, retryable, expected_kind, expected_retryable) in [
            (
                "operation_conflict",
                None,
                Some(CoreErrorKind::OperationConflict),
                false,
            ),
            ("queue_full", None, Some(CoreErrorKind::QueueFull), true),
            // The daemon's own answer outranks the kind's default.
            (
                "operation_conflict",
                Some(true),
                Some(CoreErrorKind::OperationConflict),
                true,
            ),
            // A kind this build does not know stays unclassified.
            ("made_up", None, None, false),
        ] {
            let error = map_client_error(server_error(kind, retryable));
            assert_eq!(error.kind, expected_kind, "kind for {kind}");
            assert_eq!(error.retryable, expected_retryable, "retryable for {kind}");
        }
    }

    #[test]
    fn a_transport_failure_is_backend_unavailable_and_retryable() {
        let error = map_client_error(ClientError::EmptyData {
            operation: "/core/v2/submit",
        });
        assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
        assert!(error.retryable);
    }
}
