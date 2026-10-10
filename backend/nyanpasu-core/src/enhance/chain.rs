use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

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
