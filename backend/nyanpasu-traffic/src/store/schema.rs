use crate::Dimension;
use nyanpasu_tsdb::*;
use std::time::Duration;

pub(super) const MINUTE: TierId = TierId(0);
pub(super) const HOUR: TierId = TierId(1);
pub(super) const CLOSED: StreamId = StreamId(0);
pub(super) const TARGET: LabelId = LabelId(4);
pub(super) const LABELS: usize = 15;
pub(super) const MINUTE_MS: i64 = 60_000;
pub(super) const HOUR_MS: i64 = 3_600_000;

pub(super) fn schema() -> Schema {
    let mut labels: Vec<_> = Dimension::ALL
        .into_iter()
        .map(|d| LabelSpec { name: d.name() })
        .collect();
    labels.extend([LabelSpec { name: "rule_key" }, LabelSpec { name: "chains" }]);
    Schema {
        version: 1,
        labels,
        series: Some(SeriesSpec {
            dimensions: (0..LABELS as u16).map(LabelId).collect(),
            fields: ["upload", "download", "connections"]
                .map(|name| FieldSpec {
                    name,
                    aggregate: Aggregate::SaturatingSum,
                })
                .into(),
            tiers: vec![
                TierSpec {
                    name: "minute",
                    bucket: Duration::from_secs(60),
                    extent: Duration::from_secs(3600),
                },
                TierSpec {
                    name: "hour",
                    bucket: Duration::from_secs(3600),
                    extent: Duration::from_secs(86400),
                },
            ],
            fold: vec![vec![TARGET]],
        }),
        events: vec![EventStreamSpec {
            name: "closed",
            key: EventKey::Timestamp,
            extent: ExtentSpec::Window(Duration::from_secs(86400)),
            columns: [
                ("started_at", ColumnType::Timestamp, false),
                ("first_seen_at", ColumnType::Timestamp, false),
                ("upload", ColumnType::U64, false),
                ("download", ColumnType::U64, false),
                ("dimensions", ColumnType::Series, false),
                ("id", ColumnType::Identity, false),
                ("target", ColumnType::FoldedLabel(TARGET), true),
            ]
            .map(|(name, ty, nullable)| ColumnSpec { name, ty, nullable })
            .into(),
        }],
    }
}
