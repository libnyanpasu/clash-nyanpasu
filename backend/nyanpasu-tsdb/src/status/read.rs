//! Coherent metadata-only status from one captured storage/version pair.

use std::ops::ControlFlow;

use snafu::{OptionExt, ResultExt, ensure};

use super::*;
use crate::{
    Snapshot,
    budget::{MemoryAccount, MemoryPlan, Retain, Retained},
    catalog,
    head::journal::StoredValue,
    layout::{diagnostics, manifest, meta::read_state, references},
    storage::{Direction, Storage},
    time::{Grid, Span},
};

impl<S: Storage> crate::version::Shared<S> {
    /// Shares the facade and Reader capture path, including state-only unavailable observations.
    pub(crate) fn status(self: &std::sync::Arc<Self>) -> Result<Status, StatusError> {
        let state = self.registry.state();
        if state != LifecycleState::Open {
            return Ok(Status::unavailable(state, self.query_budget.reserved()));
        }
        let snapshot = match self.capture() {
            Ok(snapshot) => snapshot,
            Err(source) => {
                let state = self.registry.state();
                if state != LifecycleState::Open
                    && matches!(
                        source,
                        CaptureError::CheckOpen { .. } | CaptureError::RecoveryNeeded
                    )
                {
                    return Ok(Status::unavailable(state, self.query_budget.reserved()));
                }
                return Err(StatusError::Capture { source });
            }
        };
        let mut status = snapshot.status()?;
        drop(snapshot);
        status.query_reserved_bytes = self.query_budget.reserved();
        Ok(status)
    }
}

impl<S: Storage> Snapshot<S> {
    /// Observes this capture's publication, retaining only schema-sized arrays and up to 64
    /// ring records, charged to the query budget until drop. Scans each stream's manifest once,
    /// its head rows and persistent work metadata; decodes no chunk payload and runs no work.
    /// Takes no clock or policy. Outside Open returns lifecycle only, even for an old snapshot.
    pub fn status(&self) -> Result<Status, StatusError> {
        let state = self.lifecycle();
        if state != LifecycleState::Open {
            return Ok(Status::unavailable(state, self.budget().reserved()));
        }
        let mut result = read(self);
        if let Err(error) = &result
            && error.kind() == ErrorKind::BackendFailed
        {
            self.report(error.kind());
            return result;
        }
        let state = self.lifecycle();
        if state != LifecycleState::Open {
            drop(result);
            return Ok(Status::unavailable(state, self.budget().reserved()));
        }
        if let Ok(status) = &mut result {
            status.query_reserved_bytes = self.budget().reserved();
        }
        result
    }
}

/// Adds exact metadata counts; overflow is damaged metadata, never a wrapping diagnostic.
fn add(to: &mut u64, value: u64) -> Result<(), StatusError> {
    *to = to.checked_add(value).context(InvalidSnafu)?;
    Ok(())
}

/// Expands an actual key range, never an extent boundary or a retention-policy estimate.
fn bounds(stream: &mut StreamStatus, first: i64, last: i64) -> Result<(), StatusError> {
    ensure!(first <= last, InvalidSnafu);
    stream.oldest = Some(stream.oldest.map_or(first, |old| old.min(first)));
    stream.newest = Some(stream.newest.map_or(last, |old| old.max(last)));
    Ok(())
}

/// Constructs charged output and releases the capture-independent workspace before returning.
fn read<S: Storage>(snapshot: &Snapshot<S>) -> Result<Status, StatusError> {
    snapshot
        .revoked()
        .check("status::read:1")
        .context(StorageSnafu)?;
    let view = snapshot.storage();
    let schema = snapshot.schema();
    let version = snapshot.version();
    let state = read_state(view).context(StateSnafu)?;
    ensure!(state.publication == version.publication, InvalidSnafu);
    let account = MemoryAccount::new(snapshot.budget(), "status", MemoryPlan::default())
        .context(ReserveSnafu)?;
    let mut streams = account
        .buffer(diagnostics::streams(schema).count())
        .context(ReserveSnafu)?;
    for stream in diagnostics::streams(schema) {
        streams.push(read_stream(snapshot, &account, stream)?);
    }
    let global = diagnostics::global_totals(view).context(DiagnosticsSnafu)?;
    let mut losses = account
        .buffer(global.next_seq.min(diagnostics::RING_LEN) as usize)
        .context(ReserveSnafu)?;
    diagnostics::losses(view, |entry| losses.push(entry)).context(DiagnosticsSnafu)?;
    let (combinations, label_values, folded_combinations) = version.catalog.status_counts();
    let (pinned_combinations, pin_keys) =
        catalog::pin_totals(view, |id| version.catalog.combo(id).is_some()).context(PinsSnafu)?;
    let mut candidates = 0u64;
    references::scan_candidates(view, |_| {
        candidates += 1;
        ControlFlow::<()>::Continue(())
    })
    .context(ReferencesSnafu)?;
    snapshot
        .revoked()
        .check("status::read:2")
        .context(StorageSnafu)?;
    let mut retained = Retained::new(&account);
    let streams = streams.retain(&mut retained).context(ReserveSnafu)?;
    let losses = losses.retain(&mut retained).context(ReserveSnafu)?;
    let data = retained.finish(StatusData {
        incarnation: state.incarnation,
        publication: state.publication,
        last_batch: state.last_batch,
        journal_bytes: state.journal_bytes,
        catalog: CatalogStatus {
            bytes: version.catalog.bytes(),
            combinations,
            label_values,
            folded_combinations,
            has_overflow_combination: schema.series.is_some(),
            pinned_combinations,
            pin_keys,
            recycled_combinations: global.recycled_combos,
            recycled_values: global.recycled_values,
        },
        maintenance: MaintenanceStatus {
            behind: snapshot.status_behind,
            event_consolidation_gated_off: snapshot.event_consolidation_gated_off(),
            recycle_candidates: candidates,
        },
        streams,
        losses,
    });
    Ok(Status {
        lifecycle: LifecycleState::Open,
        query_reserved_bytes: snapshot.budget().reserved(),
        data: Some(data),
    })
}

/// Sums unexpired chunks and head rows, reporting expired chunks separately for cleanup.
fn read_stream<S: Storage>(
    snapshot: &Snapshot<S>,
    account: &MemoryAccount,
    stream: StreamRef,
) -> Result<StreamStatus, StatusError> {
    let view = snapshot.storage();
    let stored = manifest::read_stream(view, stream).context(ManifestSnafu)?;
    let visible = |extent| stored.expired.is_none_or(|expired| extent > expired);
    let mut result = StreamStatus {
        stream,
        rows: 0,
        bytes: 0,
        oldest: None,
        newest: None,
        expiration_watermark: stored.expired,
        downsampling_watermark: None,
        cumulative: diagnostics::counters(view, stream).context(DiagnosticsSnafu)?,
        pending: StreamWork::default(),
    };
    let bad = manifest::scan_chunks(
        view,
        stream,
        i64::MIN..=i64::MAX,
        Direction::Forward,
        |listed| {
            let mut examined = || -> Result<(), StatusError> {
                snapshot
                    .revoked()
                    .check("status::manifest:1")
                    .context(StorageSnafu)?;
                let entry = listed.entry;
                ensure!(
                    (1..=3).contains(&entry.kind)
                        && entry.rows > 0
                        && entry.key_range.0 <= entry.key_range.1,
                    InvalidSnafu
                );
                if visible(listed.extent) {
                    add(&mut result.rows, u64::from(entry.rows))?;
                    add(&mut result.bytes, entry.bytes)?;
                    bounds(&mut result, entry.key_range.0, entry.key_range.1)?;
                    if entry.kind <= 2 {
                        add(&mut result.pending.l0_chunks, 1)?;
                    }
                } else {
                    add(&mut result.pending.cleanup_chunks, 1)?;
                    add(&mut result.pending.cleanup_bytes, entry.bytes)?;
                }
                Ok(())
            };
            match examined() {
                Ok(()) => ControlFlow::Continue(()),
                Err(error) => ControlFlow::Break(error),
            }
        },
    )
    .context(ManifestSnafu)?;
    if let Some(error) = bad {
        return Err(error);
    }
    match stream {
        StreamRef::Tier(tier) => {
            let spec = &snapshot
                .schema()
                .series
                .as_ref()
                .expect("a tier has a series")
                .tiers[usize::from(tier.0)];
            let grid = Grid::new(
                Span::from_duration(spec.bucket).expect("validated bucket"),
                Span::from_duration(spec.extent).expect("validated extent"),
            )
            .expect("validated grid");
            for (source, accumulator) in snapshot.version().head.series() {
                if source.tier == tier && visible(grid.extent_of_bucket(source.bucket)) {
                    snapshot.check_open().context(StorageSnafu)?;
                    add(&mut result.rows, accumulator.len() as u64)?;
                    add(&mut result.bytes, accumulator.journal_bytes())?;
                    if accumulator.len() > 0 {
                        bounds(&mut result, source.bucket, source.bucket)?;
                    }
                }
            }
        }
        StreamRef::Events(id) => {
            let columns = snapshot.schema().events[usize::from(id.0)].columns.len();
            // One bounded decoded head row; its value bytes borrow the already charged head.
            let _row = account
                .lease(MemoryPlan::array::<StoredValue<'_>>(columns).context(ReserveSnafu)?)
                .context(ReserveSnafu)?;
            for segment in snapshot.version().head.events(id).segments() {
                if !visible(segment.extent()) {
                    continue;
                }
                add(&mut result.rows, segment.rows())?;
                add(&mut result.bytes, segment.journal_bytes())?;
                for (index, row) in segment.record().rows().enumerate() {
                    if index.is_multiple_of(1024) {
                        snapshot.check_open().context(StorageSnafu)?;
                    }
                    bounds(&mut result, row.key, row.key)?;
                }
            }
        }
    }
    ensure!(result.bytes == stored.bytes, InvalidSnafu);
    result.pending.crowding_marks =
        manifest::crowding_count(view, stream).context(ManifestSnafu)?;
    if matches!(stream, StreamRef::Events(_)) {
        result.pending.runs = manifest::run_count(view, stream).context(ManifestSnafu)?;
    }
    Ok(result)
}
