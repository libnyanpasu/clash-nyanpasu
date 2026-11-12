//! Response values shared by native and HTTP callers.

// TODO: remove this struct use Sysproxy
#[derive(specta::Type, serde::Serialize)]
pub struct GetSysProxyResponse {
    // Sysproxy fields (manually defined),
    // because specta not support serde(flatten)
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub bypass: String,

    // old version compatible
    pub server: String,
}

/// `StatusInfo` 的 additive 镜像：字段逐一复制（specta 不支持 `serde(flatten)`，
/// 见本文件 `GetSysProxyResponse` 的同款处理），追加 actor 投影字段。
/// wire 是原结构的严格超集，前端既有消费点不受影响。
#[derive(serde::Serialize, specta::Type)]
pub struct ServiceStatusInfo {
    pub name: std::borrow::Cow<'static, str>,
    pub version: std::borrow::Cow<'static, str>,
    pub status: nyanpasu_ipc::types::ServiceStatus,
    pub server: Option<nyanpasu_ipc::api::status::StatusResBody<'static>>,
    pub compat: nyanpasu_runtime_values::core_status::ServiceCompat,
    pub phase: nyanpasu_runtime_values::core_status::ServicePhase,
    pub restart_attempts: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StorageEntry {
    pub key: String,
    /// Raw JSON-encoded value string.
    pub value: String,
}
