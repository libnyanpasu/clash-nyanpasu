#[derive(Debug, Default)]
pub(crate) struct TypedConfigPatchPlan {
    pub application: Option<nyanpasu_config::application::NyanpasuAppConfigPatch>,
    pub session_state: Option<nyanpasu_config::state::PersistentStatePatch>,
    pub clash_config: Option<nyanpasu_config::clash::config::ClashConfigPatch>,
}

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
