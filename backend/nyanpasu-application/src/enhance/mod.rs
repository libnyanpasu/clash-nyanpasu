mod runtime_builder;

pub use runtime_builder::{
    RuntimeBuildError, RuntimeBuildInput, RuntimeBuildLog, RuntimeBuilder, builtin_transforms_for,
    derive_tun_flavor,
};

#[derive(
    Debug,
    strum::EnumString,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    Default,
    Eq,
    PartialEq,
    Hash,
    specta::Type,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    #[default]
    #[strum(serialize = "javascript")]
    #[serde(rename = "javascript")]
    JavaScript,
    Lua,
}

#[derive(Debug, Clone)]
pub struct ScriptWrapper(pub ScriptType, pub String);

#[cfg(test)]
mod tests {
    use super::ScriptType;

    #[test]
    fn javascript_keeps_its_legacy_wire_name() {
        assert_eq!(
            serde_json::to_string(&ScriptType::JavaScript).unwrap(),
            "\"javascript\""
        );
        assert_eq!(
            serde_json::from_str::<ScriptType>("\"javascript\"").unwrap(),
            ScriptType::JavaScript
        );
        assert_eq!(
            "javascript".parse::<ScriptType>().unwrap(),
            ScriptType::JavaScript
        );
    }
}
