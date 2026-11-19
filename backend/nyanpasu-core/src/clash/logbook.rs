//! The persistent event log exposed by Meow's `/logbook` endpoint.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Default, Deserialize, Serialize, Type)]
pub struct MeowLogbookQuery {
    pub before: Option<u64>,
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct MeowLogbookPage {
    pub enabled: bool,
    pub entries: Vec<MeowLogbookEntry>,
    pub more: bool,
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct MeowLogbookEntry {
    pub id: u64,
    pub time: String,
    pub last: Option<String>,
    pub count: u32,
    pub kind: String,
    pub level: String,
    pub subject: Option<String>,
    pub message: String,
    #[specta(type = Option<BTreeMap<String, Option<clash_api::api::JsonValue>>>)]
    pub fields: Option<BTreeMap<String, serde_json::Value>>,
}
