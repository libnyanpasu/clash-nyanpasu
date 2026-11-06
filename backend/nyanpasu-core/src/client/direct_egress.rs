//! The public address the core's DIRECT outbound leaves from. DIRECT uses the
//! machine's own route, so the app asks over that route itself: STUN servers
//! first, then the HTTP lookup services the user accepted.
use std::net::{Ipv4Addr, Ipv6Addr};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DirectEgress {
    Disabled,
    /// Nothing was probed: TUN mode captures the app's own requests and routes
    /// them by rule, so an echo could come back from a proxy exit.
    TunEnabled,
    /// `None` for a family no source gave an address over, such as a network
    /// without IPv6.
    Probed {
        ipv4: Option<Ipv4Addr>,
        ipv6: Option<Ipv6Addr>,
    },
}
