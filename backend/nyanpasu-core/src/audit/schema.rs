use std::time::Duration;

use nyanpasu_tsdb::{
    ColumnSpec, ColumnType, EventKey, EventStreamSpec, ExtentSpec, Schema, StreamId,
};

pub(super) const AUDIT: StreamId = StreamId(0);
pub(super) const AUDIT_WINDOW: Duration = Duration::from_secs(10 * 60);
pub(super) const TRANSPORT: usize = 0;
pub(super) const PHASE: usize = 5;
pub(super) const OUTCOME: usize = 6;

pub(super) fn schema() -> Schema {
    Schema {
        version: 1,
        labels: vec![],
        series: None,
        events: vec![EventStreamSpec {
            name: "rpc_audit",
            key: EventKey::Timestamp,
            extent: ExtentSpec::Window(AUDIT_WINDOW),
            columns: [
                ("transport", ColumnType::Dict, false),
                ("method", ColumnType::Dict, false),
                ("owner", ColumnType::Dict, false),
                ("invocation_id", ColumnType::U64, false),
                ("request_id", ColumnType::U64, true),
                ("phase", ColumnType::Dict, false),
                ("outcome", ColumnType::Dict, true),
                ("kind", ColumnType::Dict, true),
                ("code", ColumnType::Dict, true),
                ("duration_ns", ColumnType::U64, false),
            ]
            .map(|(name, ty, nullable)| ColumnSpec { name, ty, nullable })
            .into(),
        }],
    }
}
