//! Stored profile definitions, subscriptions, chains and the proxy group layout.

nyanpasu_rpc::namespace!(expose = [http, local]);

pub mod content;
pub mod layout;
pub mod providers;
pub mod runtime;
pub mod store;
pub mod sync;
