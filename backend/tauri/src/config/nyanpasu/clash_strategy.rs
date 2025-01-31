use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct ClashStrategy {
    pub external_controller_port_strategy: ExternalControllerPortStrategy,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExternalControllerPortStrategy {
    Fixed,
    Random,
    #[default]
    AllowFallback,
}

impl super::IVerge {
    pub fn get_external_controller_port_strategy(&self) -> ExternalControllerPortStrategy {
        self.clash_strategy
            .as_ref()
            .unwrap_or(&ClashStrategy::default())
            .external_controller_port_strategy
            .to_owned()
    }
}
