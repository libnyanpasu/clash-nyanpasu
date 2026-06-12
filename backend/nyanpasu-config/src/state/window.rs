use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Deserialize, Serialize, Type, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct WindowLabel(pub String);

#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub maximized: bool,
    pub fullscreen: bool,
}
