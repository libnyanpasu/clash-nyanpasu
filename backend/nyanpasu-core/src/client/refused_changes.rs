//! Changes that no caller waited on — library sync, the tray and global
//! shortcuts — that kept a candidate when they failed. Nothing else would
//! report them, so the configuration status shows each origin's latest one
//! for as long as this session holds it. Nothing persists them.
//!
//! The source owners report here as they decide, not their callers when
//! they resume: a commit at the version it published, a failure after the
//! version it was refused at, numbered by its owner. Each origin belongs to
//! one owner, so a commit clears only the failures before it, and a failure
//! shows only if nothing after it was decided.

use nyanpasu_runtime_values::{
    candidate::CandidateSnapshotId,
    configuration::{BackgroundChangeOrigin, RefusedBackgroundChange},
};
use tokio::sync::watch;

use crate::state::mutation::ports::OutcomeRecord;

/// Each origin's latest failure that named a candidate.
#[derive(Debug, Clone, Default)]
pub struct RefusedChangesSnapshot {
    pub seq: u64,
    rows: Vec<(Position, RefusedBackgroundChange)>,
    /// The latest commit each origin reported.
    committed: Vec<(BackgroundChangeOrigin, Position)>,
}

impl RefusedChangesSnapshot {
    pub fn entries(&self) -> impl Iterator<Item = &RefusedBackgroundChange> {
        self.rows.iter().map(|(_, change)| change)
    }
}

/// Where an owner decided an outcome: a commit at the version it published,
/// then the failures at that version, in the order the owner numbered them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    version: u64,
    refusal: Option<u64>,
}

/// The status's record of refused background changes. Its value is
/// replaced whole under the channel's lock, so owners reporting at once
/// cannot interleave an update.
#[derive(Clone)]
pub(crate) struct RefusedChanges(watch::Sender<RefusedChangesSnapshot>);

impl RefusedChanges {
    pub(crate) fn new() -> Self {
        Self(watch::Sender::new(RefusedChangesSnapshot::default()))
    }

    pub(crate) fn snapshot(&self) -> RefusedChangesSnapshot {
        self.0.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<RefusedChangesSnapshot> {
        self.0.subscribe()
    }
}

impl OutcomeRecord for RefusedChanges {
    fn committed(&self, origins: &[BackgroundChangeOrigin], version: u64) {
        let commit = Position {
            version,
            refusal: None,
        };
        self.0.send_if_modified(|snapshot| {
            for origin in origins {
                match snapshot
                    .committed
                    .iter_mut()
                    .find(|(kept, _)| kept == origin)
                {
                    Some((_, latest)) => *latest = (*latest).max(commit),
                    None => snapshot.committed.push((*origin, commit)),
                }
            }
            let before = snapshot.rows.len();
            snapshot
                .rows
                .retain(|(at, change)| !origins.contains(&change.origin) || *at > commit);
            let cleared = snapshot.rows.len() != before;
            if cleared {
                snapshot.seq += 1;
            }
            cleared
        });
    }

    fn refused(
        &self,
        origin: BackgroundChangeOrigin,
        version: u64,
        refusal: u64,
        message: String,
        candidate: CandidateSnapshotId,
    ) {
        let at = Position {
            version,
            refusal: Some(refusal),
        };
        self.0.send_if_modified(|snapshot| {
            if snapshot
                .committed
                .iter()
                .any(|(kept, committed)| *kept == origin && *committed > at)
            {
                return false;
            }
            let row = snapshot
                .rows
                .iter()
                .position(|(_, change)| change.origin == origin);
            let entry = (
                at,
                RefusedBackgroundChange {
                    origin,
                    message,
                    candidate_snapshot_id: Some(candidate),
                },
            );
            match row {
                Some(row) if snapshot.rows[row].0 > at => return false,
                Some(row) => snapshot.rows[row] = entry,
                None => snapshot.rows.push(entry),
            }
            snapshot.seq += 1;
            true
        });
    }
}
