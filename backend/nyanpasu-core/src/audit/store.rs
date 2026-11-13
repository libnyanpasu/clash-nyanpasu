use std::time::{SystemTime, UNIX_EPOCH};

use nyanpasu_observers::audit::{
    Appended, AuditCursor, AuditFilter, AuditPage, AuditRow, AuditStore, AuditStoreError,
};
use nyanpasu_tsdb::{
    ErrorKind, Event, EventCondition, EventFilter, EventPredicate, EventQuery, EventRow,
    EventValue, Input, Maintain, Profile, RecoveryError, Retention, RetentionPolicy, TimeRange,
    Timestamp, Tsdb, Value,
    storage::{Direction, Storage},
};
use snafu::ResultExt as _;

use super::schema::{self, AUDIT, OUTCOME, PHASE, TRANSPORT};

/// Unique synchronous storage adapter, moved into and out of blocking tasks by
/// the audit owner. Opening always uses `Profile::low_end()`.
pub struct TsdbAuditStore<S: Storage> {
    db: Tsdb<S>,
    last_batch: u64,
}

#[derive(Debug, snafu::Snafu)]
enum Error {
    #[snafu(display("failed to decode audit row column {column}"))]
    DecodeColumn { column: &'static str },
    #[snafu(display("failed to decode audit text column {column}"))]
    DecodeText {
        column: &'static str,
        source: std::str::Utf8Error,
    },
}

impl<S: Storage> TsdbAuditStore<S> {
    /// Opens a dataset and restores its last committed batch identity without
    /// replaying diagnostic work. Call only on a blocking thread.
    ///
    /// # Errors
    /// Reports incompatible, corrupted, or unavailable tsdb storage.
    pub fn open(storage: S) -> Result<Self, RecoveryError> {
        let (db, last_batch) =
            Tsdb::open_with(storage, schema::schema(), Profile::low_end(), |view| {
                Ok(view.last_batch().unwrap_or(0))
            })?;
        Ok(Self { db, last_batch })
    }
}

pub(super) fn values(row: &AuditRow) -> [Value<'_>; 10] {
    fn text(value: Option<&str>) -> Value<'_> {
        value.map_or(Value::Null, |s| Value::Bytes(s.as_bytes()))
    }
    [
        Value::Bytes(row.transport.as_bytes()),
        Value::Bytes(row.method.as_bytes()),
        Value::Bytes(row.owner.as_bytes()),
        Value::U64(row.invocation_id),
        row.request_id.map_or(Value::Null, Value::U64),
        Value::Bytes(row.phase.as_bytes()),
        text(row.outcome.as_deref()),
        text(row.kind.as_deref()),
        text(row.code.as_deref()),
        Value::U64(row.duration_ns),
    ]
}

fn timestamp(now: SystemTime) -> Timestamp {
    Timestamp(now.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
    }))
}

impl<S: Storage> AuditStore for TsdbAuditStore<S> {
    fn append(&mut self, rows: &[AuditRow]) -> Result<Appended, AuditStoreError> {
        if rows.is_empty() {
            return Ok(Appended::default());
        }
        let values: Vec<_> = rows.iter().map(values).collect();
        let events: Vec<_> = rows
            .iter()
            .zip(&values)
            .map(|(row, values)| EventRow {
                stream: AUDIT,
                key: row.at,
                seq: self.db.next_event_seq(),
                values,
            })
            .collect();
        let now = timestamp(SystemTime::now());
        let batch_id = self.last_batch + 1;
        let input = Input {
            batch_id,
            now,
            series: &[],
            events: &events,
            aux: &[],
            pins: &[],
        };
        let written = self
            .db
            .write(&input, self.db.durability_due(now))
            .map_err(|source| AuditStoreError::Append {
                kind: if source.not_applied() {
                    source.kind()
                } else {
                    ErrorKind::BackendFailed
                },
                source: Box::new(source),
            })?;
        self.last_batch = batch_id;
        Ok(Appended { late: written.late })
    }

    fn maintain(&mut self, now: SystemTime, retention: Retention) -> Result<(), AuditStoreError> {
        self.db
            .maintain(&Maintain {
                now: timestamp(now),
                retention: &RetentionPolicy {
                    tiers: vec![],
                    streams: vec![retention],
                },
                // Match traffic's bounded maintenance work per owner turn.
                rounds: 16,
            })
            .map_err(|source| AuditStoreError::Maintain {
                kind: source.kind(),
                source: Box::new(source),
            })?;
        Ok(())
    }

    fn query(
        &mut self,
        filter: AuditFilter,
        before: Option<AuditCursor>,
        limit: usize,
    ) -> Result<AuditPage, AuditStoreError> {
        let snapshot = self
            .db
            .reader()
            .capture()
            .map_err(|source| AuditStoreError::Query {
                kind: source.kind(),
                source: Box::new(source),
            })?;
        let columns = [
            (TRANSPORT, filter.transport.as_deref()),
            (PHASE, filter.phase.as_deref()),
            (OUTCOME, filter.outcome.as_deref()),
        ]
        .map(|(column, value)| value.map(|value| (column, value.as_bytes())));
        let filters: Vec<_> = columns
            .iter()
            .flatten()
            .map(|(column, value)| EventFilter {
                column: *column,
                condition: EventCondition::BytesIn(std::slice::from_ref(value)),
            })
            .collect();
        let page = snapshot
            .scan_events(&EventQuery {
                stream: AUDIT,
                range: TimeRange::new(Timestamp(i64::MIN), Timestamp(i64::MAX))
                    .expect("full timestamp range is ordered"),
                direction: Direction::Backward,
                cursor: before.map(Into::into),
                predicate: &EventPredicate { filters: &filters },
                limit: limit.clamp(1, 500),
            })
            .map_err(|source| AuditStoreError::Query {
                kind: source.kind(),
                source: Box::new(source),
            })?;
        let rows = page
            .events()
            .iter()
            .map(decode)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| AuditStoreError::Query {
                kind: ErrorKind::Corrupted,
                source: Box::new(source),
            })?;
        Ok(AuditPage {
            rows,
            next: page.cursor().map(Into::into),
            dropped: 0,
        })
    }
}

fn decode(event: &Event) -> Result<AuditRow, Error> {
    let [
        transport,
        method,
        owner,
        EventValue::U64(invocation_id),
        request_id,
        phase,
        outcome,
        kind,
        code,
        EventValue::U64(duration_ns),
    ] = event.values.as_slice()
    else {
        return Err(Error::DecodeColumn { column: "row" });
    };
    let text = |value: &EventValue, column| -> Result<String, Error> {
        let EventValue::Bytes(bytes) = value else {
            return Err(Error::DecodeColumn { column });
        };
        Ok(std::str::from_utf8(bytes)
            .context(DecodeTextSnafu { column })?
            .to_owned())
    };
    let optional = |value: &EventValue, column| match value {
        EventValue::Null => Ok(None),
        value => text(value, column).map(Some),
    };
    Ok(AuditRow {
        at: event.key,
        transport: text(transport, "transport")?,
        method: text(method, "method")?,
        owner: text(owner, "owner")?,
        invocation_id: *invocation_id,
        request_id: match request_id {
            EventValue::Null => None,
            EventValue::U64(value) => Some(*value),
            _ => {
                return Err(Error::DecodeColumn {
                    column: "request_id",
                });
            }
        },
        phase: text(phase, "phase")?,
        outcome: optional(outcome, "outcome")?,
        kind: optional(kind, "kind")?,
        code: optional(code, "code")?,
        duration_ns: *duration_ns,
    })
}
