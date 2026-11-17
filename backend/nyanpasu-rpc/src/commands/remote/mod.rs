//! Remote-access administration: the HTTP and local socket settings, access tokens and browser
//! sessions.
//!
//! They are reachable from the desktop shell and the same-user local socket, never over HTTP:
//! no bearer token or browser session can hold `remote`.

nyanpasu_rpc::namespace!(expose = [local]);

pub mod sessions;
pub mod settings;
pub mod tokens;
