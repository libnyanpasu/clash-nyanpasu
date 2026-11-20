//! The running routing state: proxy groups and nodes, selection, latency tests, the proxy
//! mode, providers, rules, the core's resolver and closing connections.

nyanpasu_rpc::namespace!(expose = [http, local]);

pub mod connections;
pub mod delay;
pub mod dns;
pub mod groups;
pub mod mode;
pub mod providers;
pub mod rules;
