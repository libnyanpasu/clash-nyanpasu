use std::{collections::BTreeMap, sync::Arc};

use crate::{
    config::{Config, Draft, IVerge, nyanpasu as legacy_app},
    state::mirror::{PreparedLegacyMirror, WindowLegacyBridge},
};
use nyanpasu_config::state::{
    PersistentState,
    window::{WindowLabel, WindowState},
};

const MAIN_WINDOW_LABEL: &str = "main";

pub struct LegacyWindowBridge {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl Default for LegacyWindowBridge {
    fn default() -> Self {
        Self::new(Arc::new(parking_lot::Mutex::new(())))
    }
}

impl LegacyWindowBridge {
    pub(crate) fn new(legacy_lock: Arc<parking_lot::Mutex<()>>) -> Self {
        Self { legacy_lock }
    }
}

struct PreparedWindowMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    store: Draft<IVerge>,
    projected: IVerge,
}

impl PreparedLegacyMirror for PreparedWindowMirror {
    fn apply(self: Box<Self>) {
        let Self {
            legacy_lock,
            store,
            projected,
        } = *self;
        let _guard = legacy_lock.lock();
        store.apply_update(|target| {
            target.window_size_state = projected.window_size_state.clone();
            target.window_size_position = projected.window_size_position.clone();
        });
    }
}

impl WindowLegacyBridge for LegacyWindowBridge {
    fn prepare(&self, snap: &PersistentState) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        // TODO(actor-migration): compatibility bridge for legacy Config::verge().window_size_state.
        // Reason: window-state readers still consume Config::verge() during typed actor rollout.
        // Remove when window state commands and restore paths use SessionStateClient.
        let store = Config::verge();
        let mut projected = {
            let _guard = self.legacy_lock.lock();
            store.data().clone()
        };
        apply_session_state_to_legacy_verge(&mut projected, snap)?;
        Ok(Box::new(PreparedWindowMirror {
            legacy_lock: Arc::clone(&self.legacy_lock),
            store,
            projected,
        }))
    }

    fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
        let _guard = self.legacy_lock.lock();
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
