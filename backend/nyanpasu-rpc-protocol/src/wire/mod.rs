//! Application wire projections; canonical foundation values retain their original owners.

pub mod app_update;
pub mod clash;
pub mod configuration;
mod connection_receive;
pub mod core;
pub mod events;
pub mod host;
pub mod logs;
pub mod native;
pub mod profiles;
pub mod receive;
pub mod runtime;
pub mod storage;
pub mod streams;
pub mod updater;

#[cfg(test)]
mod closure_tests;

#[cfg(test)]
mod receiving_tests;
