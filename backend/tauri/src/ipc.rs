pub mod profile;
pub mod proxy;
pub mod remote;
pub mod settings;
pub mod system;
pub mod traffic;

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ConfigurationStatusChanged(
    pub nyanpasu_runtime_values::configuration::ConfigurationStatus,
);

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "configuration-status-summary-changed")]
pub struct ConfigurationStatusSummaryChanged(
    pub nyanpasu_runtime_values::observation::ConfigurationStatusSummary,
);

#[cfg(test)]
mod tests {
    use nyanpasu_config::profile::{ManagedProfilePath, ProfileId};
    use nyanpasu_contracts::system_dns::SystemDnsError;
    use nyanpasu_core::{
        client::{
            ClientError, http_server::HttpServerControlError, local_socket::LocalSocketControlError,
        },
        state::config_error::ConfigError,
    };
    use nyanpasu_rpc::error::IpcError;
    use nyanpasu_runtime_values::profiles::ProfilesError;
    use nyanpasu_state_engine::ReplaceIfVersionError;
    use serde_json::json;

    use nyanpasu_contracts::{
        profiles::error::{ProfileFileError, SubscriptionFetchError},
        system_proxy::OsProxyError,
    };
    use nyanpasu_runtime_values::{
        effects::EffectsError, mutation::CommitAborted, runtime_error::RuntimeError,
    };

    fn wire(error: impl Into<ClientError>) -> serde_json::Value {
        serde_json::to_value(IpcError::from(error.into())).unwrap()
    }

    #[test]
    fn a_profiles_error_reaches_the_frontend_as_its_own_domain() {
        let wire = wire(ProfilesError::ProfileNotFound {
            uid: ProfileId("p1".into()),
        });

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "profiles",
                "error": { "kind": "profile_not_found", "uid": "p1" },
            })
        );
        assert_eq!(wire["message"], "profile not found: p1");
    }

    #[test]
    fn a_runtime_error_reaches_the_frontend_with_the_cores_own_kind() {
        let failure = RuntimeError::ApplyRuntime {
            failure: nyanpasu_runtime_values::runtime_error::CoreFailure {
                kind: Some(nyanpasu_core_manager::CoreErrorKind::ApplyFailed),
                message: "the core kept the previous configuration".into(),
                retryable: false,
                operation_id: None,
            },
            candidate_snapshot_id: None,
        };
        let wire = wire(failure);

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "runtime",
                "error": {
                    "kind": "apply_runtime",
                    "failure": {
                        "kind": "apply_failed",
                        "message": "the core kept the previous configuration",
                        "retryable": false,
                        "operation_id": null,
                    },
                },
            })
        );
        assert_eq!(
            serde_json::to_value(IpcError::from(RuntimeError::ConfigurationNotInEffect {
                reason: "waiting for a stable runtime".into(),
                candidate_snapshot_id: None,
            }))
            .unwrap()["kind"],
            json!({ "domain": "runtime", "error": { "kind": "configuration_not_in_effect", "reason": "waiting for a stable runtime" } })
        );
    }

    #[test]
    fn a_dns_flush_failure_names_the_command_and_its_exit_code() {
        let wire = serde_json::to_value(IpcError::from(SystemDnsError::FlushRejected {
            command: "ipconfig.exe",
            code: Some(5),
        }))
        .unwrap();

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "system_dns",
                "error": { "kind": "flush_rejected", "command": "ipconfig.exe", "code": 5 },
            })
        );
    }

    #[test]
    fn a_stopped_effects_owner_reaches_the_frontend_as_its_own_domain() {
        let wire = serde_json::to_value(IpcError::from(EffectsError::EffectsStopped)).unwrap();

        assert_eq!(
            wire["kind"],
            json!({ "domain": "effects", "error": { "kind": "effects_stopped" } })
        );
    }

    #[test]
    fn a_failed_os_proxy_write_names_where_it_was_going() {
        let wire = serde_json::to_value(IpcError::from(OsProxyError::WriteOsProxy {
            enable: true,
            host: "127.0.0.1".into(),
            port: 7890,
            source: "access denied".into(),
        }))
        .unwrap();

        assert_eq!(
            wire["kind"],
            json!({
                "domain": "system_proxy",
                "error": {
                    "kind": "write_os_proxy",
                    "enable": true,
                    "host": "127.0.0.1",
                    "port": 7890,
                },
            })
        );
        assert!(wire["detail"].as_str().unwrap().contains("access denied"));
    }

    #[test]
    fn unit_variants_and_context_free_kinds_serialize_their_tag_only() {
        assert_eq!(
            wire(ConfigError::ValidateHotkeyCapability)["kind"],
            json!({ "domain": "config", "error": { "kind": "validate_hotkey_capability" } })
        );
        for (error, kind) in [
            (
                HttpServerControlError::ReadStatusUnsupported,
                "read_status_unsupported",
            ),
            (
                HttpServerControlError::ChangeUnsupported,
                "change_unsupported",
            ),
        ] {
            assert_eq!(
                serde_json::to_value(IpcError::from(error)).unwrap()["kind"],
                json!({ "domain": "http_server", "error": { "kind": kind } })
            );
        }
        for (error, kind) in [
            (
                LocalSocketControlError::ReadStatusUnsupported,
                "read_status_unsupported",
            ),
            (
                LocalSocketControlError::ChangeUnsupported,
                "change_unsupported",
            ),
        ] {
            assert_eq!(
                serde_json::to_value(IpcError::from(error)).unwrap()["kind"],
                json!({ "domain": "local_socket", "error": { "kind": kind } })
            );
        }
        assert_eq!(
            wire(ProfilesError::ShuttingDown)["kind"],
            json!({ "domain": "profiles", "error": { "kind": "shutting_down" } })
        );
        assert_eq!(
            wire(ClientError::Custom("no domain".into()))["kind"],
            json!({ "domain": "unknown" })
        );
    }

    #[test]
    fn nested_domain_errors_are_serialized_and_library_sources_are_not() {
        let error = HttpServerControlError::ReadStatus {
            source: anyhow::anyhow!("read cause"),
        };
        let http_server = serde_json::to_value(IpcError::from(error)).unwrap();
        assert_eq!(
            http_server["kind"],
            json!({ "domain": "http_server", "error": { "kind": "read_status" } })
        );
        assert!(http_server["detail"].as_str().unwrap().contains("cause"));
        let local_error = LocalSocketControlError::ReadStatus {
            source: anyhow::anyhow!("socket read cause"),
        };
        let local_socket = serde_json::to_value(IpcError::from(local_error)).unwrap();
        assert_eq!(
            local_socket["kind"],
            json!({ "domain": "local_socket", "error": { "kind": "read_status" } })
        );
        assert!(
            local_socket["detail"]
                .as_str()
                .unwrap()
                .contains("socket read cause")
        );
        let fetch = wire(ProfilesError::FetchSubscription {
            url: "https://sub.example/x".parse().unwrap(),
            source: SubscriptionFetchError::SubscriptionHttpStatus { status: 404 },
        });
        assert_eq!(
            fetch["kind"]["error"],
            json!({
                "kind": "fetch_subscription",
                "url": "https://sub.example/x",
                "source": { "kind": "subscription_http_status", "status": 404 },
            })
        );

        let read = wire(ProfilesError::ReadProfileFile {
            uid: ProfileId("p1".into()),
            source: ProfileFileError::mock("disk full"),
        });
        assert_eq!(
            read["kind"]["error"],
            json!({
                "kind": "read_profile_file",
                "uid": "p1",
                "source": { "kind": "write_file", "path": "mock" },
            }),
            "the io error stays out of the wire form"
        );
        assert!(
            read["detail"].as_str().unwrap().contains("disk full"),
            "and reaches the user through the copied detail: {}",
            read["detail"]
        );
    }

    #[test]
    fn an_aborted_commit_names_the_write_failure() {
        let cause = ReplaceIfVersionError::WriteConfig(anyhow::anyhow!("disk full"));
        let aborted =
            CommitAborted::write_failed(Box::new(cause) as nyanpasu_contracts::BoxedCause);
        assert!(matches!(aborted, CommitAborted::WriteConfig { .. }));

        let wire = wire(ProfilesError::Commit {
            uid: Some(ProfileId("r1".into())),
            path: Some(ManagedProfilePath::new("r1.yaml").unwrap()),
            source: aborted,
            cleanup_failures: Vec::new(),
        });
        assert_eq!(
            wire["kind"]["error"],
            json!({
                "kind": "commit",
                "uid": "r1",
                "path": "r1.yaml",
                "source": {
                    "kind": "write_config",
                },
            })
        );
    }

    #[test]
    fn a_version_conflict_does_not_serialize_its_cleanup_failures() {
        let wire = wire(ProfilesError::VersionConflict {
            expected: 3,
            actual: 4,
            cleanup_failures: vec![ProfilesError::ShuttingDown],
            candidate_snapshot_id: None,
        });
        assert_eq!(
            wire["kind"]["error"],
            json!({ "kind": "version_conflict", "expected": 3, "actual": 4 })
        );
    }

    #[test]
    fn unclassified_error_serializes_message_and_original_error() {
        let error = anyhow::anyhow!("disk full").context("failed to save the profile");
        let wire = serde_json::to_value(IpcError::from(error)).unwrap();

        assert_eq!(wire["kind"], serde_json::json!({ "domain": "unknown" }));
        assert_eq!(wire["message"], "failed to save the profile");
        let detail = wire["detail"].as_str().unwrap();
        assert!(
            detail.contains("failed to save the profile") && detail.contains("disk full"),
            "detail keeps the whole source chain: {detail}"
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unsupported_engine_mutation_serializes_typed_ipc_envelope() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        use nyanpasu_config::{
            application::{ClashCore, NyanpasuAppConfig},
            profile::ScriptRuntime,
            runtime::{
                executor::{
                    PortError, ProfileContentSource, ScriptExecutionError, ScriptOperation,
                    ScriptRunner, StepLogEntry,
                },
                value::ConfigValue,
            },
        };
        use nyanpasu_core::client::{
            NyanpasuClient, runtime_build::RuntimeBuildService, test_support,
            test_support::TestControlEndpoint,
        };
        use struct_patch::Patch as _;

        struct Scripts(Arc<AtomicBool>);
        impl ScriptRunner for Scripts {
            fn run(
                &self,
                runtime: ScriptRuntime,
                _: &str,
                config: &ConfigValue,
                _: &mut Vec<StepLogEntry>,
            ) -> Result<ConfigValue, PortError> {
                if self.0.load(Ordering::SeqCst) {
                    Err(ScriptExecutionError::StartEngine {
                        runtime,
                        operation: ScriptOperation::Transform,
                    }
                    .into())
                } else {
                    Ok(config.clone())
                }
            }
            fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
                panic!("builtin transform")
            }
            fn eval_item_expr(&self, _: &str, _: &ConfigValue) -> Result<ConfigValue, PortError> {
                panic!("builtin transform")
            }
        }
        struct Content;
        impl ProfileContentSource for Content {
            fn read(&self, _: &ManagedProfilePath) -> Result<String, PortError> {
                Ok("proxies: []".into())
            }
        }

        for (core, runtime) in [
            (ClashCore::Mihomo, ScriptRuntime::JavaScript),
            (ClashCore::ClashRs, ScriptRuntime::Lua),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let endpoint = TestControlEndpoint::succeeding();
            let mut args =
                crate::test_support::test_client_args_with_endpoint(&dir, endpoint.clone()).await;
            let unavailable = Arc::new(AtomicBool::new(false));
            args.host_adapters.runtime_build = Arc::new(RuntimeBuildService::new(
                Arc::new(Content),
                Arc::new(test_support::runtime_core_spec),
                args.runtime_paths.clone(),
                {
                    let unavailable = unavailable.clone();
                    Arc::new(move || Ok(Box::new(Scripts(unavailable.clone()))))
                },
            ));
            let app_path = args.paths.application_config_path();
            let profiles_path = args.paths.profiles_path();
            let shutdown = args.shutdown.clone();
            let tasks = args.tasks.clone();
            let client = NyanpasuClient::try_new(args).await.unwrap();
            endpoint.prime(&client).await;
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.core = Some(core);
            patch.enable_builtin_enhanced = Some(true);
            client.patch_app_config(patch).await.unwrap();
            let uid = client
                .add_profile(
                    test_support::minimal_file_profile_request(),
                    Some("proxies: []".into()),
                )
                .await
                .unwrap()
                .into_value();
            let profile_path = client.get_profiles().await.unwrap().items[&uid]
                .definition
                .source()
                .unwrap()
                .materialized()
                .file
                .to_string();
            let before = client.configuration_status().source_versions;
            let app_bytes = std::fs::read(&app_path).ok();
            let profiles_bytes = std::fs::read(&profiles_path).ok();
            let submissions = endpoint.submissions();
            unavailable.store(true, Ordering::SeqCst);

            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.enable_service_mode = Some(true);
            let config_error = client.patch_app_config(patch).await.unwrap_err();
            let profile_error = client
                .activate_profile(Some(uid.clone()))
                .await
                .unwrap_err();
            for (domain, error) in [("config", config_error), ("profiles", profile_error)] {
                let message = error.to_string();
                let detail = format!("{error:?}");
                let wire = serde_json::to_value(IpcError::from(error)).unwrap();
                assert_eq!(wire["kind"]["domain"], domain);
                let source_error = &wire["kind"]["error"];
                assert_eq!(source_error["kind"], "commit");
                if domain == "config" {
                    assert_eq!(source_error["domain"], "application");
                } else {
                    assert_eq!(source_error["uid"], uid.to_string());
                    assert_eq!(source_error["path"], profile_path);
                }
                let commit = &source_error["source"];
                assert_eq!(commit["kind"], "runtime_refused");
                assert!(commit.get("runtime").is_none());
                // The steps before the unavailable engine are kept for inspection.
                let mut errors = commit["errors"].clone();
                let kept = errors[0]
                    .as_object_mut()
                    .unwrap()
                    .remove("candidate_snapshot_id")
                    .expect("the completed steps are kept");
                assert_eq!(kept.as_str().map(str::len), Some(21));
                assert_eq!(
                    errors,
                    serde_json::json!([{
                        "kind": "build_runtime", "source": {
                            "kind": "run_pipeline", "source": {
                                "kind": "execute_script", "source": {
                                    "kind": "start_engine", "runtime": runtime, "operation": "transform",
                                }
                            }
                        }
                    }])
                );
                assert_eq!(wire["message"], message);
                assert_eq!(wire["detail"], detail);
                assert!(detail.contains("StartEngine"));
                assert!(detail.contains("Transform"));
                assert!(detail.contains(&format!("{runtime:?}")));
            }
            let after = client.configuration_status().source_versions;
            assert_eq!(
                (after.application, after.clash, after.profiles),
                (before.application, before.clash, before.profiles)
            );
            assert_eq!(std::fs::read(&app_path).ok(), app_bytes);
            assert_eq!(std::fs::read(&profiles_path).ok(), profiles_bytes);
            assert_eq!(
                endpoint.submissions(),
                submissions,
                "failure precedes core application"
            );
            shutdown.cancel();
            tasks.close();
            tasks.wait().await;
        }
    }
}
