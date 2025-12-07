use crate::config::{
    Profile, ProfileContentGuard, ProfileMetaGetter,
    nyanpasu::ClashCore,
    profile::item_type::{ProfileItemType, ProfileUid},
};
use enumflags2::{BitFlag, BitFlags};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use strum::EnumString;

use super::Logs;

#[derive(Default, Debug, Clone, Serialize, Deserialize, specta::Type)]
/// 后处理输出
pub struct PostProcessingOutput {
    /// 局部链的输出
    pub scopes: IndexMap<ProfileUid, IndexMap<ProfileUid, Logs>>,
    /// 全局链的输出
    pub global: IndexMap<ProfileUid, Logs>,
    /// 根据配置进行的分析建议
    pub advice: Logs,
    // TODO: 增加 Meta 信息
}

#[derive(Debug, Clone)]
pub struct ChainItem {
    pub uid: String,
    pub data: ChainTypeWrapper,
}

impl From<&ProfileContentGuard<'_>> for Option<ChainItem> {
    fn from(value: ProfileContentGuard<'_>) -> Self {
        match value.profile {
            Profile::Script(script) => Some(ChainItem::new_script(
                script.uid(),
                ChainTypeWrapper::Script {
                    kind: script.script_type,
                    data: value.content.clone(),
                },
            )),
            Profile::Merge(merge) => Some(ChainItem::new_merge(
                merge.uid().to_string(),
                serde_yaml::from_slice(&value.content)
                    .inspect_err(|e| tracing::error!(profile_id = %merge.uid(), "failed to parse merge profile yaml: {e:#?}"))
                    .ok()?,
            )),
            _ => None,
        }
    }
}

type Data = bytes::Bytes;

#[derive(Debug, Clone)]
pub enum ChainTypeWrapper {
    Merge(Mapping),
    Script { kind: ScriptType, data: Data },
}

impl ChainTypeWrapper {
    pub fn new_js(data: Data) -> Self {
        Self::Script {
            kind: ScriptType::JavaScript,
            data,
        }
    }

    pub fn new_lua(data: Data) -> Self {
        Self::Script {
            kind: ScriptType::Lua,
            data,
        }
    }

    pub fn new_merge(data: Mapping) -> Self {
        Self::Merge(data)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainType {
    #[serde(rename = "merge")]
    Merge,
    #[serde(rename = "script")]
    Script(ScriptType),
}

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

impl From<ScriptType> for ProfileItemType {
    fn from(value: ScriptType) -> Self {
        ProfileItemType::Script(value)
    }
}

impl ChainItem {
    pub fn new_merge(uid: String, data: Mapping) -> Self {
        Self {
            uid: "merge".to_string(),
            data: ChainTypeWrapper::Merge(Mapping::new()),
        }
    }

    /// 内建支持一些脚本
    pub fn builtin() -> Vec<(BitFlags<ClashCore>, ChainItem)> {
        // meta 的一些处理
        let meta_guard = ChainItem::new_script(
            "verge_meta_guard",
            ChainTypeWrapper::new_js(bytes::Bytes::from_static(include_bytes!(
                "./builtin/meta_guard.js"
            ))),
        );

        // meta 1.13.2 alpn string 转 数组
        let hy_alpn = ChainItem::new_script(
            "verge_hy_alpn",
            ChainTypeWrapper::new_js(bytes::Bytes::from_static(include_bytes!(
                "./builtin/meta_hy_alpn.js"
            ))),
        );

        // 修复配置的一些问题
        let config_fixer = ChainItem::new_script(
            "config_fixer",
            ChainTypeWrapper::new_js(bytes::Bytes::from_static(include_bytes!(
                "./builtin/config_fixer.js"
            ))),
        );

        // 移除或转换 Clash Rs 不支持的字段
        let clash_rs_comp = ChainItem::new_script(
            "clash_rs_comp",
            ChainTypeWrapper::new_lua(bytes::Bytes::from_static(include_bytes!(
                "./builtin/clash_rs_comp.lua"
            ))),
        );

        vec![
            (ClashCore::Mihomo | ClashCore::MihomoAlpha, hy_alpn),
            (ClashCore::Mihomo | ClashCore::MihomoAlpha, meta_guard),
            (ClashCore::all(), config_fixer),
            (ClashCore::ClashRs.into(), clash_rs_comp),
        ]
    }

    pub fn new_script<U: Into<String>, D: Into<ChainTypeWrapper>>(uid: U, data: D) -> Self {
        Self {
            uid: uid.into(),
            data: data.into(),
        }
    }
}
