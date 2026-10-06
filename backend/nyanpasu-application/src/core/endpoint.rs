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

use std::{sync::Arc, time::Duration};

use super::api::{ApiError, InstanceApiPort};
use nyanpasu_core_manager::{CoreCommandEnvelope, CoreError, CoreErrorKind, CoreKind, OperationId};
use nyanpasu_ipc::api::{core::v2::OperationInfo, status::CoreStateDetail};

/// Which controller owns the runtime. The app perceives the difference in
/// exactly two places: this tag on the endpoint slot, and the handoff
/// protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionHost {
    Local,
    Service,
}

/// The wire core type collapsed to the coarser `CoreKind` used by the
/// application. SingBox has no `CoreKind` counterpart yet and stays unknown.
pub fn wire_core_type_to_kind(core_type: &nyanpasu_utils::core::CoreType) -> Option<CoreKind> {
    use nyanpasu_utils::core::{ClashCoreType, CoreType};
    match core_type {
        CoreType::Clash(ClashCoreType::Mihomo | ClashCoreType::MihomoAlpha) => {
            Some(CoreKind::Mihomo)
        }
        CoreType::Clash(ClashCoreType::ClashRust | ClashCoreType::ClashRustAlpha) => {
            Some(CoreKind::ClashRust)
        }
        CoreType::Clash(ClashCoreType::ClashPremium) => Some(CoreKind::ClashPremium),
        CoreType::Clash(ClashCoreType::Meow) => Some(CoreKind::Meow),
        CoreType::SingBox => None,
    }
}

/// The host's canonical core status, projected field by field for the app.
/// No field here is ever derived by the router itself.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreStatusSnapshot {
    pub controller: Option<nyanpasu_ipc::api::status::CoreControllerInfo>,
    /// `None` means the host published no state this router can trust — an
    /// unmapped future variant, or a daemon that answered without a detail.
    /// No consumer may read it as `Stopped`: "we do not know" is the one
    /// answer a stop proof must never accept.
    pub state: Option<CoreStateDetail>,
    pub state_changed_at: i64,
    /// The applied revision's CAS identity, when one is running.
    pub revision: Option<nyanpasu_ipc::api::status::RevisionIdInfo>,
    /// The source identity of that same applied revision.
    ///
    /// Deliberately kept next to the CAS identity rather than folded into it:
    /// `RevisionIdInfo` is what a reconcile sends back as `expected_applied`,
    /// and `source_hash` takes no part in that comparison. It is here because
    /// it is the only document identity that survives a restart — the effective
    /// hash covers the epoch-specific controller endpoint the manager stamps
    /// into every new epoch, so two epochs of one unchanged configuration never
    /// share it.
    pub source_hash: Option<String>,
    /// Healthy / unhealthy, when the host reports it.
    pub healthy: Option<bool>,
    /// The kind of core the host has actually applied -- not the desired
    /// config. `None` when the host does not report an applied identity at
    /// all (an old daemon with no `type`, or a manager that has never
    /// applied anything). `CoreKind` collapses alpha channels on purpose: a
    /// running mihomo-alpha and a running mihomo both report `Mihomo`, and a
    /// consumer deciding whether an applied core "may be" some target must
    /// treat them the same way it treats an unknown `state` -- as a fact it
    /// cannot rule out, not as a mismatch.
    pub applied_kind: Option<CoreKind>,
}

#[derive(Debug, Clone)]
pub struct CoreSubmission {
    pub expected_owner: Option<(ExecutionHost, u64)>,
    pub envelope: CoreCommandEnvelope,
    pub core_type: Option<nyanpasu_utils::core::CoreType>,
}

/// One advisory config check. It carries the document twice on purpose: the
/// in-process control plane takes the bytes inline, while the daemon's
/// `/core/check` opens a file itself, and both must see the same document the
/// reconcile will submit.
#[derive(Debug, Clone)]
pub struct CheckSubmission {
    pub core_spec: nyanpasu_core_manager::CoreSpec,
    pub core_type: nyanpasu_utils::core::CoreType,
    pub config_bytes: Vec<u8>,
    /// `payload_digest` of `config_bytes`, verified on receipt where the host
    /// supports it.
    pub digest: String,
    /// A private file holding exactly `config_bytes`, for a host that reads
    /// the config from disk rather than from the request.
    pub staged_config: Option<camino::Utf8PathBuf>,
}

/// A host's answer about a check. `Unsupported` is deliberately not an error:
/// "this host cannot check" and "the core rejected the config" are different
/// facts, and neither may be read as a pass.
#[derive(Debug)]
pub enum CheckSupport {
    Ran(Result<(), CoreError>),
    Unsupported { reason: String },
}

/// One host's control plane, as the router consumes it. Submit is the only
/// mutating call and is always envelope-shaped; waiting on an operation is a
/// read and deliberately not routed through the actor mailbox.
#[async_trait::async_trait]
pub trait ControlEndpoint: Send + Sync {
    async fn effective_config(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        Ok(None)
    }

    /// The applied process binding; absent means no usable API. Old hosts must
    /// fail explicitly rather than reconstructing credentials from globals.
    async fn api_connection(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        Err(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "the endpoint does not expose instance-bound API access",
            false,
        ))
    }

    /// Builds the concrete API adapter bound to this endpoint and its current
    /// process credentials. Adapters must not look up another instance.
    fn api_backend(
        &self,
        _binding: &nyanpasu_ipc::api::core::v2::CoreApiConnection,
    ) -> Result<Arc<dyn InstanceApiPort>, ApiError> {
        Err(ApiError::Unavailable(
            "the endpoint has no Clash API adapter".into(),
        ))
    }

    /// Ordered lifecycle notifications, used to wake API revocation checks.
    /// The periodic authority check also covers lost/coalesced notifications.
    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        Ok(None)
    }

    fn host(&self) -> ExecutionHost;

    /// Advisory, read-only config validation. It never enters the mutating
    /// queue and is never a precondition for a change (core-manager amendment
    /// A2). The default is `Unsupported`: a host that has no check capability
    /// says so rather than answering "fine".
    async fn check_config(&self, _submission: CheckSubmission) -> CheckSupport {
        CheckSupport::Unsupported {
            reason: "this execution host exposes no config check".into(),
        }
    }

    /// Admission into the host's executor. Returns the operation's
    /// admission-time snapshot; the transaction survives this future's drop.
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError>;

    /// Long-poll the host's registry. `None` = unknown/evicted id (recover by
    /// re-reading status; the revision CAS blocks double application).
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo>;

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError>;
}

pub type ApiChanges = std::pin::Pin<Box<dyn futures::Stream<Item = Result<(), CoreError>> + Send>>;

pub type EndpointHandle = Arc<dyn ControlEndpoint>;
