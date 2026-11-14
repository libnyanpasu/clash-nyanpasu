//! The desired core run state.

/// Whether the user wants a core running at all.
///
/// A stopped core is an intent, not a failure: an ordinary save must not start
/// one behind the user's back just because it has something to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreRunIntent {
    Running,
    StoppedByUser,
}
