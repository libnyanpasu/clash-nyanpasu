//! Structural failures only; transform-level failures go to step logs (spec D7).

use thiserror::Error;

use crate::{
    profile::{ManagedProfilePath, ProfileId},
    runtime::snapshot::SnapshotBuildError,
};

use super::ports::PortError;

#[derive(Debug, Error)]
pub enum RuntimePipelineError {
    #[error("selected profile {0} not found")]
    SelectedProfileNotFound(ProfileId),

    #[error("selected profile {0} is not a Config")]
    SelectedProfileNotConfig(ProfileId),

    #[error("composition {composition} member {member} invalid: {reason}")]
    CompositionMemberInvalid {
        composition: ProfileId,
        member: ProfileId,
        reason: String,
    },

    #[error("read profile {profile} content at {path}: {source}")]
    ContentSource {
        profile: ProfileId,
        path: ManagedProfilePath,
        #[source]
        source: PortError,
    },

    #[error("parse profile {profile} as config: {message}")]
    ParseProfile { profile: ProfileId, message: String },

    #[error(transparent)]
    Snapshot(#[from] SnapshotBuildError),

    /// Theoretically unreachable invariant breaks (e.g. guard serialization).
    #[error("internal executor invariant: {0}")]
    Internal(String),
}
