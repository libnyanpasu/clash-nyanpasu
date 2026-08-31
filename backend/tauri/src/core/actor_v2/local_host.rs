#[cfg(target_os = "macos")]
use std::sync::Arc;

use anyhow::Result;
use camino::Utf8PathBuf;
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{
    ControlOptions, CoreControl, CoreKind, CoreManager, CoreSpec, LocalIpcPolicy, ManagerOptions,
};

use crate::utils::path::PathResolver;

pub async fn build(paths: &PathResolver) -> Result<CoreControl> {
    let runtime_root = paths.app_config_dir().join("runtime");
    let options = ManagerOptions {
        runtime_dir: Some(to_utf8(runtime_root.join("control"))?),
        local_ipc_policy: LocalIpcPolicy::Disable,
        ..ManagerOptions::default()
    };
    let manager = CoreManager::builder(options);

    #[cfg(target_os = "macos")]
    let manager = manager.dns_controller(Arc::new(
        nyanpasu_core_manager::dns::macos::MacosDnsController::new(
            "State:/Network/Service/nyanpasu-dns/DNS".into(),
        ),
    ));

    let manager = manager.build().await?;
    let source_dir = to_utf8(runtime_root.join("staging"))?;
    let working_dir = to_utf8(paths.app_data_dir().to_owned())?;

    Ok(CoreControl::spawn(
        manager,
        ControlOptions::new(source_dir, working_dir),
    ))
}

pub fn core_spec(core: &ClashCore) -> Result<CoreSpec> {
    core_spec_with(core, crate::core::find_binary_path)
}

fn core_spec_with(
    core: &ClashCore,
    find_binary: impl FnOnce(&nyanpasu_utils::core::CoreType) -> std::io::Result<std::path::PathBuf>,
) -> Result<CoreSpec> {
    let core_type = core.into();
    let kind = match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::Meow => CoreKind::Meow,
    };
    let binary_path = find_binary(&core_type)?;

    Ok(CoreSpec {
        kind,
        binary_path: to_utf8(binary_path)?,
        version: None,
        features: vec![],
    })
}

fn to_utf8(path: std::path::PathBuf) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("path is not valid UTF-8: {}", path.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_local_host_spawns_under_a_temp_root() {
        let root = tempfile::TempDir::new().unwrap();
        let paths =
            PathResolver::with_base_dirs(root.path().join("config"), root.path().join("data"));

        let control = build(&paths).await.unwrap();

        let _ = control.status();
        assert!(!control.executor_is_closed());
    }

    #[test]
    fn core_spec_maps_every_clash_core_variant() {
        let root = tempfile::TempDir::new().unwrap();
        let variants = [
            (ClashCore::ClashPremium, CoreKind::ClashPremium),
            (ClashCore::ClashRs, CoreKind::ClashRust),
            (ClashCore::Mihomo, CoreKind::Mihomo),
            (ClashCore::MihomoAlpha, CoreKind::Mihomo),
            (ClashCore::ClashRsAlpha, CoreKind::ClashRust),
            (ClashCore::Meow, CoreKind::Meow),
        ];

        for (core, expected_kind) in variants {
            let spec = core_spec_with(&core, |core_type| {
                Ok(root.path().join(core_type.get_executable_name()))
            })
            .unwrap();
            assert_eq!(spec.kind, expected_kind);
            assert!(!spec.binary_path.as_str().is_empty());
        }
    }
}
