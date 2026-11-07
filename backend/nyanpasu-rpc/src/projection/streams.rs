//! Explicit application-boundary conversions for streams.

use std::sync::Arc;

use nyanpasu_core::clash::{
    connection_rates::{
        ClashConnectionsSummary as DomainClashConnectionsSummary, TrafficRate as DomainTrafficRate,
    },
    ws::{
        ClashConnectionDetails as DomainClashConnectionDetails,
        ClashConnectionsConnectorEvent as DomainClashConnectionsConnectorEvent,
        ClashConnectionsConnectorState as DomainClashConnectionsConnectorState,
        ClashConnectionsInfo as DomainClashConnectionsInfo, ClashWsKind as DomainClashWsKind,
        ClashWsMemory as DomainClashWsMemory, ClashWsPacket as DomainClashWsPacket,
        ClashWsRecording as DomainClashWsRecording, ClashWsSnapshot as DomainClashWsSnapshot,
        ClashWsTraffic as DomainClashWsTraffic, ClashWsUpdate as DomainClashWsUpdate,
    },
};
use nyanpasu_rpc_protocol::wire;

use super::{IntoApplication, IntoWire};

impl IntoWire for DomainTrafficRate {
    type Wire = wire::streams::TrafficRate;

    fn into_wire(self) -> Self::Wire {
        wire::streams::TrafficRate {
            download: self.download,
            upload: self.upload,
        }
    }
}

impl IntoWire for DomainClashConnectionsSummary {
    type Wire = wire::streams::ClashConnectionsSummary;

    fn into_wire(self) -> Self::Wire {
        wire::streams::ClashConnectionsSummary {
            download_total: self.download_total,
            upload_total: self.upload_total,
            download_speed: self.download_speed,
            upload_speed: self.upload_speed,
            memory: self.memory,
            connection_count: self.connection_count,
            member_rates: self
                .member_rates
                .into_iter()
                .map(|(key, value)| (key, value.into_wire()))
                .collect(),
        }
    }
}

impl IntoWire for DomainClashConnectionsInfo {
    type Wire = wire::streams::ClashConnectionsInfo;

    fn into_wire(self) -> Self::Wire {
        wire::streams::ClashConnectionsInfo {
            download_total: self.download_total,
            upload_total: self.upload_total,
            download_speed: self.download_speed,
            upload_speed: self.upload_speed,
        }
    }
}

impl IntoWire for DomainClashConnectionsConnectorEvent {
    type Wire = wire::streams::ClashConnectionsConnectorEvent;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainClashConnectionsConnectorEvent::StateChanged(value0) => {
                wire::streams::ClashConnectionsConnectorEvent::StateChanged(value0.into_wire())
            }
            DomainClashConnectionsConnectorEvent::Update(value0) => {
                wire::streams::ClashConnectionsConnectorEvent::Update(value0.into_wire())
            }
        }
    }
}

impl IntoWire for DomainClashConnectionsConnectorState {
    type Wire = wire::streams::ClashConnectionsConnectorState;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainClashConnectionsConnectorState::Disconnected => {
                wire::streams::ClashConnectionsConnectorState::Disconnected
            }
            DomainClashConnectionsConnectorState::Connecting => {
                wire::streams::ClashConnectionsConnectorState::Connecting
            }
            DomainClashConnectionsConnectorState::Connected => {
                wire::streams::ClashConnectionsConnectorState::Connected
            }
        }
    }
}

impl IntoWire for DomainClashWsKind {
    type Wire = wire::streams::ClashWsKind;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainClashWsKind::Connections => wire::streams::ClashWsKind::Connections,
            DomainClashWsKind::Logs => wire::streams::ClashWsKind::Logs,
            DomainClashWsKind::Traffic => wire::streams::ClashWsKind::Traffic,
            DomainClashWsKind::Memory => wire::streams::ClashWsKind::Memory,
        }
    }
}

impl IntoApplication for wire::streams::ClashWsKind {
    type Application = DomainClashWsKind;

    fn into_application(self) -> Self::Application {
        match self {
            wire::streams::ClashWsKind::Connections => DomainClashWsKind::Connections,
            wire::streams::ClashWsKind::Logs => DomainClashWsKind::Logs,
            wire::streams::ClashWsKind::Traffic => DomainClashWsKind::Traffic,
            wire::streams::ClashWsKind::Memory => DomainClashWsKind::Memory,
        }
    }
}

impl IntoWire for DomainClashWsRecording {
    type Wire = wire::streams::ClashWsRecording;

    fn into_wire(self) -> Self::Wire {
        wire::streams::ClashWsRecording {
            connections: self.connections,
            logs: self.logs,
            traffic: self.traffic,
            memory: self.memory,
        }
    }
}

impl IntoWire for DomainClashWsMemory {
    type Wire = wire::streams::ClashWsMemory;

    fn into_wire(self) -> Self::Wire {
        wire::streams::ClashWsMemory {
            inuse: self.inuse,
            oslimit: self.oslimit,
        }
    }
}

impl IntoWire for DomainClashWsTraffic {
    type Wire = wire::streams::ClashWsTraffic;

    fn into_wire(self) -> Self::Wire {
        wire::streams::ClashWsTraffic {
            up: self.up,
            down: self.down,
        }
    }
}

impl IntoWire for DomainClashWsSnapshot {
    type Wire = wire::streams::ClashWsSnapshot;

    fn into_wire(self) -> Self::Wire {
        wire::streams::ClashWsSnapshot {
            sequence: self.sequence,
            state: self.state.into_wire(),
            recording: self.recording.into_wire(),
            connections: self
                .connections
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            traffic: self
                .traffic
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            memory: self
                .memory
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
        }
    }
}

impl IntoWire for DomainClashWsPacket {
    type Wire = wire::streams::ClashWsPacket;

    fn into_wire(self) -> Self::Wire {
        wire::streams::ClashWsPacket {
            sequence: self.sequence,
            update: self.update.into_wire(),
        }
    }
}

impl IntoWire for DomainClashWsUpdate {
    type Wire = wire::streams::ClashWsUpdate;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainClashWsUpdate::Reset(value0) => {
                wire::streams::ClashWsUpdate::Reset(Box::new((*value0).into_wire()))
            }
            DomainClashWsUpdate::StateChanged(value0) => {
                wire::streams::ClashWsUpdate::StateChanged(value0.into_wire())
            }
            DomainClashWsUpdate::ConnectionsUpdated(value0) => {
                wire::streams::ClashWsUpdate::ConnectionsUpdated(value0.into_wire())
            }
            DomainClashWsUpdate::TrafficUpdated(value0) => {
                wire::streams::ClashWsUpdate::TrafficUpdated(value0.into_wire())
            }
            DomainClashWsUpdate::MemoryUpdated(value0) => {
                wire::streams::ClashWsUpdate::MemoryUpdated(value0.into_wire())
            }
            DomainClashWsUpdate::RecordingChanged(value0) => {
                wire::streams::ClashWsUpdate::RecordingChanged(value0.into_wire())
            }
            DomainClashWsUpdate::HistoryCleared(value0) => {
                wire::streams::ClashWsUpdate::HistoryCleared(value0.into_wire())
            }
        }
    }
}

/// Sending view that retains the immutable source snapshot instead of copying connections.
/// Receiving clients continue to own `wire::streams::ClashConnectionDetails`.
#[derive(Debug, serde::Serialize)]
#[serde(transparent)]
pub struct ClashConnectionDetailsSend(Arc<DomainClashConnectionDetails>);

impl specta::Type for ClashConnectionDetailsSend {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        wire::streams::ClashConnectionDetails::definition(types)
    }
}

impl IntoWire for &Arc<DomainClashConnectionDetails> {
    type Wire = ClashConnectionDetailsSend;

    fn into_wire(self) -> Self::Wire {
        ClashConnectionDetailsSend(self.clone())
    }
}
