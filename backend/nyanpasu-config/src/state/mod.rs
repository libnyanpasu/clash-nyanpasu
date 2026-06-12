//! Persistent State and only if recover while startup, not notify other module while state changed, just save to disk
//!

pub mod window;

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

#[derive(Debug, Default, Serialize, Deserialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Serialize, Deserialize, specta::Type)))]
pub struct PersistentState {
    pub window_state: BTreeMap<window::WindowLabel, window::WindowState>,
}
