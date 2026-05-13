//! Persistent State and only if recover while startup, not notify other module while state changed, just save to disk
//!

pub mod window;

use derive_builder::Builder;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Default, Serialize, Deserialize, Type, Builder)]
#[builder(default, derive(Debug, Serialize, Deserialize, specta::Type))]
pub struct PersistentState {
    pub window_state: IndexMap<window::WindowLabel, window::WindowState>,
}
