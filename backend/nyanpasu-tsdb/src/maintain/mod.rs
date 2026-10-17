//! Maintenance (§9.2) consists of synchronous calls made by the owner's actor, each with a
//! work budget.
//!
//! - Every round is one atomic transaction with bounded inputs.
//! - A round that does not fit in the budget waits for the next call.
//! - When the backlog exceeds the profile, the next commit runs the most urgent round first
//!   (§4.3).

mod consolidate;
mod recycle;
mod retention;
