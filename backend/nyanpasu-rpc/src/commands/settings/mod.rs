//! Ordinary application and Clash preferences, configuration sync and the frontend key-value
//! storage.

nyanpasu_rpc::namespace!(expose = [http, local]);

pub mod app;
pub mod clash;
pub mod network;
pub mod storage;
pub mod sync;
