//! Local endpoints that only the current OS user can reach, owned by the first
//! process that binds them.
//!
//! The desktop and the headless server use [`endpoint`] for their local RPC
//! listener; the CLI uses it to find one. Ownership needs no lock file on
//! Windows, where a named pipe created as the first instance is exclusive, and a
//! private directory with an advisory claim file on Unix.

pub mod endpoint;
