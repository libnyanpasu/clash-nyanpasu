//! Internal evidence of the runtime accepted by the core.

use nyanpasu_config::application::ClashCore;

use crate::{
    actor_v2::facade::AppliedConfigBinding,
    client::{ports::CandidatePortBindings, runtime::RuntimeRevision},
};
use nyanpasu_runtime_values::execution_host::ExecutionHost;

/// The accepted runtime identity and port bindings used to verify observations.
#[derive(Debug, Clone)]
pub(crate) struct RuntimeApplyReceipt {
    /// Which build produced this application, used when retrying product publication.
    pub revision: RuntimeRevision,
    pub target_core: ClashCore,
    pub host: ExecutionHost,
    pub instance_id: Option<String>,
    /// The binding confirmed by this application, used to verify a running instance.
    pub binding: AppliedConfigBinding,
    /// The ports this apply bound. Confirming them is gated on this receipt.
    pub ports: CandidatePortBindings,
    /// The committed target these bytes were built from, when the build had
    /// one: what tells a running receipt from a newer target still owed.
    pub target: Option<String>,
}
