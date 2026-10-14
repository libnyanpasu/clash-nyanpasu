//! Diagnostic data contracts and the environment collection port.
//! Build metadata is supplied by the host, never discovered by this crate.

pub mod direct_egress;
pub mod net;
pub mod os;

use std::{borrow::Cow, collections::BTreeMap, io};

use serde::Serialize;

#[derive(Debug, Serialize, Clone, specta::Type)]
pub struct BuildInfo {
    pub app_name: &'static str,
    pub app_version: &'static str,
    pub pkg_version: &'static str,
    pub commit_hash: &'static str,
    pub commit_author: &'static str,
    pub commit_date: &'static str,
    pub build_date: &'static str,
    pub build_profile: &'static str,
    pub build_platform: &'static str,
    pub rustc_version: &'static str,
    pub llvm_version: &'static str,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct DeviceInfo<'a> {
    /// Device name, such as "Intel Core i5-8250U CPU @ 1.60GHz x 8"
    pub cpu: Vec<Cow<'a, str>>,
    /// GPU name, such as "Intel UHD Graphics 620 (Kabylake GT2)"
    // pub gpu: Cow<'a, str>,
    /// Memory size in bytes
    pub memory: Cow<'a, str>,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct EnvInfo<'a> {
    pub os: Cow<'a, str>,
    pub arch: Cow<'a, str>,
    pub core: CoreInfo<'a>,
    pub device: DeviceInfo<'a>,
    pub build_info: Cow<'a, BuildInfo>,
    // TODO: add service info
    // pub service_info: xxx
}

pub type CoreInfo<'a> = BTreeMap<Cow<'a, str>, Cow<'a, str>>;

/// Collects a diagnostic snapshot without borrowing from the adapter.
pub trait EnvironmentCollector: Send + Sync + 'static {
    fn collect(&self) -> io::Result<EnvInfo<'static>>;
}
