//! Profiles domain state (PR-3). Tauri-free.

pub mod actor;
pub(crate) mod jobs;
mod scheduler;
pub mod sources;

pub use actor::{
    CommitReport, ImportOperationToken, ProfilesActor, ProfilesActorArgs, ProfilesActorMessage,
    ProfilesActorState, RefreshAttemptToken, RefreshOrigin, RefreshOutcome, ReorderOp,
};
