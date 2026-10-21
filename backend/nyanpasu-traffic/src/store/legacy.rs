//! The schema-v3 `traffic.redb` written before nyanpasu-tsdb, imported once so users
//! upgrading keep their recorded statistics (design §14.3). Nothing outside the import
//! reads that file; remove this module once no supported release upgrades from a
//! version that wrote schema v3.

use super::{TrafficDb, decode, schema::*, storage};
use crate::{Bytes, GeoBasis, TrafficResult};
use nyanpasu_tsdb::{
    AuxOp, ErrorKind, EventRow, EventSeq, Input, InputError, RecoveryError, RecoveryView,
    SeriesPoint, Timestamp, Value, WriteError,
    storage::{Durability, Storage},
};
use redb::{ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    ops::{Bound, Range},
    path::Path,
};

const PROGRESS: &[u8] = b"legacy-v3";
const SCHEMA: TableDefinition<&str, u64> = TableDefinition::new("schema");
const SESSION: TableDefinition<&str, &[u8]> = TableDefinition::new("session");
const ACTIVE: TableDefinition<&str, &[u8]> = TableDefinition::new("active");
const CLOSED_TABLE: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("closed");
const TUPLES: TableDefinition<u64, &[u8]> = TableDefinition::new("tuples");
const MINUTES: TableDefinition<(u32, u64), (u64, u64, u64)> = TableDefinition::new("minute_usage");
const HOURS: TableDefinition<(u32, u64), (u64, u64, u64)> = TableDefinition::new("hour_usage");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Stage {
    #[default]
    Session,
    Active,
    Minute,
    Hour,
    Closed,
    Done,
}

/// The cursor is exclusive. It is committed with exactly the rows it acknowledges.
#[derive(Default, Serialize, Deserialize)]
pub(super) struct Progress {
    stage: Stage,
    after: Option<(u64, u64, String)>,
}

pub(super) fn restore<S: Storage>(view: &RecoveryView<'_, S>) -> Result<Progress, RecoveryError> {
    Ok(view
        .aux_get(PROGRESS, |v| v.map(decode::<Progress>).transpose())??
        .unwrap_or_default())
}

// Newtypes make serde borrow strings even inside Option and Vec. Escaped JSON strings
// still require ownership. Serialize has the same postcard layout as the C2 owned model.
#[derive(Serialize, Deserialize)]
struct Text<'a>(#[serde(borrow)] Cow<'a, str>);
fn unknown<'a>() -> Text<'a> {
    Text(Cow::Borrowed("unknown"))
}

#[derive(Serialize, Deserialize)]
struct Rule<'a> {
    #[serde(borrow)]
    kind: Text<'a>,
    #[serde(borrow)]
    payload: Text<'a>,
}
#[derive(Serialize, Deserialize)]
struct Dimensions<'a> {
    #[serde(borrow)]
    process: Text<'a>,
    #[serde(borrow)]
    source: Text<'a>,
    #[serde(borrow, default = "unknown")]
    inbound: Text<'a>,
    #[serde(borrow)]
    target: Text<'a>,
    #[serde(borrow)]
    protocol: Text<'a>,
    #[serde(borrow)]
    rule: Rule<'a>,
    #[serde(borrow)]
    chains: Vec<Text<'a>>,
    #[serde(borrow, default)]
    profile: Option<Text<'a>>,
    #[serde(borrow, default = "unknown")]
    source_region: Text<'a>,
    #[serde(borrow, default = "unknown")]
    destination_region: Text<'a>,
    #[serde(default)]
    destination_basis: Option<GeoBasis>,
}
#[derive(Serialize, Deserialize)]
struct Meta<'a> {
    last_sample_at: Option<i64>,
    #[serde(borrow)]
    instance_id: Option<Text<'a>>,
    global_counters: Option<Bytes>,
}
#[derive(Serialize, Deserialize)]
struct Active<'a> {
    #[serde(borrow)]
    id: Text<'a>,
    started_at: i64,
    first_seen_at: i64,
    counters: Bytes,
    #[serde(borrow)]
    dimensions: Dimensions<'a>,
    minutes: std::collections::BTreeMap<u32, Bytes>,
    hours: std::collections::BTreeMap<u32, Bytes>,
}
#[derive(Deserialize)]
struct Closed<'a> {
    #[serde(borrow)]
    id: Text<'a>,
    started_at: i64,
    first_seen_at: i64,
    closed_at: i64,
    bytes: Bytes,
    #[serde(borrow)]
    dimensions: Dimensions<'a>,
}

fn append(arena: &mut Vec<u8>, bytes: &[u8]) -> Range<usize> {
    let start = arena.len();
    arena.extend_from_slice(bytes);
    start..arena.len()
}
fn encode(arena: &mut Vec<u8>, value: &impl Serialize) -> TrafficResult<Range<usize>> {
    let start = arena.len();
    postcard::to_io(value, &mut *arena).map_err(storage)?;
    Ok(start..arena.len())
}
fn auxiliary(arena: &mut Vec<u8>, value: &impl Serialize) -> TrafficResult<Range<usize>> {
    let start = arena.len();
    arena.push(1);
    encode(arena, value)?;
    Ok(start..arena.len())
}

impl Dimensions<'_> {
    fn labels(&self, arena: &mut Vec<u8>) -> TrafficResult<[Range<usize>; LABELS]> {
        let mut text = |v: &Text<'_>| append(arena, v.0.as_bytes());
        let origin = text(if self.process.0 == "unknown" {
            &self.source
        } else {
            &self.process
        });
        let process = text(&self.process);
        let source = text(&self.source);
        let inbound = text(&self.inbound);
        let target = text(&self.target);
        let protocol = text(&self.protocol);
        let start = arena.len();
        arena.extend_from_slice(self.rule.kind.0.as_bytes());
        if !self.rule.payload.0.is_empty() {
            arena.push(b',');
            arena.extend_from_slice(self.rule.payload.0.as_bytes());
        }
        let rule = start..arena.len();
        let start = arena.len();
        for (i, group) in self.chains.iter().skip(1).rev().enumerate() {
            if i > 0 {
                arena.extend_from_slice(" → ".as_bytes());
            }
            arena.extend_from_slice(group.0.as_bytes());
        }
        let chain = start..arena.len();
        let exit = append(
            arena,
            self.chains.first().map_or(b"unknown", |s| s.0.as_bytes()),
        );
        let profile = encode(arena, &self.profile)?;
        let source_region = append(arena, self.source_region.0.as_bytes());
        let destination_region = append(arena, self.destination_region.0.as_bytes());
        let basis = append(
            arena,
            self.destination_basis.map_or("", |b| b.as_str()).as_bytes(),
        );
        let rule_key = encode(arena, &(&self.rule.kind, &self.rule.payload))?;
        let chains = encode(arena, &self.chains)?;
        Ok([
            origin,
            process,
            source,
            inbound,
            target,
            protocol,
            rule,
            chain,
            exit,
            profile,
            source_region,
            destination_region,
            basis,
            rule_key,
            chains,
        ])
    }
}

struct Row {
    /// Numeric and text parts of the legacy table key, for the exclusive checkpoint.
    key: (u64, u64, Range<usize>),
    value: RowValue,
}
enum RowValue {
    Aux {
        key: Range<usize>,
        value: Range<usize>,
    },
    Series {
        labels: [Range<usize>; LABELS],
        fields: [u64; 3],
    },
    Closed {
        labels: [Range<usize>; LABELS],
        seq: EventSeq,
        times: [i64; 3],
        bytes: Bytes,
    },
}

impl<S: Storage> TrafficDb<S> {
    /// Startup migration only. The caller drops and reopens this store afterwards so session
    /// restoration also observes surviving commits after an unknown import outcome. Errors
    /// authorize discarding the legacy file (design §14.3). Before recording after a failure,
    /// the host must reopen and durably abandon the import, even if legacy deletion fails.
    /// Opens only an existing legacy file, with the caller's cache budget; never creates a
    /// file or writes legacy tables (redb may settle its own journal). Deletes neither file.
    pub fn import_legacy(
        &mut self,
        path: &Path,
        batch_bytes: u64,
        cache_bytes: u64,
    ) -> TrafficResult<()> {
        self.import_legacy_batches(path, batch_bytes, cache_bytes, || true)
    }

    /// Makes a failed import terminal before live recording can overwrite its destination.
    /// The host reopens first: a failed import commit may have had an unknown outcome.
    pub fn abandon_legacy_import(&mut self) -> TrafficResult<()> {
        self.check()?;
        self.arena.clear();
        self.import_batch(&[], Stage::Done, Stage::Done, &mut || true)?;
        Ok(())
    }

    fn import_legacy_batches(
        &mut self,
        path: &Path,
        batch_bytes: u64,
        cache_bytes: u64,
        mut committed: impl FnMut() -> bool,
    ) -> TrafficResult<()> {
        if self.legacy.stage == Stage::Done {
            return Ok(());
        }
        let legacy = redb::Builder::new()
            .set_cache_size(cache_bytes as usize)
            .open(path)
            .map_err(storage)?;
        let txn = legacy.begin_read().map_err(storage)?;
        if txn
            .open_table(SCHEMA)
            .map_err(storage)?
            .get("version")
            .map_err(storage)?
            .map(|v| v.value())
            != Some(3)
        {
            return Err(storage("legacy traffic schema is not v3"));
        }
        let mut rows = Vec::new();
        while self.legacy.stage != Stage::Done {
            self.arena.clear();
            rows.clear();
            let stage = self.legacy.stage;
            // Reserve ample space for numeric fields and the progress record; write() remains
            // the authority and a local BatchSize refusal is split below, never other errors.
            let target = (batch_bytes / 2).max(1) as usize;
            let after = &self.legacy.after;
            match stage {
                Stage::Session => {
                    if let Some(value) = txn
                        .open_table(SESSION)
                        .map_err(storage)?
                        .get("meta")
                        .map_err(storage)?
                    {
                        let meta: Meta<'_> =
                            serde_json::from_slice(value.value()).map_err(storage)?;
                        let key = append(&mut self.arena, b"m");
                        let value = auxiliary(&mut self.arena, &meta)?;
                        rows.push(Row {
                            key: (0, 0, 0..0),
                            value: RowValue::Aux { key, value },
                        });
                    }
                }
                Stage::Active => {
                    // Like v3 load(), orphan active records without metadata are not live.
                    if txn
                        .open_table(SESSION)
                        .map_err(storage)?
                        .get("meta")
                        .map_err(storage)?
                        .is_some()
                    {
                        let table = txn.open_table(ACTIVE).map_err(storage)?;
                        let lower = after
                            .as_ref()
                            .map_or(Bound::Unbounded, |(_, _, id)| Bound::Excluded(id.as_str()));
                        for entry in table
                            .range::<&str>((lower, Bound::Unbounded))
                            .map_err(storage)?
                        {
                            let (id, value) = entry.map_err(storage)?;
                            let conn: Active<'_> =
                                serde_json::from_slice(value.value()).map_err(storage)?;
                            if conn.id.0 != id.value() {
                                return Err(storage("legacy active key disagrees with id"));
                            }
                            let start = self.arena.len();
                            self.arena.push(b'a');
                            self.arena.extend_from_slice(id.value().as_bytes());
                            let key = start..self.arena.len();
                            let id = start + 1..self.arena.len();
                            let value = auxiliary(&mut self.arena, &conn)?;
                            rows.push(Row {
                                key: (0, 0, id),
                                value: RowValue::Aux { key, value },
                            });
                            if self.arena.len() + rows.len() * 128 >= target {
                                break;
                            }
                        }
                    }
                }
                Stage::Minute | Stage::Hour => {
                    let table = txn
                        .open_table(if stage == Stage::Minute {
                            MINUTES
                        } else {
                            HOURS
                        })
                        .map_err(storage)?;
                    let tuples = txn.open_table(TUPLES).map_err(storage)?;
                    let lower = after.as_ref().map_or(Bound::Unbounded, |(b, t, _)| {
                        Bound::Excluded((*b as u32, *t))
                    });
                    for entry in table
                        .range::<(u32, u64)>((lower, Bound::Unbounded))
                        .map_err(storage)?
                    {
                        let (key, value) = entry.map_err(storage)?;
                        let (bucket, tuple) = key.value();
                        let value = value.value();
                        let json = tuples
                            .get(tuple)
                            .map_err(storage)?
                            .ok_or_else(|| storage("unknown legacy tuple"))?;
                        let dimensions: Dimensions<'_> =
                            serde_json::from_slice(json.value()).map_err(storage)?;
                        let labels = dimensions.labels(&mut self.arena)?;
                        rows.push(Row {
                            key: (u64::from(bucket), tuple, 0..0),
                            value: RowValue::Series {
                                labels,
                                fields: [value.0, value.1, value.2],
                            },
                        });
                        if self.arena.len() + rows.len() * 128 >= target {
                            break;
                        }
                    }
                }
                Stage::Closed => {
                    let table = txn.open_table(CLOSED_TABLE).map_err(storage)?;
                    let lower = after.as_ref().map_or(Bound::Unbounded, |(at, _, id)| {
                        Bound::Excluded((*at, id.as_str()))
                    });
                    for entry in table
                        .range::<(u64, &str)>((lower, Bound::Unbounded))
                        .map_err(storage)?
                    {
                        let (key, value) = entry.map_err(storage)?;
                        let conn: Closed<'_> =
                            serde_json::from_slice(value.value()).map_err(storage)?;
                        let (at, id) = key.value();
                        if conn.id.0 != id || u64::try_from(conn.closed_at).unwrap_or(0) != at {
                            return Err(storage("legacy closed key disagrees with connection"));
                        }
                        let id = append(&mut self.arena, id.as_bytes());
                        let labels = conn.dimensions.labels(&mut self.arena)?;
                        let seq = self.db.next_event_seq();
                        rows.push(Row {
                            key: (at, 0, id),
                            value: RowValue::Closed {
                                labels,
                                seq,
                                times: [conn.started_at, conn.first_seen_at, conn.closed_at],
                                bytes: conn.bytes,
                            },
                        });
                        if self.arena.len() + rows.len() * 128 >= target {
                            break;
                        }
                    }
                }
                Stage::Done => unreachable!(),
            }
            // Empty scans advance a phase durably. Session is a single-row phase.
            let next = if rows.is_empty() || stage == Stage::Session {
                match stage {
                    Stage::Session => Stage::Active,
                    Stage::Active => Stage::Minute,
                    Stage::Minute => Stage::Hour,
                    Stage::Hour => Stage::Closed,
                    Stage::Closed => Stage::Done,
                    Stage::Done => unreachable!(),
                }
            } else {
                stage
            };
            if !self.import_batch(&rows, stage, next, &mut committed)? {
                return Ok(());
            }
        }
        Ok(())
    }

    fn import_batch(
        &mut self,
        rows: &[Row],
        stage: Stage,
        next: Stage,
        committed: &mut impl FnMut() -> bool,
    ) -> TrafficResult<bool> {
        let after = if stage == next {
            rows.last().map(|r| {
                (
                    r.key.0,
                    r.key.1,
                    std::str::from_utf8(&self.arena[r.key.2.clone()]).expect("JSON string"),
                )
            })
        } else {
            None
        };
        // The one owned cursor per batch is the checkpoint, not a copy of each row.
        let progress = Progress {
            stage: next,
            after: after.map(|(b, t, id)| (b, t, id.to_owned())),
        };
        let progress_bytes = auxiliary(&mut self.arena, &progress)?;
        let arena = &self.arena;
        let labels: Vec<_> = rows
            .iter()
            .map(|r| match &r.value {
                RowValue::Series { labels, .. } | RowValue::Closed { labels, .. } => {
                    labels.each_ref().map(|r| &arena[r.clone()])
                }
                RowValue::Aux { .. } => [&b""[..]; LABELS],
            })
            .collect();
        let mut series = Vec::new();
        let mut values = Vec::new();
        let mut aux = Vec::new();
        for (row, labels) in rows.iter().zip(&labels) {
            match &row.value {
                RowValue::Series { fields, .. } => series.push(SeriesPoint {
                    tier: if stage == Stage::Minute { MINUTE } else { HOUR },
                    at: Timestamp(
                        row.key.0 as i64
                            * if stage == Stage::Minute {
                                MINUTE_MS
                            } else {
                                HOUR_MS
                            },
                    ),
                    labels,
                    fields,
                }),
                RowValue::Closed { times, bytes, .. } => values.push([
                    Value::I64(times[0]),
                    Value::I64(times[1]),
                    Value::U64(bytes.upload),
                    Value::U64(bytes.download),
                    Value::Series(labels),
                    Value::Bytes(&arena[row.key.2.clone()]),
                    Value::Null,
                ]),
                RowValue::Aux { key, value } => aux.push(AuxOp::Put {
                    key: &arena[key.clone()],
                    value: &arena[value.clone()],
                }),
            }
        }
        let events: Vec<_> = rows
            .iter()
            .filter_map(|r| {
                if let RowValue::Closed { seq, times, .. } = &r.value {
                    Some((*seq, times[2]))
                } else {
                    None
                }
            })
            .zip(&values)
            .map(|((seq, key), values)| EventRow {
                stream: CLOSED,
                key,
                seq,
                values,
            })
            .collect();
        aux.push(AuxOp::Put {
            key: PROGRESS,
            value: &arena[progress_bytes],
        });
        let result = self.db.write(
            &Input {
                batch_id: self.last_batch + 1,
                now: Timestamp(0),
                series: &series,
                events: &events,
                aux: &aux,
                pins: &[],
            },
            Durability::Durable,
        );
        match result {
            Ok(_) => {
                self.last_batch += 1;
                self.legacy = progress;
                Ok(committed())
            }
            Err(WriteError::InvalidInput {
                source: InputError::BatchSize { .. },
            }) if rows.len() > 1 => {
                let middle = rows.len() / 2;
                if !self.import_batch(&rows[..middle], stage, stage, committed)? {
                    return Ok(false);
                }
                self.import_batch(&rows[middle..], stage, next, committed)
            }
            Err(error) => {
                if error.kind() == ErrorKind::BackendFailed || !error.not_applied() {
                    self.recovery_needed.set(true);
                }
                Err(storage(error))
            }
        }
    }
}

#[cfg(test)]
mod tests;
