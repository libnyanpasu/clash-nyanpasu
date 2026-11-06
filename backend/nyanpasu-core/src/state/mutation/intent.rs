//! Source command classification and the desired core run state.

/// What kind of command produced the candidate.
///
/// The split is the one the policy table needs, and it follows the command, not
/// its diff: asking for a specific core, host or profile to be running is a
/// different promise from saving a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    /// `select_core`, an `enable_service_mode` switch, and profile activation
    /// or deactivation: the user named something that has to be running.
    ExplicitSwitch,
    /// Every other source-config write: clash and overrides parameters, managed
    /// content updates, profile bookkeeping, application settings.
    // Chosen by the domain actors, which move onto the participant in T6.
    #[allow(dead_code)]
    Save,
}

/// Whether the user wants a core running at all.
///
/// A stopped core is an intent, not a failure: an ordinary save must not start
/// one behind the user's back just because it has something to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreRunIntent {
    Running,
    StoppedByUser,
}
