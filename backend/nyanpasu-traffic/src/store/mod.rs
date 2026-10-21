//! Traffic's tsdb schema and actor-owned storage. Queries lend catalog strings and session
//! keys to the same aggregation path. Overflow ids have no display key: their usage counts
//! in totals and other/merged nodes, never distinct values or filters. A fully overflowed
//! closed event retains its target in FoldedLabel; lost dimensions restore as unknown,
//! None or empty. Auxiliary state holds complete dimensions and needs no catalog pins.
mod legacy;
mod read;
mod schema;
#[cfg(test)]
mod tests;
mod write;

use std::{cell::Cell, fmt::Display, ops::ControlFlow, time::Duration};

use nyanpasu_tsdb::{
    ErrorKind, EventSeq, Profile, Reader, RecoveryError, RecoveryProgress, RecoveryView, Tsdb,
    storage::{Storage, StorageError},
};
use serde::de::DeserializeOwned;

use crate::{ActiveConnection, FlushBatch, SessionMeta, TrafficError, TrafficResult};

pub type SessionState = Option<(SessionMeta, Vec<ActiveConnection>)>;
pub type Opened<S> = (TrafficDb<S>, SessionState);

pub struct TrafficDb<S: Storage> {
    db: Tsdb<S>,
    reader: Reader<S>,
    last_batch: u64,
    batch_bytes: u64,
    arena: Vec<u8>,
    retention: Option<Duration>,
    /// Queries can flag a backend failure through &self; the actor remains the sole owner.
    recovery_needed: Cell<bool>,
    legacy: legacy::Progress,
}

#[derive(Debug, thiserror::Error)]
pub enum FlushError {
    #[error("batch was not applied: {0}")]
    NotApplied(TrafficError),
    #[error("traffic recovery is required: {0}")]
    Recovery(TrafficError),
    #[error("batch applied, maintenance failed: {0}")]
    Applied(TrafficError),
    /// Committed prefixes; remove them with `FlushBatch::uncommitted` before requeueing.
    #[error("batch partially applied: {error}")]
    Partial {
        error: TrafficError,
        closed: usize,
        active: usize,
    },
    #[error("indivisible traffic input is too large: {bytes} bytes, limit {limit}")]
    Oversized { bytes: u64, limit: u64 },
}

pub enum Recovery {
    Restored(SessionState),
    WaitingForReaders,
}

impl<S: Storage> TrafficDb<S> {
    pub fn open(storage: S, profile: Profile) -> Result<Opened<S>, RecoveryError> {
        let batch_bytes = profile.batch_bytes;
        let (db, ((last_batch, state), legacy)) =
            Tsdb::open_with(storage, schema::schema(), profile, |view| {
                let legacy = legacy::restore(&view)?;
                Ok((restore(view)?, legacy))
            })?;
        Ok((
            Self {
                reader: db.reader(),
                db,
                last_batch,
                batch_bytes,
                arena: Vec::new(),
                retention: None,
                recovery_needed: Cell::new(false),
                legacy,
            },
            state,
        ))
    }

    /// The host's current policy, also installed before the first query after opening.
    pub fn set_retention(&mut self, retention: Option<Duration>) {
        self.retention = retention;
    }

    pub fn next_event_seq(&mut self) -> EventSeq {
        self.db.next_event_seq()
    }

    pub fn needs_recovery(&self) -> bool {
        self.recovery_needed.get()
    }

    pub fn batch_bytes(&self) -> u64 {
        self.batch_bytes
    }

    pub fn recover(
        &mut self,
        reopen: impl FnOnce() -> Result<S, StorageError>,
    ) -> TrafficResult<Recovery> {
        self.recovery_needed.set(true);
        match self.db.recover_with(reopen, restore).map_err(storage)? {
            RecoveryProgress::WaitingForReaders => Ok(Recovery::WaitingForReaders),
            RecoveryProgress::Recovered {
                restored: (last_batch, state),
                ..
            } => {
                self.last_batch = last_batch;
                self.recovery_needed.set(false);
                Ok(Recovery::Restored(state))
            }
        }
    }

    pub fn flush(
        &mut self,
        batch: &FlushBatch,
        now_ms: i64,
        retention: Option<Duration>,
    ) -> Result<(), FlushError> {
        self.retention = retention;
        if self.recovery_needed.get() {
            return Err(FlushError::Recovery(storage("recovery is pending")));
        }
        write::flush(self, batch, now_ms, retention)
    }

    fn check(&self) -> TrafficResult<()> {
        if self.recovery_needed.get() {
            Err(storage("recovery is pending"))
        } else {
            Ok(())
        }
    }
}

fn restore<S: Storage>(view: RecoveryView<'_, S>) -> Result<(u64, SessionState), RecoveryError> {
    let meta = view.aux_get(b"m", |value| value.map(decode::<SessionMeta>).transpose())??;
    let mut active = Vec::new();
    if let Some(error) = view.aux_scan(|key, value| {
        if key.first() != Some(&b'a') {
            return ControlFlow::Continue(());
        }
        match decode::<ActiveConnection>(value) {
            Ok(conn) if &key[1..] == conn.id.as_bytes() => {
                active.push(conn);
                ControlFlow::Continue(())
            }
            Ok(_) => ControlFlow::Break(invalid("active record key disagrees with its id")),
            Err(error) => ControlFlow::Break(error),
        }
    })? {
        return Err(error);
    }
    if meta.is_none() && !active.is_empty() {
        return Err(invalid("active records without session metadata"));
    }
    Ok((
        view.last_batch().unwrap_or(0),
        meta.map(|meta| (meta, active)),
    ))
}

fn decode<T: DeserializeOwned>(value: &[u8]) -> Result<T, RecoveryError> {
    let Some((&1, bytes)) = value.split_first() else {
        return Err(invalid("unknown auxiliary record format"));
    };
    postcard::from_bytes(bytes).map_err(|source| RecoveryError::Restore {
        kind: ErrorKind::Corrupted,
        source: Box::new(source),
    })
}

fn invalid(message: &str) -> RecoveryError {
    RecoveryError::Restore {
        kind: ErrorKind::Corrupted,
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )),
    }
}

pub(crate) fn storage(error: impl Display) -> TrafficError {
    TrafficError::Storage(error.to_string())
}
