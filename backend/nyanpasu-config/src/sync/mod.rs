//! Portable projections of the configuration documents for cross-device sync.
//!
//! Each document mixes fields that describe the user's intent with fields
//! bound to one device. This module is the pure side of sync: it decides which
//! fields are portable ([`scope`]), turns a document into the canonical bytes
//! that are hashed and uploaded ([`canonical`], [`document`]), and merges two
//! devices' projections against a common base ([`merge`]). It performs no IO
//! and contains no platform-conditional code, so equal content hashes equally
//! on every platform and build.

pub mod canonical;
pub mod document;
pub mod merge;
pub mod scope;

pub use canonical::{CanonicalError, ObjectId, canonical_json, canonical_text};
pub use document::{
    DocumentError, PortableDocument, apply_application, apply_clash, apply_profiles,
    portable_profile_files, project_application, project_clash, project_profiles,
};
pub use merge::{
    ConflictKind, ConflictPolicy, ConflictSide, MergeError, MergeInput, MergeOutput, UnitConflict,
    merge_document,
};
pub use scope::{APPLICATION_SCOPES, CLASH_SCOPES, DocumentKind, FieldScope};
