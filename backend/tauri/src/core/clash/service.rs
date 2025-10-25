use super::core::CoreManager;
use std::sync::Arc;

pub struct ClashCoreService {
    core_manager: Arc<CoreManager>,
    // runtime_config_service: Arc<ClashRuntimeConfigService>,
}
