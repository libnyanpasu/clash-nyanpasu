use std::collections::BTreeMap;

use crate::config::IVerge;
use nyanpasu_config::state::{
    PersistentState,
    window::{WindowLabel, WindowState},
};

const MAIN_WINDOW_LABEL: &str = "main";

pub(crate) fn persistent_state_from_legacy(legacy: &IVerge) -> anyhow::Result<PersistentState> {
    let state = if let Some(window_state) = legacy.window_size_state.as_ref() {
        super::yaml_convert::<_, WindowState>(window_state)?
    } else {
        #[allow(deprecated)]
        let Some(position) = legacy.window_size_position.as_ref() else {
            return Ok(PersistentState::default());
        };
        window_state_from_position(position)
    };

    Ok(PersistentState {
        window_state: BTreeMap::from([(WindowLabel(MAIN_WINDOW_LABEL.into()), state)]),
    })
}

fn window_state_from_position(position: &[f64]) -> WindowState {
    WindowState {
        width: position.first().copied().unwrap_or_default().max(0.0) as u32,
        height: position.get(1).copied().unwrap_or_default().max(0.0) as u32,
        x: position.get(2).copied().unwrap_or_default() as i32,
        y: position.get(3).copied().unwrap_or_default() as i32,
        maximized: false,
        fullscreen: false,
    }
}
