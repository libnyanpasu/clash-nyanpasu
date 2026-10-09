//! The desktop tray rendering model.

use nyanpasu_config::{
    application::{ClashCore, ProxiesSelectorMode, TrayMenuMode},
    clash::config::overrides::Mode,
};

/// What a [`nyanpasu_core::effects::plan::TrayRefresh::Full`] rebuild renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayMenuDesired {
    pub menu_mode: TrayMenuMode,
    pub selector_mode: ProxiesSelectorMode,
    /// Only a Premium core offers the script mode item.
    pub core: ClashCore,
}

/// What a [`nyanpasu_core::effects::plan::TrayRefresh::Part`] redraw reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayPartDesired {
    pub mode: Mode,
    pub system_proxy: bool,
    pub tun: bool,
    pub text: bool,
    pub traffic: bool,
}

/// Everything the tray renders, whichever of the two groups changed.
///
/// A part redraw still carries the menu inputs: the tray keeps the latest view
/// and renders every later rebuild from it, including the ones it triggers
/// itself when the proxy list changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayView {
    pub menu: TrayMenuDesired,
    pub part: TrayPartDesired,
}
