//! Explicit application-boundary conversions for events.

use nyanpasu_core::client::StateChanged as DomainStateChanged;
use nyanpasu_rpc_protocol::wire;

use super::IntoWire;

impl IntoWire for DomainStateChanged {
    type Wire = wire::events::StateChanged;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainStateChanged::NyanpasuConfig => wire::events::StateChanged::NyanpasuConfig,
            DomainStateChanged::ClashConfig => wire::events::StateChanged::ClashConfig,
            DomainStateChanged::Profiles { files } => {
                wire::events::StateChanged::Profiles { files }
            }
            DomainStateChanged::Proxies => wire::events::StateChanged::Proxies,
        }
    }
}
