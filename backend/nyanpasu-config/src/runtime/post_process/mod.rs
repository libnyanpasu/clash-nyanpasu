//! What post-processing does to the runtime config, described as data.

mod apply;
mod ownership;

pub use apply::{apply_defaults, apply_settings};
pub use ownership::{OwnedField, OwnedFields, Ownership, OwnershipEntries};
