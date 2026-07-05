use std::collections::BTreeMap;

use crate::{
    config::{Config, IVerge, nyanpasu as legacy_app},
    state::mirror::WindowLegacyBridge,
};
use nyanpasu_config::state::{
    PersistentState,
    window::{WindowLabel, WindowState},
};

const MAIN_WINDOW_LABEL: &str = "main";

pub struct LegacyWindowBridge;

impl WindowLegacyBridge for LegacyWindowBridge {
    fn mirror(&self, snap: &PersistentState) -> anyhow::Result<()> {
        // TODO(actor-migration): compatibility bridge for legacy Config::verge().window_size_state.
        // Reason: window-state readers still consume Config::verge() during typed actor rollout.
        // Remove when window state commands and restore paths use SessionStateClient.
        let verge = Config::verge();
        let mut draft = verge.draft();
        apply_session_state_to_legacy_verge(&mut draft, snap)?;
        drop(draft);
        verge.apply();
        Ok(())
    }

    fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
        let legacy = Config::verge().data().clone();
        persistent_state_from_legacy(&legacy)
    }
}

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

pub(crate) fn apply_session_state_to_legacy_verge(
    draft: &mut IVerge,
    snap: &PersistentState,
) -> anyhow::Result<()> {
    draft.window_size_state = snap
        .window_state
        .get(&WindowLabel(MAIN_WINDOW_LABEL.into()))
        .map(super::yaml_convert::<_, legacy_app::WindowState>)
        .transpose()?;

    draft.window_size_position = draft.window_size_state.as_ref().map(|state| {
        vec![
            state.width as f64,
            state.height as f64,
            state.x as f64,
            state.y as f64,
        ]
    });
    Ok(())
}
