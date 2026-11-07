//! Explicit application-boundary conversions for runtime.

use nyanpasu_core::client::{
    clash_info::ClashInfo as DomainClashInfo,
    direct_egress::DirectEgress as DomainDirectEgress,
    runtime_inspection::{
        RuntimeInspection as DomainRuntimeInspection,
        RuntimeInspectionContent as DomainRuntimeInspectionContent,
        RuntimeInspectionDiff as DomainRuntimeInspectionDiff,
        RuntimeInspectionNode as DomainRuntimeInspectionNode,
    },
};
use nyanpasu_rpc_protocol::wire;

use super::IntoWire;

impl IntoWire for DomainClashInfo {
    type Wire = wire::runtime::ClashInfo;

    fn into_wire(self) -> Self::Wire {
        wire::runtime::ClashInfo {
            port: self.port,
            server: self.server,
            secret: self.secret,
        }
    }
}

impl IntoWire for DomainRuntimeInspection {
    type Wire = wire::runtime::RuntimeInspection;

    fn into_wire(self) -> Self::Wire {
        wire::runtime::RuntimeInspection {
            snapshot_id: self.snapshot_id,
            applied: self.applied,
            effective_pending: self.effective_pending,
            effective_revision: self.effective_revision,
            revision: self.revision,
            target_core: self.target_core,
            root_id: self.root_id,
            final_id: self.final_id,
            nodes: self
                .nodes
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
        }
    }
}

impl IntoWire for DomainRuntimeInspectionNode {
    type Wire = wire::runtime::RuntimeInspectionNode;

    fn into_wire(self) -> Self::Wire {
        wire::runtime::RuntimeInspectionNode {
            id: self.id,
            tag: self.tag,
            next: self.next,
            has_logs: self.has_logs,
            max_log_level: self.max_log_level,
            changed_fields: self.changed_fields,
        }
    }
}

impl IntoWire for DomainRuntimeInspectionContent {
    type Wire = wire::runtime::RuntimeInspectionContent;

    fn into_wire(self) -> Self::Wire {
        wire::runtime::RuntimeInspectionContent {
            yaml: self.yaml,
            diff: self.diff.map(|value| value.into_wire()),
            logs: self.logs,
        }
    }
}

impl IntoWire for DomainRuntimeInspectionDiff {
    type Wire = wire::runtime::RuntimeInspectionDiff;

    fn into_wire(self) -> Self::Wire {
        wire::runtime::RuntimeInspectionDiff {
            parent_id: self.parent_id,
            hunks: self.hunks,
        }
    }
}

impl IntoWire for DomainDirectEgress {
    type Wire = wire::runtime::DirectEgress;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainDirectEgress::Disabled => wire::runtime::DirectEgress::Disabled,
            DomainDirectEgress::TunEnabled => wire::runtime::DirectEgress::TunEnabled,
            DomainDirectEgress::Probed { ipv4, ipv6 } => {
                wire::runtime::DirectEgress::Probed { ipv4, ipv6 }
            }
        }
    }
}
