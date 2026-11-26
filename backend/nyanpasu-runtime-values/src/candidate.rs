//! The session-local identity of a candidate build kept for inspection.

use std::{fmt, sync::Arc};

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use specta::Type;

/// Names one candidate build in the running session's inspection index: 21
/// characters of the URL-safe nanoid alphabet. It is never a file name and
/// grants no read; only the index of the session that issued it resolves it.
///
/// One pointer wide, so the errors that can name a candidate grow by no
/// more than that.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CandidateSnapshotId(Arc<String>);

/// A string that is not a candidate snapshot ID.
#[derive(Debug, Snafu)]
#[snafu(display("not a candidate snapshot id"))]
pub struct InvalidCandidateSnapshotId;

impl CandidateSnapshotId {
    const LENGTH: usize = 21;

    /// Validates an ID received from outside the session.
    ///
    /// # Errors
    /// Rejects anything but 21 characters of `[A-Za-z0-9_-]`.
    pub fn parse(raw: impl Into<String>) -> Result<Self, InvalidCandidateSnapshotId> {
        let value = raw.into();
        if value.len() != Self::LENGTH
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(InvalidCandidateSnapshotId);
        }
        Ok(Self(Arc::new(value)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CandidateSnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for CandidateSnapshotId {
    type Error = InvalidCandidateSnapshotId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<CandidateSnapshotId> for String {
    fn from(value: CandidateSnapshotId) -> Self {
        Arc::unwrap_or_clone(value.0)
    }
}

impl Type for CandidateSnapshotId {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

#[cfg(test)]
mod tests {
    use super::CandidateSnapshotId;

    #[test]
    fn an_id_is_one_pointer_wide() {
        assert_eq!(size_of::<Option<CandidateSnapshotId>>(), size_of::<usize>());
    }

    #[test]
    fn only_ids_of_the_nanoid_shape_decode() {
        let valid = "a_-012345678901234567";
        assert_eq!(CandidateSnapshotId::parse(valid).unwrap().as_str(), valid);
        assert_eq!(
            serde_json::from_value::<CandidateSnapshotId>(serde_json::json!(valid))
                .unwrap()
                .as_str(),
            valid
        );
        for invalid in [
            "",
            "../escape",
            "/tmp/escape",
            "aaaaaaaaaaaaaaaaaaaa/",
            "aaaaaaaaaaaaaaaaaaaa.",
            "aaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(CandidateSnapshotId::parse(invalid).is_err(), "{invalid:?}");
            assert!(
                serde_json::from_value::<CandidateSnapshotId>(serde_json::json!(invalid)).is_err()
            );
        }
    }
}
