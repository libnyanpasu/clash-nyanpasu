use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_yaml_ng::{Mapping, Value};
use std::{
    io::{Read, Write},
    marker::PhantomData,
};

pub trait Format {
    fn serialize<W: Write, T: Serialize>(
        &self,
        writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()>;
    fn deserialize<R: Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct YamlFormat;

impl Format for YamlFormat {
    fn serialize<W: Write, T: Serialize>(
        &self,
        mut writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(prefix) = prefix {
            writeln!(writer, "{}", prefix)?;
        }
        serde_yaml_ng::to_writer(writer, value)?;
        Ok(())
    }

    fn deserialize<R: Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T> {
        let value = serde_yaml_ng::from_reader(reader)?;
        Ok(value)
    }
}

/// Top-level key reserved for the stamp. Documents must not use it themselves.
pub const STAMP_KEY: &str = "_nyanpasu";

/// Which document a file holds and the schema revision its content is at.
/// Written into the file itself, so the revision survives the loss of any
/// external bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentStamp {
    pub document: String,
    pub schema_revision: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum StampError {
    #[error("failed to parse the document")]
    Parse(#[source] serde_yaml_ng::Error),
    #[error("the document is not a mapping")]
    NotAMapping,
    #[error("the `_nyanpasu` stamp is malformed")]
    Malformed(#[source] serde_yaml_ng::Error),
    #[error("the document already has a `_nyanpasu` key of its own")]
    ReservedKey,
    #[error("the document is stamped as `{found}`, expected `{expected}`")]
    WrongDocument { expected: String, found: String },
    #[error("the document is stamped at schema revision {found}, expected {expected}")]
    SchemaMismatch { expected: u64, found: u64 },
    #[error("the document has no `_nyanpasu` stamp")]
    Unstamped,
}

/// A parsed document split into its stamp, if any, and the rest.
#[derive(Debug, Clone, PartialEq)]
pub enum Inspected {
    Unstamped(Mapping),
    Stamped {
        stamp: DocumentStamp,
        payload: Mapping,
    },
}

impl Inspected {
    pub fn stamp(&self) -> Option<&DocumentStamp> {
        match self {
            Self::Unstamped(_) => None,
            Self::Stamped { stamp, .. } => Some(stamp),
        }
    }

    pub fn into_payload(self) -> Mapping {
        match self {
            Self::Unstamped(payload) | Self::Stamped { payload, .. } => payload,
        }
    }
}

/// Parses `raw` and takes the stamp out of it. A stamp key that does not hold
/// a well-formed stamp is an error rather than an unstamped document.
pub fn inspect(raw: &str) -> Result<Inspected, StampError> {
    let value: Value = serde_yaml_ng::from_str(raw).map_err(StampError::Parse)?;
    let Value::Mapping(mut payload) = value else {
        return Err(StampError::NotAMapping);
    };
    match payload.shift_remove(STAMP_KEY) {
        None => Ok(Inspected::Unstamped(payload)),
        Some(stamp) => {
            let stamp = serde_yaml_ng::from_value(stamp).map_err(StampError::Malformed)?;
            Ok(Inspected::Stamped { stamp, payload })
        }
    }
}

/// `payload` with `stamp` inserted as its first key.
pub fn stamp(payload: Mapping, stamp: &DocumentStamp) -> Result<Mapping, StampError> {
    if payload.contains_key(STAMP_KEY) {
        return Err(StampError::ReservedKey);
    }
    let stamp = serde_yaml_ng::to_value(stamp).map_err(StampError::Malformed)?;
    let mut stamped = Mapping::with_capacity(payload.len() + 1);
    stamped.insert(STAMP_KEY.into(), stamp);
    stamped.extend(payload);
    Ok(stamped)
}

/// A document persisted with a [`DocumentStamp`].
pub trait StampedDocument {
    const DOCUMENT: &'static str;
    /// The only schema revision this build reads and writes.
    const SCHEMA_REVISION: u64;
}

/// [`YamlFormat`] that stamps every write and only reads documents stamped as
/// `D` at `D::SCHEMA_REVISION`. Migrations own every other revision, and they
/// run before any file is loaded through this format.
pub struct StampedYamlFormat<D>(PhantomData<fn() -> D>);

impl<D> Default for StampedYamlFormat<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<D> Clone for StampedYamlFormat<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D> Copy for StampedYamlFormat<D> {}

impl<D: StampedDocument> std::fmt::Debug for StampedYamlFormat<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StampedYamlFormat")
            .field(&D::DOCUMENT)
            .finish()
    }
}

impl<D: StampedDocument> StampedYamlFormat<D> {
    fn document_stamp() -> DocumentStamp {
        DocumentStamp {
            document: D::DOCUMENT.to_owned(),
            schema_revision: D::SCHEMA_REVISION,
        }
    }
}

impl<D: StampedDocument> Format for StampedYamlFormat<D> {
    fn serialize<W: Write, T: Serialize>(
        &self,
        writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()> {
        let Value::Mapping(payload) = serde_yaml_ng::to_value(value)? else {
            return Err(StampError::NotAMapping.into());
        };
        let stamped = stamp(payload, &Self::document_stamp())?;
        YamlFormat.serialize(writer, &stamped, prefix)
    }

    fn deserialize<R: Read, T: DeserializeOwned>(&self, mut reader: R) -> anyhow::Result<T> {
        let mut raw = String::new();
        reader.read_to_string(&mut raw)?;
        let (stamp, payload) = match inspect(&raw)? {
            Inspected::Stamped { stamp, payload } => (stamp, payload),
            Inspected::Unstamped(_) => return Err(StampError::Unstamped.into()),
        };
        if stamp.document != D::DOCUMENT {
            return Err(StampError::WrongDocument {
                expected: D::DOCUMENT.to_owned(),
                found: stamp.document,
            }
            .into());
        }
        if stamp.schema_revision != D::SCHEMA_REVISION {
            return Err(StampError::SchemaMismatch {
                expected: D::SCHEMA_REVISION,
                found: stamp.schema_revision,
            }
            .into());
        }
        Ok(serde_yaml_ng::from_value(Value::Mapping(payload))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDocument;

    impl StampedDocument for TestDocument {
        const DOCUMENT: &'static str = "test";
        const SCHEMA_REVISION: u64 = 3;
    }

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct TestState {
        #[serde(default)]
        revision: u64,
        #[serde(default)]
        items: Vec<String>,
    }

    fn format() -> StampedYamlFormat<TestDocument> {
        StampedYamlFormat::default()
    }

    fn write(value: &impl Serialize, prefix: Option<&str>) -> String {
        let mut buf = Vec::new();
        format().serialize(&mut buf, value, prefix).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn read(raw: &str) -> anyhow::Result<TestState> {
        format().deserialize(raw.as_bytes())
    }

    fn stamp_error(error: anyhow::Error) -> StampError {
        error.downcast().expect("a stamp error")
    }

    #[test]
    fn round_trip_keeps_the_document_own_revision_field() {
        let state = TestState {
            revision: 42,
            items: vec!["a".into()],
        };
        let raw = write(&state, Some("# header"));

        assert!(raw.starts_with("# header\n_nyanpasu:\n"), "{raw}");
        assert_eq!(read(&raw).unwrap(), state);
        assert_eq!(
            inspect(&raw).unwrap().stamp(),
            Some(&DocumentStamp {
                document: "test".into(),
                schema_revision: 3,
            })
        );
    }

    #[test]
    fn inspect_tells_unstamped_from_malformed() {
        assert!(matches!(
            inspect("items: []\n").unwrap(),
            Inspected::Unstamped(_)
        ));
        assert!(matches!(
            inspect("_nyanpasu: {document: test}\nitems: []\n"),
            Err(StampError::Malformed(_))
        ));
        assert!(matches!(
            inspect("_nyanpasu: {document: test, schema_revision: 3, extra: 1}\n"),
            Err(StampError::Malformed(_))
        ));
        assert!(matches!(inspect("- a\n"), Err(StampError::NotAMapping)));
        assert!(matches!(
            inspect("items: [unclosed\n"),
            Err(StampError::Parse(_))
        ));
    }

    #[test]
    fn serialize_refuses_the_reserved_key_and_non_mappings() {
        #[derive(Serialize)]
        struct Clashing {
            _nyanpasu: u64,
        }
        let mut buf = Vec::new();
        let error = format()
            .serialize(&mut buf, &Clashing { _nyanpasu: 1 }, None)
            .unwrap_err();
        assert!(matches!(stamp_error(error), StampError::ReservedKey));

        let error = format().serialize(&mut buf, &vec![1, 2], None).unwrap_err();
        assert!(matches!(stamp_error(error), StampError::NotAMapping));
    }

    #[test]
    fn deserialize_accepts_only_this_document_at_this_revision() {
        let error = read("revision: 1\n").unwrap_err();
        assert!(matches!(stamp_error(error), StampError::Unstamped));

        let error =
            read("_nyanpasu: {document: other, schema_revision: 3}\nrevision: 1\n").unwrap_err();
        assert!(matches!(
            stamp_error(error),
            StampError::WrongDocument { found, .. } if found == "other"
        ));

        let error =
            read("_nyanpasu: {document: test, schema_revision: 4}\nrevision: 1\n").unwrap_err();
        assert!(matches!(
            stamp_error(error),
            StampError::SchemaMismatch {
                expected: 3,
                found: 4
            }
        ));
    }
}
