use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum MeowTrafficGroup {
    Host,
    App,
    Policy,
    Interface,
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct MeowTrafficLedgerQuery {
    pub from: String,
    pub to: String,
    pub by: MeowTrafficGroup,
    pub host: Option<String>,
    pub app: Option<String>,
    pub policy: Option<String>,
    pub interface: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct MeowTrafficLedger {
    pub enabled: bool,
    pub upload: u64,
    pub download: u64,
    pub requests: u64,
    pub rows: Vec<MeowTrafficRow>,
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct MeowTrafficRow {
    pub name: String,
    pub upload: u64,
    pub download: u64,
    pub requests: u64,
    #[serde(default)]
    pub via_upload: u64,
    #[serde(default)]
    pub via_download: u64,
}

#[derive(Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MeowStatistics {
    pub upload_total: u64,
    pub download_total: u64,
    pub proxies: BTreeMap<String, MeowProxyTotals>,
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct MeowProxyTotals {
    pub upload: u64,
    pub download: u64,
}
