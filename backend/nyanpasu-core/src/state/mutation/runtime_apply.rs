//! Internal evidence of the runtime accepted by the core, used for recovery.

use nyanpasu_config::application::ClashCore;

use super::intent::CoreRunIntent;
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
    /// Whether the app wants this runtime running at all. A core the user
    /// stopped is a target too, and a recovery must not start it.
    pub run_intent: CoreRunIntent,
    /// The binding this apply produced. A recovery may legitimately land on a
    /// newer instance generation, so only its content identity is a target.
    pub binding: AppliedConfigBinding,
    /// The ports this apply bound. Confirming them is gated on this receipt.
    pub ports: CandidatePortBindings,
    /// The committed target these bytes were built from, when the build had
    /// one: what tells a running receipt from a newer target still owed.
    pub target: Option<String>,
}
