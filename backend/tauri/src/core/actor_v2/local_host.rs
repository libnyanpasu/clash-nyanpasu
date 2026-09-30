use std::sync::Arc;

use anyhow::Result;
use camino::Utf8PathBuf;
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{
    ControlOptions, CoreControl, CoreKind, CoreManager, CoreSpec, LocalIpcPolicy, ManagerOptions,
    native_store::{FsNativeStore, StoreOwner, legacy_kind},
};
use serde::Serialize;
use snafu::{ResultExt, Snafu};

use crate::utils::path::PathResolver;

pub async fn build(
    paths: &PathResolver,
    cancellation: tokio_util::sync::CancellationToken,
    tasks: &tokio_util::task::TaskTracker,
    traffic: nyanpasu_traffic::TrafficResult<crate::core::traffic::TrafficClient>,
) -> Result<CoreControl> {
    let runtime_root = paths.app_config_dir().join("runtime");
    let options = ManagerOptions {
        cancel_token: cancellation.clone(),
        runtime_dir: Some(to_utf8(runtime_root.join("control"))?),
        local_ipc_policy: LocalIpcPolicy::Disable,
        ..ManagerOptions::default()
    };
    let working_dir = to_utf8(paths.app_data_dir().to_owned())?;
    let config_path = paths.application_config_path();
    let config_path = if config_path.try_exists()? {
        config_path
    } else {
        paths.nyanpasu_config_path()
    };
    let native_store = Arc::new(FsNativeStore::new(
        working_dir.clone(),
        StoreOwner::current(),
        legacy_kind(&config_path)?,
    ));
    let mut manager = CoreManager::builder(options).native_store(native_store);
    if let Ok(client) = &traffic {
        manager =
            manager.lifecycle_sink(Arc::new(super::traffic_host::LifecycleSink(client.clone())));
    }

    #[cfg(target_os = "macos")]
    let manager = manager.dns_controller(Arc::new(
        nyanpasu_core_manager::dns::macos::MacosDnsController::new(
            "State:/Network/Service/nyanpasu-dns/DNS".into(),
        ),
    ));

    let manager = manager.build().await?;
    let source_dir = to_utf8(runtime_root.join("staging"))?;

    let control = CoreControl::spawn(
        manager.clone(),
        ControlOptions::new(source_dir, working_dir),
    );
    let (owner_control, owner_traffic, token) = (control.clone(), traffic.clone(), cancellation);
    // Exact process exits are cleanup of this host's already-started runtime.
    // They reach traffic before its mailbox drain; cancellation refuses all
    // new sampling/query/start work and never invents an exit from a watch.
    tasks.spawn(async move {
        token.cancelled().await;
        if let Err(error) = owner_control.shutdown().await {
            tracing::error!(%error,"local core control shutdown failed");
            if owner_control.executor_is_closed()
                && let Err(error) = manager.shutdown().await
            {
                tracing::error!(%error,"local runtime cleanup failed");
            }
        }
        if let Ok(client) = owner_traffic
            && let Err(error) = client.shutdown().await
        {
            tracing::warn!(%error,"local traffic shutdown failed");
        }
    });
    Ok(control)
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
    #[snafu(display("the {core} core binary path is not valid UTF-8: {path}"))]
    CoreBinaryPathNotUtf8 {
        #[specta(type = String)]
        core: ClashCore,
        path: String,
    },
}

pub fn core_spec(core: &ClashCore) -> Result<CoreSpec, CoreSpecError> {
    core_spec_with(core, crate::core::find_binary_path)
}

fn core_spec_with(
    core: &ClashCore,
    find_binary: impl FnOnce(&nyanpasu_utils::core::CoreType) -> std::io::Result<std::path::PathBuf>,
) -> Result<CoreSpec, CoreSpecError> {
    let core_type = core.into();
    let kind = match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::Meow => CoreKind::Meow,
    };
    let binary_path = find_binary(&core_type).context(FindCoreBinarySnafu { core: *core })?;
    let binary_path = Utf8PathBuf::from_path_buf(binary_path).map_err(|path| {
        CoreSpecError::CoreBinaryPathNotUtf8 {
            core: *core,
            path: path.to_string_lossy().into_owned(),
        }
    })?;

    Ok(CoreSpec {
        kind,
        binary_path,
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

        let cancellation = tokio_util::sync::CancellationToken::new();
        let tasks = tokio_util::task::TaskTracker::new();
        let control = build(
            &paths,
            cancellation.clone(),
            &tasks,
            Err(nyanpasu_traffic::StoreError::Unsupported),
        )
        .await
        .unwrap();

        let _ = control.status();
        assert!(!control.executor_is_closed());
        control.shutdown().await.unwrap();
        cancellation.cancel();
        tasks.close();
        tasks.wait().await;
        drop(control);
    }

    #[tokio::test]
    async fn the_local_host_reads_the_typed_config_before_the_legacy_file() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = PathResolver::with_base_dirs(root.path().to_owned(), root.path().join("data"));
        std::fs::write(paths.application_config_path(), "core: mihomo\n").unwrap();
        std::fs::write(paths.nyanpasu_config_path(), "invalid: [").unwrap();

        let control = build(
            &paths,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
            Err(nyanpasu_traffic::StoreError::Unsupported),
        )
        .await
        .unwrap();
        assert!(!control.executor_is_closed());

        std::fs::remove_file(paths.application_config_path()).unwrap();
        assert!(
            build(
                &paths,
                tokio_util::sync::CancellationToken::new(),
                &tokio_util::task::TaskTracker::new(),
                Err(nyanpasu_traffic::StoreError::Unsupported)
            )
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
