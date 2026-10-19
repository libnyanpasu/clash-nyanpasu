//! The errors of the catalog. Each variant names the step that failed; `kind()` gives the
//! recovery decision of §7.5:
//!
//! - a storage failure delegates to the [`StorageError`];
//! - a stored record that fails validation is `Corrupted`;
//! - a release the budget has no room for is `Local`: the round of maintenance ends there.

use snafu::Snafu;

use crate::{
    ErrorKind,
    budget::BudgetExceeded,
    layout::{keys::KeyError, record::RecordProblem},
    storage::StorageError,
};

/// What is wrong with a stored catalog record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogProblem {
    /// The value is not one record of a known format.
    Encoding { problem: RecordProblem },
    /// A combination has another number of labels than the series has dimensions.
    LabelCount { count: usize, expected: usize },
    /// The key names id 0, reserved for the overflow marker and the whole-overflow
    /// combination, or `u32::MAX`, which the lookup table reserves.
    ReservedId,
    /// The key names a label dimension the schema does not have.
    UnknownDimension { labels: usize },
    /// The value repeats the value of an earlier id of the same dimension.
    DuplicateValue { first: u32 },
    /// The value is longer than a label value can be.
    TooLong { len: usize },
    /// The values of the dimension up to this one take `bytes` bytes, more than a dictionary
    /// holds.
    ValueBytes { bytes: u64 },
    /// The label at this dimension position is not a value of its dictionary.
    UnknownLabel { position: usize, label: u32 },
    /// Every label is the overflow marker: that is the implicit combination 0.
    WholeOverflow,
    /// The schema has no series, so it has no combinations.
    NoSeries,
}

/// Loading the catalog from a snapshot.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum LoadError {
    #[snafu(display("failed to scan the label values"))]
    ScanLabels { source: StorageError },

    #[snafu(display("failed to scan the combinations"))]
    ScanCombos { source: StorageError },

    /// The stored catalog does not fit the catalog budget, so nothing past the version's
    /// own structure was allocated. It is `Local`, not `Corrupted`: a budget admits work but
    /// does not judge validity (§6.5), and a catalog written under a larger budget is
    /// intact; calling it corrupted would let recovery discard it after a profile change. A
    /// key corrupted into a huge id is refused all the same. The host raises the budget or
    /// reports the mismatch.
    #[snafu(display("the stored catalog does not fit the catalog budget"))]
    ReserveMemory { source: BudgetExceeded },

    #[snafu(display("label value key {key:02x?} is malformed"))]
    LabelKey { key: Box<[u8]>, source: KeyError },

    #[snafu(display("label value {id} of dimension {dimension} is invalid: {problem:?}"))]
    InvalidLabel {
        dimension: u16,
        id: u32,
        problem: CatalogProblem,
    },

    #[snafu(display("combination key {key:02x?} is malformed"))]
    ComboKey { key: Box<[u8]>, source: KeyError },

    #[snafu(display("combination {id} is invalid: {problem:?}"))]
    InvalidCombo { id: u32, problem: CatalogProblem },
}

impl LoadError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanLabels { source } | Self::ScanCombos { source } => source.kind(),
            Self::ReserveMemory { source } => source.kind(),
            Self::LabelKey { source, .. } | Self::ComboKey { source, .. } => source.kind(),
            Self::InvalidLabel { .. } | Self::InvalidCombo { .. } => ErrorKind::Corrupted,
        }
    }
}

/// Resolving a combination or a label value in a draft.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ResolveError {
    /// Staging cannot cover a draft allocation; the caller discards the batch and draft.
    #[snafu(display("staging cannot cover the catalog draft's copies"))]
    StageResolution { source: BudgetExceeded },

    #[snafu(display("failed to read the reverse lookup of combinations"))]
    ReadComboIndex { source: StorageError },

    #[snafu(display("a reverse lookup record is invalid: {problem:?}"))]
    InvalidComboIndex { problem: CatalogProblem },

    #[snafu(display(
        "the reverse lookup names combination {id}, which is not live or has other labels"
    ))]
    StaleComboIndex { id: u32 },

    #[snafu(display("failed to write label value {id} of dimension {dimension}"))]
    WriteLabel {
        dimension: u16,
        id: u32,
        source: StorageError,
    },

    #[snafu(display("failed to write combination {id}"))]
    WriteCombo { id: u32, source: StorageError },

    #[snafu(display("failed to write the reverse lookup of combination {id}"))]
    WriteComboIndex { id: u32, source: StorageError },

    #[snafu(display("failed to count the uses of the labels of combination {id}"))]
    CountUses { id: u32, source: UsesError },
}

impl ResolveError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadComboIndex { source }
            | Self::WriteLabel { source, .. }
            | Self::WriteCombo { source, .. }
            | Self::WriteComboIndex { source, .. } => source.kind(),
            Self::InvalidComboIndex { .. } | Self::StaleComboIndex { .. } => ErrorKind::Corrupted,
            Self::CountUses { source, .. } => source.kind(),
            Self::StageResolution { source } => source.kind(),
        }
    }
}

/// Reading or changing how many live combinations use a label value (§5.3).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum UsesError {
    #[snafu(display("failed to read the uses of label value {id} of dimension {dimension}"))]
    ReadUses {
        dimension: u16,
        id: u32,
        source: StorageError,
    },

    #[snafu(display(
        "the uses record of label value {id} of dimension {dimension} is invalid: {problem:?}"
    ))]
    InvalidUses {
        dimension: u16,
        id: u32,
        problem: CatalogProblem,
    },

    /// The stored count is 0, which only damage can cause: a count of 0 deletes the record.
    #[snafu(display("the uses record of label value {id} of dimension {dimension} counts 0"))]
    ZeroUses { dimension: u16, id: u32 },

    /// The stored count is `u64::MAX`, which only damage can cause.
    #[snafu(display("the uses of label value {id} of dimension {dimension} cannot be counted"))]
    UsesOverflow { dimension: u16, id: u32 },

    /// A released combination uses the value, but its count is 0, which only damage can cause.
    #[snafu(display(
        "a combination uses label value {id} of dimension {dimension}, which has no uses"
    ))]
    UsesUnderflow { dimension: u16, id: u32 },

    #[snafu(display("failed to write the uses of label value {id} of dimension {dimension}"))]
    WriteUses {
        dimension: u16,
        id: u32,
        source: StorageError,
    },

    #[snafu(display("failed to delete the uses of label value {id} of dimension {dimension}"))]
    DeleteUses {
        dimension: u16,
        id: u32,
        source: StorageError,
    },
}

impl UsesError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadUses { source, .. }
            | Self::WriteUses { source, .. }
            | Self::DeleteUses { source, .. } => source.kind(),
            Self::InvalidUses { .. }
            | Self::ZeroUses { .. }
            | Self::UsesOverflow { .. }
            | Self::UsesUnderflow { .. } => ErrorKind::Corrupted,
        }
    }
}

/// Reading, pinning or unpinning a combination's pins.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum PinError {
    /// A diagnostic scan could not read the pin namespaces.
    #[snafu(display("failed to scan pins"))]
    ScanPins { source: StorageError },

    /// Pin keys, nonzero counts and the captured catalog disagree.
    #[snafu(display("pin records disagree with the captured catalog or their counts"))]
    InvalidPins,

    #[snafu(display("failed to read pin key {key:02x?}"))]
    ReadKey {
        key: Box<[u8]>,
        source: StorageError,
    },

    #[snafu(display("the record of pin key {key:02x?} is invalid: {problem:?}"))]
    InvalidKey {
        key: Box<[u8]>,
        problem: CatalogProblem,
    },

    #[snafu(display("failed to write pin key {key:02x?}"))]
    WriteKey {
        key: Box<[u8]>,
        source: StorageError,
    },

    #[snafu(display("failed to delete pin key {key:02x?}"))]
    DeleteKey {
        key: Box<[u8]>,
        source: StorageError,
    },

    #[snafu(display("failed to read the pins of combination {id}"))]
    ReadCount { id: u32, source: StorageError },

    #[snafu(display("the pin record of combination {id} is invalid: {problem:?}"))]
    InvalidRecord { id: u32, problem: CatalogProblem },

    /// The stored count is 0, which only damage can cause: a count of 0 deletes the record.
    #[snafu(display("the pin record of combination {id} counts 0"))]
    ZeroPins { id: u32 },

    /// The stored count is `u64::MAX`, which only damage can cause.
    #[snafu(display("the pin count of combination {id} cannot be incremented"))]
    CountOverflow { id: u32 },

    /// A key pins the combination, but its count is 0, which only damage can cause.
    #[snafu(display("a key pins combination {id}, which has no pins"))]
    CountUnderflow { id: u32 },

    #[snafu(display("failed to write the pins of combination {id}"))]
    WriteCount { id: u32, source: StorageError },

    #[snafu(display("failed to delete the pins of combination {id}"))]
    DeleteRecord { id: u32, source: StorageError },
}

impl PinError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanPins { source } => source.kind(),
            Self::InvalidPins => ErrorKind::Corrupted,
            Self::ReadKey { source, .. }
            | Self::WriteKey { source, .. }
            | Self::DeleteKey { source, .. }
            | Self::ReadCount { source, .. }
            | Self::WriteCount { source, .. }
            | Self::DeleteRecord { source, .. } => source.kind(),
            Self::InvalidKey { .. }
            | Self::InvalidRecord { .. }
            | Self::ZeroPins { .. }
            | Self::CountOverflow { .. }
            | Self::CountUnderflow { .. } => ErrorKind::Corrupted,
        }
    }
}

/// Releasing a combination or a label value in a draft.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ReleaseError {
    /// Staging refused the release before any catalog or storage mutation.
    #[snafu(display("staging cannot cover the catalog release's copies"))]
    StageRelease { source: BudgetExceeded },

    /// Debug builds check that a released combination is not pinned.
    #[cfg(debug_assertions)]
    #[snafu(display("failed to check the pins of combination {id}"))]
    CheckPin { id: u32, source: PinError },

    #[snafu(display("no room in the catalog budget to release an id"))]
    ReserveRelease { source: BudgetExceeded },

    #[snafu(display("failed to delete combination {id}"))]
    DeleteCombo { id: u32, source: StorageError },

    #[snafu(display("failed to delete the reverse lookup of combination {id}"))]
    DeleteComboIndex { id: u32, source: StorageError },

    #[snafu(display("failed to delete label value {id} of dimension {dimension}"))]
    DeleteLabel {
        dimension: u16,
        id: u32,
        source: StorageError,
    },

    #[snafu(display("failed to take the uses of combination {id} off its labels"))]
    UncountUses { id: u32, source: UsesError },
}

impl ReleaseError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            #[cfg(debug_assertions)]
            Self::CheckPin { source, .. } => source.kind(),
            Self::ReserveRelease { source } | Self::StageRelease { source } => source.kind(),
            Self::DeleteCombo { source, .. }
            | Self::DeleteComboIndex { source, .. }
            | Self::DeleteLabel { source, .. } => source.kind(),
            Self::UncountUses { source, .. } => source.kind(),
        }
    }
}
