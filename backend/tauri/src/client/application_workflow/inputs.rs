//! Fixed source inputs and content for one runtime target and build.
use super::impact;
use nyanpasu_config::{
    application::NyanpasuAppConfig,
    clash::config::ClashConfig,
    profile::{ManagedProfilePath, Profiles},
    runtime::executor::{PortError, ProfileContentSource},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Default)]
pub(in crate::client) struct FrozenProfileContent(pub BTreeMap<String, Result<String, String>>);

impl ProfileContentSource for FrozenProfileContent {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        self.0
            .get(&path.to_string())
            .cloned()
            .map(|result| result.map_err(Into::into))
            .ok_or_else(|| -> PortError {
                format!("profile content was not captured: {path}").into()
            })?
    }
}

pub(in crate::client) struct RuntimeInputs {
    pub app: NyanpasuAppConfig,
    pub clash: ClashConfig,
    pub profiles: Arc<Profiles>,
    pub content: FrozenProfileContent,
}

impl RuntimeInputs {
    pub fn target_key(&self) -> anyhow::Result<String> {
        let projections = (
            impact::application_target(&self.app).ok_or_else(|| {
                anyhow::anyhow!("application runtime identity cannot be serialized")
            })?,
            impact::clash_target(&self.clash)
                .ok_or_else(|| anyhow::anyhow!("clash runtime identity cannot be serialized"))?,
            impact::profiles_target(&self.profiles)
                .ok_or_else(|| anyhow::anyhow!("profiles runtime identity cannot be serialized"))?,
            &self.content.0,
        );
        Ok(nyanpasu_core_manager::payload_digest(&serde_json::to_vec(
            &projections,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> RuntimeInputs {
        RuntimeInputs {
            app: NyanpasuAppConfig::default(),
            clash: ClashConfig::default(),
            profiles: Arc::new(Profiles::default()),
            content: FrozenProfileContent::default(),
        }
    }

    #[test]
    fn target_identity_covers_all_runtime_inputs_but_not_gui_settings() {
        let mut inputs = inputs();
        let first = inputs.target_key().unwrap();
        inputs.app.language = nyanpasu_config::application::I18nLanguage::English;
        assert_eq!(inputs.target_key().unwrap(), first);
        inputs.app.enable_builtin_enhanced = !inputs.app.enable_builtin_enhanced;
        let application_changed = inputs.target_key().unwrap();
        assert_ne!(application_changed, first);
        inputs.clash.enable_tun_mode = !inputs.clash.enable_tun_mode;
        let clash_changed = inputs.target_key().unwrap();
        assert_ne!(clash_changed, application_changed);
        inputs
            .content
            .0
            .insert("profile.yaml".into(), Ok("mode: rule".into()));
        let content_changed = inputs.target_key().unwrap();
        assert_ne!(content_changed, clash_changed);
        inputs
            .content
            .0
            .insert("profile.yaml".into(), Ok("mode: direct".into()));
        assert_ne!(inputs.target_key().unwrap(), content_changed);
    }
}
