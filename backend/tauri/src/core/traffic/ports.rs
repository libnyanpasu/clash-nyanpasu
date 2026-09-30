//! Consumer-owned ports for the traffic actor.

/// The selected profile; a change of it starts a new traffic session.
#[cfg_attr(test, mockall::automock)]
pub trait ProfileSelection: Send + Sync + 'static {
    fn current(&self) -> Option<String>;
}
