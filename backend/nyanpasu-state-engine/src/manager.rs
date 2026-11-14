//! Versioned document persistence for source owners.

mod persistent_state;

pub use persistent_state::{
    PersistentStateManager, PersistentStateManagerSetup, ReplaceIfVersionError,
    ReplaceIfVersionResult,
};
