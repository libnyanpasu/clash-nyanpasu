//! The core lifecycle, system proxy, TUN, service mode and OS integration.

nyanpasu_rpc::namespace!(expose = [http, local]);

pub mod core;
pub mod hotkeys;
pub mod icons;
pub mod network;
pub mod os;
pub mod proxy;
pub mod service;
pub mod status;
pub mod update;
pub mod window;
