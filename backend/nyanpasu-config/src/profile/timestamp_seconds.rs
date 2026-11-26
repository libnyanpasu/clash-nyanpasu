//! Keep the persisted optional epoch-second format, including pre-epoch rounding.

use jiff::Timestamp;
use serde::Serializer;

/// The previous time serializer rounded fractional seconds toward negative infinity.
pub(super) fn serialize<S: Serializer>(
    timestamp: &Option<Timestamp>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match timestamp {
        Some(timestamp) => {
            serializer.serialize_some(&(timestamp.as_nanosecond().div_euclid(1_000_000_000) as i64))
        }
        None => serializer.serialize_none(),
    }
}

pub(super) use jiff::fmt::serde::timestamp::second::optional::deserialize;
