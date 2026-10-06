use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

/// What closing a window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowCloseBehavior {
    /// Destroy the window and its webview, freeing their memory.
    #[default]
    Destroy,
    /// Keep the window and its webview, hidden, so reopening is instant.
    Hide,
}

/// One window kind's choice: follow the global behavior or set its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowCloseOverride {
    #[default]
    Inherit,
    Destroy,
    Hide,
}

impl WindowCloseOverride {
    pub fn resolve(self, global: WindowCloseBehavior) -> WindowCloseBehavior {
        match self {
            Self::Inherit => global,
            Self::Destroy => WindowCloseBehavior::Destroy,
            Self::Hide => WindowCloseBehavior::Hide,
        }
    }
}

/// How each kind of window closes: a global behavior that every kind follows
/// unless it sets its own.
///
/// A patch names only the fields it changes, so an edit of one field, made
/// from a page that last saw the others a moment ago, leaves them alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Type)))]
#[patch(attribute(serde(default)))]
#[serde(default)]
pub struct WindowCloseSettings {
    pub global: WindowCloseBehavior,
    pub main: WindowCloseOverride,
    pub editor: WindowCloseOverride,
    pub tray_menu: WindowCloseOverride,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_resolves_against_the_global_behavior() {
        for global in [WindowCloseBehavior::Destroy, WindowCloseBehavior::Hide] {
            assert_eq!(WindowCloseOverride::Inherit.resolve(global), global);
            assert_eq!(
                WindowCloseOverride::Destroy.resolve(global),
                WindowCloseBehavior::Destroy
            );
            assert_eq!(
                WindowCloseOverride::Hide.resolve(global),
                WindowCloseBehavior::Hide
            );
        }
    }

    #[test]
    fn every_window_closes_by_destroying_by_default() {
        let settings = WindowCloseSettings::default();
        for window in [settings.main, settings.editor, settings.tray_menu] {
            assert_eq!(
                window.resolve(settings.global),
                WindowCloseBehavior::Destroy
            );
        }
    }

    #[test]
    fn a_patch_changes_only_the_fields_it_names() {
        let mut settings = WindowCloseSettings {
            global: WindowCloseBehavior::Hide,
            main: WindowCloseOverride::Destroy,
            ..WindowCloseSettings::default()
        };
        let mut patch = WindowCloseSettings::new_empty_patch();
        patch.tray_menu = Some(WindowCloseOverride::Hide);

        settings.apply(patch);

        assert_eq!(
            settings,
            WindowCloseSettings {
                global: WindowCloseBehavior::Hide,
                main: WindowCloseOverride::Destroy,
                editor: WindowCloseOverride::Inherit,
                tray_menu: WindowCloseOverride::Hide,
            }
        );
    }

    #[test]
    fn a_partial_document_fills_the_rest_with_defaults() {
        let settings: WindowCloseSettings = serde_yaml_ng::from_str("tray_menu: hide\n").unwrap();
        assert_eq!(settings.tray_menu, WindowCloseOverride::Hide);
        assert_eq!(settings.global, WindowCloseBehavior::Destroy);
        assert_eq!(settings.main, WindowCloseOverride::Inherit);
    }
}
