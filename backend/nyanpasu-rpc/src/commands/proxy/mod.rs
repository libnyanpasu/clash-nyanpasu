//! The running routing state: proxy groups and nodes, selection, latency tests, the proxy
//! mode, providers, rules and closing connections.

nyanpasu_rpc::namespace!(expose = [http, local]);

pub mod connections;
pub mod delay;
pub mod groups;
pub mod mode;
pub mod providers;
pub mod rules;
