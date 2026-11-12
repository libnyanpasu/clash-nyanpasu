//! Local endpoints that only the current OS user can reach, owned by the first
//! process that binds them, and the single-instance claim built on them.
//!
//! The desktop and the headless server use [`endpoint`] for their local RPC
//! listener; the CLI uses it to find one. Ownership needs no lock file on
//! Windows, where a named pipe created as the first instance is exclusive, and a
//! private directory with an advisory claim file on Unix.
//!
//! Both shells [claim](instance::claim) an instance endpoint before composing
//! anything, so a second process for the same directory hands over its launch
//! and exits instead of starting a second application.

pub mod endpoint;
pub mod instance;
