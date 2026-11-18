//! Configuration sync storage over Apache OpenDAL.
//!
//! This is the only crate that names `opendal`. It adapts an OpenDAL operator
//! to the byte-level `RemoteStore` port and builds operators with typed
//! builders, so nothing registers itself before `main` and no process-wide
//! scheme registry or HTTP transport is involved: every operator carries the
//! `reqwest::Client` the shell passes in, and with it the shell's proxy policy.
//! Deadlines and retries live here, in the network adapter.

mod factory;
mod secrets;
mod store;

pub use factory::OpendalStoreFactory;
pub use secrets::FileSecretStore;
pub use store::OpendalRemoteStore;
