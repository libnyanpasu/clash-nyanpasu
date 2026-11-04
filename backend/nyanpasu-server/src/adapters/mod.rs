//! Honest adapters for the ports a headless host cannot fully provide.
//!
//! Each one answers truthfully: an unsupported capability returns an error or an
//! unsupported status and never reports success. Where a port's type has no way to
//! say "unsupported" the adapter is marked `SPIKE` with the finding it records.

mod effects;
mod host;

pub use effects::UnsupportedEffects;
pub use host::{
    CoreBinaryVersions, DisabledDebugHttp, NoAccelerators, NoOsProxy, NoProxyEnvironment,
    NoServiceHost, NoWindows, TracingUiEvents,
};
