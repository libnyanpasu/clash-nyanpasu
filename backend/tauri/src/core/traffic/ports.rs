//! Consumer-owned ports for the traffic actor.
use std::time::Duration;

/// The selected profile; labels the connections first seen while it is selected.
#[cfg_attr(test, mockall::automock)]
pub trait ProfileSelection: Send + Sync + 'static {
    fn current(&self) -> Option<String>;
}

/// How long recorded traffic is kept, read afresh at every flush.
#[cfg_attr(test, mockall::automock)]
pub trait RetentionPolicy: Send + Sync + 'static {
    /// `None` keeps it forever.
    fn retention(&self) -> Option<Duration>;
}

/// The wall clock: buckets, close times and retention are all counted from it.
#[cfg_attr(test, mockall::automock)]
pub trait Clock: Send + Sync + 'static {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> i64;
}
