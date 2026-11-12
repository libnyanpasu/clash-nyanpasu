//! Internal evidence of the runtime accepted by the core, used for recovery.

use std::sync::Arc;

use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{CoreSpec, LocalIpcSettings};

use super::intent::CoreRunIntent;
use crate::{
    actor_v2::facade::AppliedConfigBinding,
    client::{ports::CandidatePortBindings, runtime::RuntimeRevision},
};
use nyanpasu_runtime_values::execution_host::ExecutionHost;

/// What the core actually accepted, recorded the moment it confirms an apply
/// and never derived from a file or an inspection (v2 §5.3/§5.4, C3).
///
/// It is deliberately decoupled from
/// [`RuntimeSnapshot`](crate::client::runtime::RuntimeSnapshot): `promoted` tracks
/// a derived product file and `applied` waits for an effective-config
/// inspection that may arrive late or not at all, so neither can serve as the
/// baseline a recovery has to restore. This can.
#[derive(Debug, Clone)]
pub(crate) struct RuntimeApplyReceipt {
    /// Which build produced these bytes. Diagnostic: a restore targets the
    /// document, not the revision that happened to carry it.
    #[allow(dead_code)]
    pub revision: RuntimeRevision,
    /// The exact document the core accepted — the same bytes the validator
    /// checked, never a re-serialization of the snapshot.
    pub config_text: Arc<str>,
    /// `nyanpasu_core_manager::payload_digest` of `config_text`: the change
    /// identity the core verified on receipt.
    pub config_digest: String,
    pub target_core: ClashCore,
    pub core_spec: CoreSpec,
    pub host: ExecutionHost,
    /// Whether the app wants this runtime running at all. A core the user
    /// stopped is a target too, and a recovery must not start it.
    pub run_intent: CoreRunIntent,
    pub local_ipc: LocalIpcSettings,
    /// The binding this apply produced. A recovery may legitimately land on a
    /// newer instance generation, so only its content identity is a target.
    pub binding: AppliedConfigBinding,
    /// The ports this apply bound. Confirming them is gated on this receipt.
    pub ports: CandidatePortBindings,
    /// The committed target these bytes were built from, when the build had
    /// one: what tells a running receipt from a newer target still owed.
    pub target: Option<String>,
}
