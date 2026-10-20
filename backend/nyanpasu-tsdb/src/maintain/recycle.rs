//! Recycling of combination and label ids, once no chunk, journal record or pin refers to
//! them (§5.3).
//!
//! The write path records, per stream, the newest extent whose data references each
//! combination and label value (`layout::references`), and each label value counts the live
//! combinations that use it (`catalog`). Retention deletes a stream's extents oldest first, so
//! a stream is done with an entity once its newest extent there is cleaned up. A round does
//! two things, within one budget of D (`cleanup_chunks`) index entries and candidates
//! examined, stale ones included:
//!
//! 1. **Forgetting.** For each stream that references ids, and whose cleanup is done, the
//!    index entries of the extents up to its watermark, the oldest first: each goes with its
//!    record, and an entity left with no record becomes a candidate. A stream is cleaned up to
//!    its watermark once no chunk lies there; until then it has no cleaned extent, and a
//!    stream without a watermark has none either. The planner runs cleanup first, so the check
//!    only matters to a commit's round (`plan_backlog`), which has no policy to clean up with.
//! 2. **Releasing.** Each candidate is checked again: a combination has to be live, without a
//!    record and without pins; a label value live, without uses and without a record. Then
//!    the round's catalog draft releases it, one at a time; a stale candidate just goes. A
//!    released combination takes its use off its label values, and those left without uses
//!    become candidates, which this round or a later one releases.
//!
//! The catalog keeps a reserve that lets a fresh draft release one id whatever its budget, so
//! the first release of a round always fits. A release the draft refuses
//! (`ReleaseError::ReserveRelease`) ends the round: it commits what it did, and the refused
//! candidate waits for the next round. So every round makes progress.
//!
//! A label value freed by a released combination is released in its own step, possibly in a
//! later transaction, where §5.3 rule 4 releases it in the combination's: nothing references
//! it by then, and each release stays within the reserve (DESIGN.md).

use std::ops::ControlFlow;

use snafu::{ResultExt, Snafu, ensure};

use super::{Changed, Context, Maintained, Round};
use crate::{
    ColumnType, ErrorKind, LabelId, Schema, StreamId, TierId,
    budget::{BudgetExceeded, MemoryAccount, MemoryPlan},
    catalog::{CatalogDraft, PinError, ReleaseError, UsesError, is_pinned, label_uses},
    layout::{
        manifest::{self, ManifestError, StreamRef},
        references::{self, Entity, ReferenceError},
    },
    storage::{Direction, StorageRead, StorageWrite},
};

/// The index entries or candidates a round lists, then works through, at once: it holds this
/// many however many it examines.
const WINDOW: usize = 16;

/// The streams whose data references ids: every tier, and each event stream with a `Series`
/// or a `Label` column.
fn streams(schema: &Schema) -> impl Iterator<Item = StreamRef> + '_ {
    let tiers = schema.series.iter().flat_map(|series| {
        (0..series.tiers.len())
            .map(|tier| StreamRef::Tier(TierId(u8::try_from(tier).expect("tier ids are u8"))))
    });
    let events = schema
        .events
        .iter()
        .enumerate()
        .filter(|(_, spec)| {
            spec.columns
                .iter()
                .any(|column| matches!(column.ty, ColumnType::Series | ColumnType::Label(_)))
        })
        .map(|(stream, _)| {
            StreamRef::Events(StreamId(u8::try_from(stream).expect("stream ids are u8")))
        });
    tiers.chain(events)
}

/// The last extent of `stream` that is cleaned up: its watermark, once no chunk lies at or
/// below it. `None` while cleanup is due, or for a stream that expired nothing.
///
/// Below the watermark, what consolidation keeps beside the chunks may outlive them: cleanup
/// deletes an event stream's run index entries after the chunks (`retention`). That
/// bookkeeping, runs and marks alike, holds no ids, so no chunk at or below the watermark is
/// enough.
fn cleaned(read: &impl StorageRead, stream: StreamRef) -> Result<Option<i64>, RecycleError> {
    let Some(expired) =
        manifest::read_watermark(read, stream).context(ReadWatermarkSnafu { stream })?
    else {
        return Ok(None);
    };
    let uncleaned =
        manifest::scan_chunks(read, stream, i64::MIN..=expired, Direction::Forward, |_| {
            ControlFlow::Break(())
        })
        .context(FindUncleanedSnafu { stream })?;
    Ok(uncleaned.is_none().then_some(expired))
}

/// A recycling round, when a candidate waits or a cleaned extent keeps a reference. It reads
/// one candidate, then per stream its record, its first chunk up to the watermark and its
/// first index entry there, however much the stream holds.
pub(super) fn plan(
    read: &impl StorageRead,
    context: &Context<'_>,
) -> Result<Option<Round>, RecycleError> {
    if references::has_candidates(read).context(FindCandidatesSnafu)? {
        return Ok(Some(Round::Recycle));
    }
    for stream in streams(context.schema) {
        let Some(cleaned) = cleaned(read, stream)? else {
            continue;
        };
        let found = references::scan_up_to(read, stream, cleaned, |_, _| ControlFlow::Break(()))
            .context(FindForgottenSnafu { stream })?;
        if found.is_some() {
            return Ok(Some(Round::Recycle));
        }
    }
    Ok(None)
}

/// Forgets the references of the cleaned extents, then releases the candidates, at most
/// `cleanup_chunks` entries and candidates in all, and publishes the catalog it released them
/// from.
pub(super) fn recycle(
    batch: &mut impl StorageWrite,
    context: &Context<'_>,
) -> Result<Changed, RecycleError> {
    let limit = context.profile.cleanup_chunks;
    let mut done = Maintained {
        recycles: 1,
        ..Maintained::default()
    };
    let mut examined = 0;
    for stream in streams(context.schema) {
        if examined == limit {
            break;
        }
        if let Some(cleaned) = cleaned(&*batch, stream)? {
            examined += forget_cleaned(batch, stream, cleaned, limit - examined)?;
        }
    }
    done.references_forgotten = examined as u64;
    let current = &context.current.catalog;
    let copies = MemoryAccount::new(context.staging_budget, "staging", MemoryPlan::default())
        .context(ReserveStagingSnafu)?;
    let mut draft = current
        .draft(context.profile.catalog, &copies)
        .context(ReserveStagingSnafu)?;
    examined += release_candidates(
        batch,
        context.schema,
        &mut draft,
        limit - examined,
        &mut done,
    )?;
    // The planner found an entry or a candidate in the state the round starts from.
    ensure!(examined > 0, NothingToRecycleSnafu);
    let (catalog, staging) = draft.finish();
    Ok(Changed {
        head: context.current.head.clone(),
        catalog: Some(catalog),
        staging,
        journal_removed: 0,
        next_chunk: None,
        done,
    })
}

/// Deletes at most `limit` references of `stream` whose newest extent is at most `cleaned`,
/// the oldest first, and makes each entity left with no reference a candidate. Returns how
/// many it deleted.
fn forget_cleaned(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    cleaned: i64,
    limit: usize,
) -> Result<usize, RecycleError> {
    let mut forgotten = 0;
    while forgotten < limit {
        let wanted = WINDOW.min(limit - forgotten);
        let mut window = [None::<(i64, Entity)>; WINDOW];
        let mut taken = 0;
        references::scan_up_to(&*batch, stream, cleaned, |extent, entity| {
            window[taken] = Some((extent, entity));
            taken += 1;
            if taken == wanted {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .context(FindForgottenSnafu { stream })?;
        for &(extent, entity) in window[..taken].iter().flatten() {
            references::forget(batch, entity, stream, extent).context(ForgetSnafu { stream })?;
            references::queue_unreferenced(batch, entity).context(QueueForgottenSnafu)?;
        }
        forgotten += taken;
        if taken < wanted {
            break;
        }
    }
    Ok(forgotten)
}

/// What checking a candidate came to.
enum Checked {
    Released,
    /// Something references it, or it is gone already.
    Stale,
    /// The draft has no room left to release it.
    Refused,
}

/// Checks at most `limit` candidates, in key order, releasing those that are free, until the
/// draft refuses one, which stays a candidate. Returns how many it checked.
fn release_candidates(
    batch: &mut impl StorageWrite,
    schema: &Schema,
    draft: &mut CatalogDraft,
    limit: usize,
    done: &mut Maintained,
) -> Result<usize, RecycleError> {
    let mut checked = 0;
    'windows: while checked < limit {
        let wanted = WINDOW.min(limit - checked);
        let mut window = [None::<Entity>; WINDOW];
        let mut taken = 0;
        references::scan_candidates(&*batch, |entity| {
            window[taken] = Some(entity);
            taken += 1;
            if taken == wanted {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .context(FindCandidatesSnafu)?;
        for &entity in window[..taken].iter().flatten() {
            checked += 1;
            match check(batch, schema, draft, entity, done)? {
                Checked::Released | Checked::Stale => {
                    references::dequeue(batch, entity).context(DequeueSnafu)?;
                }
                Checked::Refused => break 'windows,
            }
        }
        // A window's releases may queue values the scan did not see: only an empty window
        // ends the candidates.
        if taken == 0 {
            break;
        }
    }
    done.candidates_checked = checked as u64;
    Ok(checked)
}

/// Checks candidate `entity` against the state `batch` holds, and releases it if it is free.
fn check(
    batch: &mut impl StorageWrite,
    schema: &Schema,
    draft: &mut CatalogDraft,
    entity: Entity,
    done: &mut Maintained,
) -> Result<Checked, RecycleError> {
    // The reserve fits the first release of a fresh draft, so only a later one is refused.
    let first = done.combos_released + done.labels_released == 0;
    match entity {
        Entity::Combo(id) => {
            if draft.combo(id).is_none()
                || references::is_referenced(&*batch, entity).context(CheckReferencesSnafu)?
                || is_pinned(&*batch, id).context(CheckPinsSnafu { id })?
            {
                return Ok(Checked::Stale);
            }
            let unused = match draft.release_combo(batch, id) {
                Ok(unused) => unused,
                Err(ReleaseError::ReserveRelease { .. } | ReleaseError::StageRelease { .. })
                    if !first =>
                {
                    return Ok(Checked::Refused);
                }
                Err(source) => return Err(source).context(ReleaseComboSnafu { id }),
            };
            for (dimension, label) in unused {
                references::queue(batch, Entity::Label(dimension, label))
                    .context(QueueUnusedSnafu)?;
            }
            done.combos_released += 1;
        }
        Entity::Label(dimension, id) => {
            ensure!(
                usize::from(dimension.0) < schema.labels.len(),
                UnknownDimensionSnafu { dimension }
            );
            if draft.label(dimension, id).is_none()
                || label_uses(&*batch, dimension, id).context(CheckUsesSnafu { dimension, id })? > 0
                || references::is_referenced(&*batch, entity).context(CheckReferencesSnafu)?
            {
                return Ok(Checked::Stale);
            }
            match draft.release_label(batch, dimension, id) {
                Ok(()) => {}
                Err(ReleaseError::ReserveRelease { .. } | ReleaseError::StageRelease { .. })
                    if !first =>
                {
                    return Ok(Checked::Refused);
                }
                Err(source) => return Err(source).context(ReleaseLabelSnafu { dimension, id }),
            }
            done.labels_released += 1;
        }
    }
    Ok(Checked::Released)
}

/// A recycling round, or planning one, failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum RecycleError {
    #[snafu(display("failed to read the watermark of {stream:?}"))]
    ReadWatermark {
        stream: StreamRef,
        source: ManifestError,
    },

    #[snafu(display("failed to look for chunks cleanup left in {stream:?}"))]
    FindUncleaned {
        stream: StreamRef,
        source: ManifestError,
    },

    #[snafu(display("failed to list the references of the cleaned extents of {stream:?}"))]
    FindForgotten {
        stream: StreamRef,
        source: ReferenceError,
    },

    #[snafu(display("failed to delete a reference of a cleaned extent of {stream:?}"))]
    Forget {
        stream: StreamRef,
        source: ReferenceError,
    },

    #[snafu(display("failed to make an entity without references a candidate"))]
    QueueForgotten { source: ReferenceError },

    #[snafu(display("failed to list the candidates for release"))]
    FindCandidates { source: ReferenceError },

    #[snafu(display("failed to check the references of a candidate"))]
    CheckReferences { source: ReferenceError },

    #[snafu(display("failed to check the pins of combination {id}"))]
    CheckPins { id: u32, source: PinError },

    #[snafu(display("failed to check the uses of label value {id} of {dimension:?}"))]
    CheckUses {
        dimension: LabelId,
        id: u32,
        source: UsesError,
    },

    /// A candidate names a label dimension the schema does not have.
    #[snafu(display("a candidate names {dimension:?}, which the schema does not have"))]
    UnknownDimension { dimension: LabelId },

    #[snafu(display("failed to release combination {id}"))]
    ReleaseCombo { id: u32, source: ReleaseError },

    #[snafu(display("failed to release label value {id} of {dimension:?}"))]
    ReleaseLabel {
        dimension: LabelId,
        id: u32,
        source: ReleaseError,
    },

    #[snafu(display("failed to make the label values a released combination used candidates"))]
    QueueUnused { source: ReferenceError },

    #[snafu(display("failed to take a checked candidate off the candidates"))]
    Dequeue { source: ReferenceError },

    /// The staging budget has no room for the catalog pages the round copied (§7.4).
    #[snafu(display("the staging budget has no room for the round's catalog"))]
    ReserveStaging { source: BudgetExceeded },

    /// The planner found a candidate or a reference of a cleaned extent, and the round,
    /// starting from the same state, none.
    #[snafu(display("a recycling round found nothing to recycle"))]
    NothingToRecycle,
}

impl RecycleError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadWatermark { source, .. } | Self::FindUncleaned { source, .. } => {
                source.kind()
            }
            Self::FindForgotten { source, .. }
            | Self::Forget { source, .. }
            | Self::QueueForgotten { source }
            | Self::FindCandidates { source }
            | Self::CheckReferences { source }
            | Self::QueueUnused { source }
            | Self::Dequeue { source } => source.kind(),
            Self::CheckPins { source, .. } => source.kind(),
            Self::CheckUses { source, .. } => source.kind(),
            Self::ReleaseCombo { source, .. } | Self::ReleaseLabel { source, .. } => source.kind(),
            Self::ReserveStaging { source } => source.kind(),
            Self::UnknownDimension { .. } | Self::NothingToRecycle => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests;
