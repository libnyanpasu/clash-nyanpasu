//! Consumer-owned ports for the traffic actor.
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalSourceIps {
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
}

/// A nonblocking view of the permitted, caller-probed local public addresses.
pub trait LocalSourceLocation: Send + Sync + 'static {
    fn addresses(&self) -> LocalSourceIps;
}

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
