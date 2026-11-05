//! What this host is and reaches: its bundle flags, the system service's
//! state, direct egress and cached icons.

use base64::{Engine, prelude::BASE64_STANDARD};
use nyanpasu_core::client::{NyanpasuClient, direct_egress::DirectEgress};
use serde::Serialize;

use super::Result;
use crate::State;

#[derive(Serialize, specta::Type)]
pub struct IconData {
    pub data_url: String,
}

/// `StatusInfo` 的 additive 镜像：字段逐一复制（specta 不支持 `serde(flatten)`，
/// 见本文件 `GetSysProxyResponse` 的同款处理），追加 actor 投影字段。
/// wire 是原结构的严格超集，前端既有消费点不受影响。
#[derive(Serialize, specta::Type)]
pub struct ServiceStatusInfo {
    pub name: std::borrow::Cow<'static, str>,
    pub version: std::borrow::Cow<'static, str>,
    pub status: nyanpasu_ipc::types::ServiceStatus,
    pub server: Option<nyanpasu_ipc::api::status::StatusResBody<'static>>,
    pub compat: nyanpasu_core::service::compat::ServiceCompat,
    pub phase: nyanpasu_core::actor_v2::service_actor::ServicePhase,
    pub restart_attempts: u8,
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn is_portable(client: State<'_, NyanpasuClient>) -> Result<bool> {
    Ok(client.is_portable())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn probe_direct_egress(client: State<'_, NyanpasuClient>) -> Result<DirectEgress> {
    Ok(client.probe_direct_egress().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_cached_icon(client: State<'_, NyanpasuClient>, url: String) -> Result<IconData> {
    let (mime, bytes) = client.get_cached_icon(&url).await?;
    Ok(IconData {
        data_url: format!("data:{mime};base64,{}", BASE64_STANDARD.encode(bytes)),
    })
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn status_service(client: State<'_, NyanpasuClient>) -> Result<ServiceStatusInfo> {
    let info = client.service_status();
    Ok(ServiceStatusInfo {
        name: info.name,
        version: info.version,
        status: info.status,
        server: info.server,
        compat: info.compat,
        phase: info.phase,
        restart_attempts: info.restart_attempts,
    })
}
