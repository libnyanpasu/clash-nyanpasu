use std::sync::Arc;

use camino::Utf8PathBuf;
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{
    ControlOptions, CoreControl, CoreKind, CoreManager, CoreSpec, LocalIpcPolicy, ManagerOptions,
    native_store::{FsNativeStore, StoreOwner, legacy_kind},
};
use serde::Serialize;
use snafu::{ResultExt, Snafu};

use nyanpasu_paths::PathResolver;

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to inspect {}", path.display()))]
    InspectConfig {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to read core kind from {}", path.display()))]
    ReadCoreKind {
        path: std::path::PathBuf,
        source: nyanpasu_core_manager::native_store::StoreError,
    },
    #[snafu(display("failed to build core manager"))]
    BuildManager {
        source: nyanpasu_core_manager::Error,
    },
}

pub async fn build(
    paths: &PathResolver,
    shutdown_helper: &std::path::Path,
) -> Result<CoreControl, Error> {
    let runtime_root = paths.app_config_dir().join("runtime");
    let options = ManagerOptions {
        runtime_dir: Some(runtime_root.join("control")),
        local_ipc_policy: LocalIpcPolicy::Disable,
        windows_shutdown_helper: Some(shutdown_helper.to_owned()),
        ..ManagerOptions::default()
    };
    let working_dir = paths.app_data_dir().to_owned();
    let config_path = paths.application_config_path();
    let config_path = if config_path.try_exists().context(InspectConfigSnafu {
        path: config_path.as_std_path(),
    })? {
        config_path
    } else {
        paths.nyanpasu_config_path()
    };
    let native_store = Arc::new(FsNativeStore::new(
        working_dir.clone(),
        StoreOwner::current(),
        legacy_kind(config_path.as_std_path()).context(ReadCoreKindSnafu {
            path: config_path.as_std_path(),
        })?,
    ));
    let manager = CoreManager::builder(options).native_store(native_store);

    #[cfg(target_os = "macos")]
    let manager = manager.dns_controller(Arc::new(
        nyanpasu_core_manager::dns::macos::MacosDnsController::new(
            "State:/Network/Service/nyanpasu-dns/DNS".into(),
        ),
    ));

    let manager = manager.build().await.context(BuildManagerSnafu)?;
    let source_dir = runtime_root.join("staging");

    Ok(CoreControl::spawn(
        manager,
        ControlOptions::new(source_dir, working_dir),
    ))
}

/// A failure of locating the binary a core is started from.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreSpecError {
    #[snafu(display("could not find the {core} core binary"))]
    FindCoreBinary {
        #[specta(type = String)]
        core: ClashCore,
        #[serde(skip)]
        source: std::io::Error,
    },
}

pub fn core_spec(
    core: &ClashCore,
    paths: &nyanpasu_paths::PathResolver,
) -> Result<CoreSpec, CoreSpecError> {
    core_spec_with(core, |core| {
        paths.find_binary_path(core.get_executable_name())
    })
}

fn core_spec_with(
    core: &ClashCore,
    find_binary: impl FnOnce(&nyanpasu_utils::core::CoreType) -> std::io::Result<Utf8PathBuf>,
) -> Result<CoreSpec, CoreSpecError> {
    let core_type = core.into();
    let kind = match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::Meow | ClashCore::MeowAlpha => CoreKind::Meow,
    };
    let binary_path = find_binary(&core_type).context(FindCoreBinarySnafu { core: *core })?;

    Ok(CoreSpec {
        kind,
        binary_path,
        version: None,
        features: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_local_host_spawns_under_a_temp_root() {
        let root = tempfile::TempDir::new().unwrap();
        let paths =
            crate::client::tests::test_paths(root.path().join("config"), root.path().join("data"));

        let control = build(&paths, &root.path().join("helper.exe"))
            .await
            .unwrap();

        let _ = control.status();
        assert!(!control.executor_is_closed());
    }

    #[tokio::test]
    async fn the_local_host_reads_the_typed_config_before_the_legacy_file() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = crate::client::tests::test_paths(root.path(), root.path().join("data"));
        std::fs::write(paths.application_config_path(), "core: mihomo\n").unwrap();
        std::fs::write(paths.nyanpasu_config_path(), "invalid: [").unwrap();

        let control = build(&paths, &root.path().join("helper.exe"))
            .await
            .unwrap();
        assert!(!control.executor_is_closed());

        std::fs::remove_file(paths.application_config_path()).unwrap();
        assert!(
            build(&paths, &root.path().join("helper.exe"))
                .await
                .is_err()
        );
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
            (ClashCore::MeowAlpha, CoreKind::Meow),
        ];

        for (core, expected_kind) in variants {
            let spec = core_spec_with(&core, |core_type| {
                Ok(
                    Utf8PathBuf::from_path_buf(root.path().join(core_type.get_executable_name()))
                        .unwrap(),
                )
            })
            .unwrap();
            assert_eq!(spec.kind, expected_kind);
            assert!(!spec.binary_path.as_str().is_empty());
        }
    }
}
