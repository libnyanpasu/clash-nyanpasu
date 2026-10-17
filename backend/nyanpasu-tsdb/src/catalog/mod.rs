//! The catalog (§5): one dictionary per label, plus the table of combinations.
//!
//! - It is resident in memory as an immutable version per publication. The writer derives
//!   the next version by copying only the pages it changes (§7.1).
//! - Its size is limited in bytes, counted at peak (§5.1).
//! - A new combination that does not fit goes down the fold ladder (§5.2).

mod combos;
mod dictionary;
mod paged;
mod persist;
