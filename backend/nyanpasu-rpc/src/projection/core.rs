//! Explicit application-boundary conversions for core.

use nyanpasu_core::{
    actor_v2::{
        CoreStatusInfo as DomainCoreStatusInfo, EndpointConnectivity as DomainEndpointConnectivity,
        service_actor::{
            ServiceHostStatus as DomainServiceHostStatus, ServicePhase as DomainServicePhase,
        },
    },
    service::compat::ServiceCompat as DomainServiceCompat,
};
use nyanpasu_rpc_protocol::wire;

use super::IntoWire;

impl IntoWire for DomainEndpointConnectivity {
    type Wire = wire::core::EndpointConnectivity;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainEndpointConnectivity::Connected => wire::core::EndpointConnectivity::Connected,
            DomainEndpointConnectivity::ShutDown => wire::core::EndpointConnectivity::ShutDown,
            DomainEndpointConnectivity::HandingOff { from, to } => {
                wire::core::EndpointConnectivity::HandingOff { from, to }
            }
            DomainEndpointConnectivity::Degraded { desired, reason } => {
                wire::core::EndpointConnectivity::Degraded { desired, reason }
            }
        }
    }
}

impl IntoWire for DomainCoreStatusInfo {
    type Wire = wire::core::CoreStatusInfo;

    fn into_wire(self) -> Self::Wire {
        wire::core::CoreStatusInfo {
            controller: self.controller,
            host: self.host,
            connectivity: self.connectivity.into_wire(),
            generation: self.generation,
            state: self.state,
            state_changed_at: self.state_changed_at,
            revision: self.revision,
            healthy: self.healthy,
        }
    }
}

impl IntoWire for DomainServicePhase {
    type Wire = wire::core::ServicePhase;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainServicePhase::Probing => wire::core::ServicePhase::Probing,
            DomainServicePhase::NotInstalled => wire::core::ServicePhase::NotInstalled,
            DomainServicePhase::DaemonStopped => wire::core::ServicePhase::DaemonStopped,
            DomainServicePhase::Installing => wire::core::ServicePhase::Installing,
            DomainServicePhase::StartingDaemon => wire::core::ServicePhase::StartingDaemon,
            DomainServicePhase::Ready => wire::core::ServicePhase::Ready,
            DomainServicePhase::Incompatible => wire::core::ServicePhase::Incompatible,
            DomainServicePhase::Restarting => wire::core::ServicePhase::Restarting,
            DomainServicePhase::Exhausted => wire::core::ServicePhase::Exhausted,
            DomainServicePhase::Uninstalling => wire::core::ServicePhase::Uninstalling,
            DomainServicePhase::Unknown => wire::core::ServicePhase::Unknown,
        }
    }
}

impl IntoWire for DomainServiceCompat {
    type Wire = wire::core::ServiceCompat;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainServiceCompat::Unknown => wire::core::ServiceCompat::Unknown,
            DomainServiceCompat::Compatible { server_version } => {
                wire::core::ServiceCompat::Compatible { server_version }
            }
            DomainServiceCompat::Incompatible {
                server_version,
                required_major,
                required_min,
            } => wire::core::ServiceCompat::Incompatible {
                server_version,
                required_major,
                required_min,
            },
            DomainServiceCompat::Unparsable { server_version } => {
                wire::core::ServiceCompat::Unparsable { server_version }
            }
        }
    }
}

impl IntoWire for DomainServiceHostStatus {
    type Wire = wire::core::ServiceHostStatus;

    fn into_wire(self) -> Self::Wire {
        wire::core::ServiceHostStatus {
            name: self.name,
            version: self.version,
            status: self.status,
            server: self.server,
            phase: self.phase.into_wire(),
            compat: self.compat.into_wire(),
            restart_attempts: self.restart_attempts,
        }
    }
}
