use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use strum::EnumString;

use super::Logs;

#[derive(Default, Debug, Clone, Serialize, Deserialize, specta::Type)]
/// 后处理输出
pub struct PostProcessingOutput {
    /// 局部链的输出
    pub scopes: IndexMap<String, IndexMap<String, Logs>>,
    /// 全局链的输出
    pub global: IndexMap<String, Logs>,
    /// 根据配置进行的分析建议
    pub advice: Logs,
    // TODO: 增加 Meta 信息
}

type Data = String;

#[derive(Debug, Clone)]
pub struct ScriptWrapper(pub ScriptType, pub Data);

#[derive(
    Debug,
    EnumString,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    Eq,
    PartialEq,
    Hash,
    specta::Type,
)]
#[strum(serialize_all = "snake_case")]
pub enum ScriptType {
    #[default]
    #[serde(rename = "javascript")]
    #[strum(serialize = "javascript")]
    JavaScript,
    #[serde(rename = "lua")]
    Lua,
}
