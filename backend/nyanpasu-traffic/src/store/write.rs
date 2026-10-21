use super::{FlushError, TrafficDb, schema::*, storage};
use crate::{Dimension, Dimensions, FlushBatch};
use nyanpasu_tsdb::{
    AuxOp, ErrorKind, EventRow, Input, Maintain, Retention, RetentionPolicy, SeriesPoint,
    Timestamp, Value, storage::Storage,
};
use serde::Serialize;
use std::{collections::HashMap, ops::Range, time::Duration};

struct Encoded<'a> {
    dimensions: &'a Dimensions,
    profile: Range<usize>,
    rule_key: Range<usize>,
    chains: Range<usize>,
    rule: Option<Range<usize>>,
    chain: Option<Range<usize>>,
}

fn encode(arena: &mut Vec<u8>, value: &impl Serialize) -> Result<Range<usize>, postcard::Error> {
    let start = arena.len();
    postcard::to_io(value, &mut *arena)?;
    Ok(start..arena.len())
}

impl<'a> Encoded<'a> {
    fn new(d: &'a Dimensions, arena: &mut Vec<u8>) -> Result<Self, postcard::Error> {
        let profile = encode(arena, &d.profile.as_deref())?;
        let rule_key = encode(arena, &(&*d.rule.kind, &*d.rule.payload))?;
        let chains = encode(arena, &d.chains)?;
        let mut display = |dimension| {
            let start = arena.len();
            // Stream formatting into the reusable arena, without a temporary String.
            for byte in crate::key::Key::of(d, dimension).bytes() {
                arena.push(byte);
            }
            start..arena.len()
        };
        let rule = (!d.rule.payload.is_empty()).then(|| display(Dimension::Rule));
        let chain = (d.chains.len() > 2).then(|| display(Dimension::Chain));
        Ok(Self {
            dimensions: d,
            profile,
            rule_key,
            chains,
            rule,
            chain,
        })
    }
    fn labels<'b>(&'b self, arena: &'b [u8]) -> [&'b [u8]; LABELS]
    where
        'a: 'b,
    {
        let d = self.dimensions;
        [
            d.origin().as_bytes(),
            d.process.as_bytes(),
            d.source.as_bytes(),
            d.inbound.as_bytes(),
            d.target.as_bytes(),
            d.protocol.as_bytes(),
            self.rule
                .as_ref()
                .map_or(d.rule.kind.as_bytes(), |r| &arena[r.clone()]),
            self.chain.as_ref().map_or_else(
                || d.chains.get(1).map_or(&b""[..], |s| s.as_bytes()),
                |r| &arena[r.clone()],
            ),
            d.exit().as_bytes(),
            &arena[self.profile.clone()],
            d.source_region.as_bytes(),
            d.destination_region.as_bytes(),
            d.destination_basis.map_or("", |b| b.as_str()).as_bytes(),
            &arena[self.rule_key.clone()],
            &arena[self.chains.clone()],
        ]
    }
}

#[derive(Clone, Copy)]
enum Metadata {
    First,
    Omit,
}

pub(super) fn flush<S: Storage>(
    store: &mut TrafficDb<S>,
    batch: &FlushBatch,
    now_ms: i64,
    retention: Option<Duration>,
) -> Result<(), FlushError> {
    // Transactions commit in this order: the deletions of closed connections' records, the
    // session metadata, then active records. A crash between any two restores metadata that
    // matches every surviving record: old records under the old instance, which the next
    // frame closes, or new ones under the new instance, one flush older at worst.
    // On the normal path all closes fit atomically with the metadata.
    match transactions(store, batch, now_ms, Metadata::First) {
        Err(FlushError::Oversized { bytes, limit })
            if !batch.closed.is_empty() && batch.closed_buckets.len() == batch.closed.len() =>
        {
            tracing::error!(
                bytes,
                limit,
                "traffic close batch exceeds the limit; committing atomic close groups"
            );
            close_groups(store, batch, 0..batch.closed.len(), now_ms)?;
            let rest = FlushBatch {
                meta: batch.meta.clone(),
                active: batch.active.clone(),
                removed: vec![],
                minutes: vec![],
                hours: vec![],
                closed: vec![],
                closed_buckets: vec![],
            };
            transactions(store, &rest, now_ms, Metadata::First)
                .map_err(|error| progress(error, batch.closed.len()))?;
        }
        result => result?,
    }
    maintain(store, now_ms, retention)
}

// Each close keeps its own buckets until commit. This exceptional path splits only
// between connections: an event, its series contributions and its aux deletion stay
// atomic, including after id reuse. Metadata and active records follow every group.
fn close_groups<S: Storage>(
    store: &mut TrafficDb<S>,
    batch: &FlushBatch,
    range: Range<usize>,
    now_ms: i64,
) -> Result<(), FlushError> {
    let mut part = batch.closed_part(range.clone());
    match transactions(store, &part, now_ms, Metadata::Omit) {
        Err(FlushError::Oversized { .. }) if range.len() > 1 => {
            let middle = range.start + range.len() / 2;
            close_groups(store, batch, range.start..middle, now_ms)?;
            close_groups(store, batch, middle..range.end, now_ms)
        }
        Err(FlushError::Oversized { bytes, limit }) => {
            // Its usage and event cannot be stored, but its record must still go: a
            // surviving record would bring the closed connection back on restore.
            tracing::error!(bytes, limit, id = %batch.closed[range.start].id,
                "dropping the usage of an unrepresentable traffic close record");
            part.minutes.clear();
            part.hours.clear();
            part.closed.clear();
            part.closed_buckets.clear();
            transactions(store, &part, now_ms, Metadata::Omit)
                .map_err(|error| progress(error, range.start))
        }
        result => result.map_err(|error| progress(error, range.start)),
    }
}

fn progress(error: FlushError, closed: usize) -> FlushError {
    match error {
        FlushError::NotApplied(error) if closed > 0 => FlushError::Partial {
            error,
            closed,
            active: 0,
        },
        FlushError::Partial {
            error,
            closed: count,
            active,
        } => FlushError::Partial {
            error,
            closed: closed + count,
            active,
        },
        error => error,
    }
}

fn transactions<S: Storage>(
    store: &mut TrafficDb<S>,
    batch: &FlushBatch,
    now_ms: i64,
    metadata: Metadata,
) -> Result<(), FlushError> {
    let local = |error| FlushError::NotApplied(storage(error));
    let arena = &mut store.arena;
    arena.clear();
    let count = batch.minutes.len() + batch.hours.len() + batch.closed.len();
    let mut known = HashMap::with_capacity(count);
    let mut encoded = Vec::with_capacity(count);
    for d in batch
        .minutes
        .iter()
        .chain(&batch.hours)
        .map(|(_, d, _)| &**d)
        .chain(batch.closed.iter().map(|c| &*c.dimensions))
    {
        if !known.contains_key(d) {
            let i = encoded.len();
            encoded.push(Encoded::new(d, arena).map_err(local)?);
            known.insert(d, i);
        }
    }
    let mut removed = Vec::with_capacity(batch.removed.len());
    for id in &batch.removed {
        let start = arena.len();
        arena.push(b'a');
        arena.extend_from_slice(id.as_bytes());
        removed.push(start..arena.len());
    }
    let mut puts = Vec::with_capacity(batch.active.len() + 1);
    if matches!(metadata, Metadata::First) {
        let key = arena.len();
        arena.push(b'm');
        let key = key..arena.len();
        let start = arena.len();
        arena.push(1);
        encode(arena, &batch.meta).map_err(local)?;
        puts.push((key, start..arena.len()));
    }
    for conn in &batch.active {
        let start = arena.len();
        arena.push(b'a');
        arena.extend_from_slice(conn.id.as_bytes());
        let key = start..arena.len();
        let start = arena.len();
        arena.push(1);
        encode(arena, conn).map_err(local)?;
        puts.push((key, start..arena.len()));
    }
    // Phase two: arena growth is finished before any label or auxiliary slice is borrowed.
    let labels: Vec<_> = encoded.iter().map(|e| e.labels(arena)).collect();
    let fields: Vec<_> = batch
        .minutes
        .iter()
        .chain(&batch.hours)
        .map(|(_, _, u)| [u.bytes.upload, u.bytes.download, u.connections])
        .collect();
    let mut series = Vec::with_capacity(fields.len());
    for (i, (bucket, d, _)) in batch.minutes.iter().chain(&batch.hours).enumerate() {
        let (tier, width) = if i < batch.minutes.len() {
            (MINUTE, MINUTE_MS)
        } else {
            (HOUR, HOUR_MS)
        };
        series.push(SeriesPoint {
            tier,
            at: Timestamp(i64::from(*bucket) * width),
            labels: &labels[known[&**d]],
            fields: &fields[i],
        });
    }
    let values: Vec<_> = batch
        .closed
        .iter()
        .map(|c| {
            [
                Value::I64(c.started_at),
                Value::I64(c.first_seen_at),
                Value::U64(c.bytes.upload),
                Value::U64(c.bytes.download),
                Value::Series(&labels[known[&*c.dimensions]]),
                Value::Bytes(c.id.as_bytes()),
                Value::Null,
            ]
        })
        .collect();
    let events: Vec<_> = batch
        .closed
        .iter()
        .zip(&values)
        .map(|(c, values)| EventRow {
            stream: CLOSED,
            key: c.closed_at,
            seq: c.seq,
            values,
        })
        .collect();
    let aux: Vec<_> = removed
        .iter()
        .map(|key| AuxOp::Delete {
            key: &arena[key.clone()],
        })
        .chain(puts.iter().map(|(key, value)| AuxOp::Put {
            key: &arena[key.clone()],
            value: &arena[value.clone()],
        }))
        .collect();
    let now = Timestamp(now_ms);
    let fixed = removed.len() + usize::from(matches!(metadata, Metadata::First));
    let mut input = Input {
        batch_id: store.last_batch + 1,
        now,
        series: &series,
        events: &events,
        aux: &aux[..fixed],
        pins: &[],
    };
    let mut bytes = input.bytes();
    let limit = store.batch_bytes;
    if bytes > limit {
        return Err(FlushError::Oversized { bytes, limit });
    }
    let mut first = true;
    let mut start = 0;
    loop {
        let mut end = if first { fixed } else { start };
        while let Some(AuxOp::Put { key, value }) = aux.get(end) {
            let size = (key.len() + value.len()) as u64;
            if bytes + size > limit {
                break;
            }
            bytes += size;
            end += 1;
        }
        if !first && end == start {
            // Everything after the fixed part is an active record; one that cannot fit even
            // alone is skipped, and the records after it still commit.
            let AuxOp::Put { key, value } = aux[start] else {
                unreachable!()
            };
            tracing::error!(bytes = key.len() + value.len(), limit,
                id = %batch.active[start - fixed].id,
                "skipping unrepresentable active traffic record");
            start += 1;
            if start == aux.len() {
                return Ok(());
            }
            continue;
        }
        input.aux = &aux[start..end];
        input.batch_id = store.last_batch + 1;
        if let Err(error) = store.db.write(&input, store.db.durability_due(now)) {
            return if error.kind() == ErrorKind::BackendFailed || !error.not_applied() {
                store.recovery_needed.set(true);
                Err(FlushError::Recovery(storage(error)))
            } else if first {
                Err(FlushError::NotApplied(storage(error)))
            } else {
                Err(FlushError::Partial {
                    error: storage(error),
                    closed: batch.closed.len(),
                    active: start - fixed,
                })
            };
        }
        store.last_batch += 1;
        if end == aux.len() {
            return Ok(());
        }
        // Active aux records contain their counters AND their buckets; none of their usage
        // is in the series yet. A crash between transactions leaves some records one flush
        // older: the next sample computes its delta from those counters, never counting twice.
        first = false;
        start = end;
        bytes = 0;
        input.series = &[];
        input.events = &[];
    }
}

fn maintain<S: Storage>(
    store: &mut TrafficDb<S>,
    now_ms: i64,
    retention: Option<Duration>,
) -> Result<(), FlushError> {
    let now = Timestamp(now_ms);
    let retention = RetentionPolicy {
        tiers: vec![
            Retention {
                max_age: Some(Duration::from_secs(6 * 3600)),
                max_bytes: None,
            },
            Retention {
                max_age: retention,
                max_bytes: None,
            },
        ],
        streams: vec![Retention {
            max_age: retention,
            max_bytes: None,
        }],
    };
    if let Err(error) = store.db.maintain(&Maintain {
        now,
        retention: &retention,
        rounds: 16,
    }) {
        return if error.kind() == ErrorKind::BackendFailed {
            store.recovery_needed.set(true);
            Err(FlushError::Recovery(storage(error)))
        } else {
            Err(FlushError::Applied(storage(error)))
        };
    }
    Ok(())
}
