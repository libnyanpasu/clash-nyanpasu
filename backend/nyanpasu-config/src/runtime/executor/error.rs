//! Structural failures only; transform-level failures go to step logs (spec D7).

use serde::Serialize;
use snafu::Snafu;
use specta::Type;

use crate::{
    profile::{ManagedProfilePath, ProfileId},
    runtime::snapshot::SnapshotBuildError,
};

use super::ports::PortError;

/// Wire shape for the app's error channel: library sources are skipped and
/// reach the user only through the error's `Debug` detail.
#[derive(Debug, Snafu, Serialize, Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimePipelineError {
    #[snafu(display("selected profile {profile} not found"))]
    SelectedProfileNotFound { profile: ProfileId },

    #[snafu(display("selected profile {profile} is not a Config"))]
    SelectedProfileNotConfig { profile: ProfileId },

    #[snafu(display("composition {composition} member {member} invalid: {reason}"))]
    CompositionMemberInvalid {
        composition: ProfileId,
        member: ProfileId,
        reason: String,
    },

    #[snafu(display("read profile {profile} content at {path}: {source}"))]
    ContentSource {
        profile: ProfileId,
        path: ManagedProfilePath,
        #[serde(skip)]
        source: PortError,
    },

    #[snafu(display("parse profile {profile} as config: {message}"))]
    ParseProfile { profile: ProfileId, message: String },

    #[snafu(transparent)]
    Snapshot {
        #[serde(skip)]
        source: SnapshotBuildError,
    },

    /// Theoretically unreachable invariant breaks (e.g. guard serialization).
    #[snafu(display("internal executor invariant: {message}"))]
    Internal { message: String },
}
