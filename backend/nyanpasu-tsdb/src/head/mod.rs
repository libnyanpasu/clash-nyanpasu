//! The head (§4.2): the in-memory view of every journal record still present in a snapshot.
//!
//! - Series: one accumulator per open (tier, bucket).
//! - Events: one row buffer per stream.
//!
//! Its size is bounded by the profile. There is an immutable version per publication, and
//! versions share unchanged pages.

mod events;
mod journal;
mod series;
