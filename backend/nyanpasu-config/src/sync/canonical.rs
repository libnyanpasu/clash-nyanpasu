//! Canonical bytes and content ids.
//!
//! An item's id is the SHA-256 of its canonical bytes, so the encoding must
//! not depend on key order, YAML formatting, line endings or the platform.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use snafu::{Snafu, ensure};

/// The lowercase hex SHA-256 of an item's canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ObjectId(String);

/// Why bytes or a string cannot be canonicalized.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum CanonicalError {
    #[snafu(display("text item is not valid UTF-8"))]
    NotUtf8,
    #[snafu(display("{value:?} is not a lowercase hex SHA-256 digest"))]
    MalformedObjectId { value: String },
}

impl ObjectId {
    /// The id of `bytes`, which must already be canonical.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }

    /// Validates a digest read from a manifest or an object key.
    ///
    /// # Errors
    ///
    /// Fails unless `value` is 64 lowercase hex characters.
    pub fn parse(value: &str) -> Result<Self, CanonicalError> {
        ensure!(
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            MalformedObjectIdSnafu { value }
        );
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ObjectId {
    type Err = CanonicalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for ObjectId {
    type Error = CanonicalError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ObjectId> for String {
    fn from(id: ObjectId) -> Self {
        id.0
    }
}

/// UTF-8 JSON with object keys sorted and no insignificant whitespace.
///
/// Array order is preserved: ordered collections are arrays in the schemas.
#[must_use]
pub fn canonical_json(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            out.push(b'{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(Value::from(key.as_str()).to_string().as_bytes());
                out.push(b':');
                write_canonical(item, out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        scalar => out.extend_from_slice(scalar.to_string().as_bytes()),
    }
}

/// The canonical form of a text item: UTF-8, no byte order mark, LF line endings.
///
/// A file that differs from another only by its line endings or a leading BOM
/// therefore has the same id.
///
/// # Errors
///
/// Fails with [`CanonicalError::NotUtf8`] when the bytes are not UTF-8.
pub fn canonical_text(bytes: &[u8]) -> Result<Vec<u8>, CanonicalError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CanonicalError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    Ok(text.replace("\r\n", "\n").into_bytes())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_sorts_keys_at_every_depth_and_keeps_array_order() {
        let value = json!({"b": [3, {"z": 1, "a": "é\"\n"}], "a": null});
        assert_eq!(
            String::from_utf8(canonical_json(&value)).unwrap(),
            r#"{"a":null,"b":[3,{"a":"é\"\n","z":1}]}"#
        );
    }

    /// Golden digests: a change here changes every device's item ids.
    #[test]
    fn golden_digests_are_stable() {
        let document = json!({"document": "application", "portable_revision": 1,
            "payload": {"theme_mode": "dark", "language": "en"}});
        assert_eq!(
            ObjectId::of(&canonical_json(&document)).as_str(),
            "e11014055e82554717dff83cff6e10e4befcc8f70ac80e9e61727d731bfc7646"
        );
        assert_eq!(
            ObjectId::of(&canonical_text(b"a: 1\r\nb: 2\r\n").unwrap()).as_str(),
            ObjectId::of(b"a: 1\nb: 2\n").as_str()
        );
    }

    #[test]
    fn text_drops_bom_and_carriage_returns() {
        assert_eq!(
            canonical_text(b"\xef\xbb\xbfa\r\nb\n").unwrap(),
            b"a\nb\n".to_vec()
        );
        assert_eq!(canonical_text(b"\xff"), Err(CanonicalError::NotUtf8));
    }

    #[test]
    fn object_id_rejects_other_shapes() {
        assert!(ObjectId::parse(&"a".repeat(64)).is_ok());
        assert!(ObjectId::parse(&"A".repeat(64)).is_err());
        assert!(ObjectId::parse("abc").is_err());
    }
}
