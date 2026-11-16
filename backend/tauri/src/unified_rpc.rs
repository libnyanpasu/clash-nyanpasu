//! Desktop binding over the shared RPC crate.

pub(crate) mod icons;
pub mod native;
mod permission_table;

#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod tests;
