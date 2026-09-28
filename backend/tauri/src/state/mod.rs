#[derive(Debug)]
pub(crate) enum ConditionalReplaceResult<T> {
    Replaced(T),
    Conflict { actual_version: u64 },
}

pub mod application;
pub mod clash_config;
pub mod mirror;
pub mod profiles;
pub mod session_state;

pub(crate) mod mutation;
