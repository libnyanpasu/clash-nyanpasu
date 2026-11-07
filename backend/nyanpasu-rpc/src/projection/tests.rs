#[path = "receiving_tests.rs"]
mod receiving_tests;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;

use super::IntoWire;

fn sample<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn check<T: Serialize + IntoWire>(value: T)
where
    T::Wire: Serialize + DeserializeOwned,
{
    let expected_bytes = serde_json::to_vec(&value).unwrap();
    let expected = serde_json::to_value(&value).unwrap();
    let projected = value.into_wire();
    assert_eq!(
        serde_json::to_vec(&projected).unwrap(),
        expected_bytes,
        "serialized bytes for {}",
        std::any::type_name::<T>()
    );
    assert_eq!(
        serde_json::to_value(&projected).unwrap(),
        expected,
        "{}",
        std::any::type_name::<T>()
    );
    let received: T::Wire = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(received).unwrap(), expected);
}

#[test]
fn wire_projections_match_application_serialization() {
    error_sources();
    request_defaults();
    storage_and_connection_details();
    for present in [false, true] {
        check(nyanpasu_core::client::agent_skills::CoreAgentSkill {
            name: "field-1/路徑\\file.yaml".to_owned(),
            dir: "field-2/路徑\\file.yaml".to_owned(),
            installed: present,
        });
        check(nyanpasu_core::client::profiles::ProfileUrlProbe {
            classification: import_classification(present),
            accepted: if present {
                vec![profile_import_kind(present)]
            } else {
                vec![]
            },
            suggested_name: "field-3/路徑\\file.yaml".to_owned(),
            subscription: sample(
                json!({"upload": 1, "download": 2, "total": 3, "expire": 1730000000}),
            ),
            suggested_update_interval_minutes: if present { Some(7) } else { None },
            content: "field-4/路徑\\file.yaml".to_owned(),
        });
        check(nyanpasu_core::client::jobs::ProfileSyncStatus {
            scheduled: present,
            next_run_at: if present {
                Some("field-5/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            active: vec![],
            journal_degraded: present,
            registration_error: if present {
                Some("field-6/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            history_limit: 2,
        });
        check(nyanpasu_core::profile_import::ImportClassification {
            suggested: if present {
                Some(profile_import_kind(present))
            } else {
                None
            },
            evidence: if present {
                vec![import_evidence(present)]
            } else {
                vec![]
            },
        });
        check(nyanpasu_core::profile_import::ProfileImportKind::Config);
        check(nyanpasu_core::profile_import::ProfileImportKind::Overlay);
        check(nyanpasu_core::profile_import::ProfileImportKind::JavaScript);
        check(nyanpasu_core::profile_import::ProfileImportKind::Lua);
        check(nyanpasu_core::profile_import::ImportSignal::SubscriptionQuota);
        check(nyanpasu_core::profile_import::ImportSignal::ProfileTitle);
        check(nyanpasu_core::profile_import::ImportSignal::UpdateInterval);
        check(nyanpasu_core::profile_import::ImportSignal::FileExtension);
        check(nyanpasu_core::profile_import::ImportSignal::ContentType);
        check(nyanpasu_core::profile_import::ImportSignal::ProxiesKey);
        check(nyanpasu_core::profile_import::ImportSignal::OverlayDirectives);
        check(nyanpasu_core::profile_import::ImportSignal::ScriptSyntax);
        check(nyanpasu_core::profile_import::ImportEvidence {
            kind: profile_import_kind(present),
            signal: import_signal(present),
        });
        check(
            nyanpasu_core::client::configuration_status::ConfigurationStatus {
                event_seq: 3,
                maintenance: if present {
                    Some("field-7/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
                source_versions: source_versions(present),
                runtime: runtime_convergence(present),
                effects: if present {
                    vec![effect_convergence(present)]
                } else {
                    vec![]
                },
                sources: if present {
                    vec![source_status(present)]
                } else {
                    vec![]
                },
                active: if present {
                    Some("field-8/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
                recent_operations: if present {
                    vec![operation_status(present)]
                } else {
                    vec![]
                },
            },
        );
        check(
            nyanpasu_core::client::configuration_status::SourceVersions {
                application: 4,
                clash: 5,
                session: 6,
                profiles: 7,
            },
        );
        check(
            nyanpasu_core::client::configuration_status::RuntimeConvergence {
                health: convergence_health(present),
                operation_id: if present {
                    Some("field-9/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
                attempts: 8,
                automatic_remaining: 9,
                message: if present {
                    Some("field-10/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
            },
        );
        check(
            nyanpasu_core::client::configuration_status::EffectConvergence {
                kind: effect_kind(present),
                health: convergence_health(present),
                desired_revision: 10,
                applied_revision: 11,
                attempts: 12,
                automatic_remaining: 13,
                code: if present {
                    Some(effect_failure_code(present))
                } else {
                    None
                },
                message: if present {
                    Some("field-11/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
            },
        );
        check(
            nyanpasu_core::client::configuration_status::OperationStatus {
                operation_id: "field-12/路徑\\file.yaml".to_owned(),
                domain: "field-13/路徑\\file.yaml".to_owned(),
                outcome: "field-14/路徑\\file.yaml".to_owned(),
                conclusion: "field-15/路徑\\file.yaml".to_owned(),
                message: if present {
                    Some("field-16/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
            },
        );
        check(nyanpasu_core::client::convergence::ConvergenceHealth::Healthy);
        check(nyanpasu_core::client::convergence::ConvergenceHealth::Pending);
        check(nyanpasu_core::client::convergence::ConvergenceHealth::RetryScheduled);
        check(nyanpasu_core::client::convergence::ConvergenceHealth::WaitingDependency);
        check(nyanpasu_core::client::convergence::ConvergenceHealth::Blocked);
        check(nyanpasu_core::client::convergence::ConvergenceHealth::RecoveryRequired);
        check(nyanpasu_core::client::effects::plan::EffectKind::Locale);
        check(nyanpasu_core::client::effects::plan::EffectKind::Logger);
        check(nyanpasu_core::client::effects::plan::EffectKind::CoreLogLevel);
        check(nyanpasu_core::client::effects::plan::EffectKind::CoreLogStorage);
        check(nyanpasu_core::client::effects::plan::EffectKind::AutoLaunch);
        check(nyanpasu_core::client::effects::plan::EffectKind::SystemProxy);
        check(nyanpasu_core::client::effects::plan::EffectKind::ProxyGuard);
        check(nyanpasu_core::client::effects::plan::EffectKind::Hotkeys);
        check(nyanpasu_core::client::effects::plan::EffectKind::Widget);
        check(nyanpasu_core::client::effects::plan::EffectKind::WidgetLogo);
        check(nyanpasu_core::client::effects::plan::EffectKind::Tray);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::PresentationUnsupported);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::HotkeyInvalidBindings);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::HotkeyPartialRegistration);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::HotkeyShutDown);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::HotkeyStopped);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::LoggerRefreshFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::CoreLogStorageFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::WidgetUnavailable);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::WidgetApplyFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::TrayRefreshFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::EffectOwnerSilent);
        check(
            nyanpasu_core::client::effects::status::EffectFailureCode::ProxyGuardWaitingDependency,
        );
        check(nyanpasu_core::client::effects::status::EffectFailureCode::AutoLaunchFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::PacDisableFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::PacApplyFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::PacUnsupported);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::SystemProxyApplyFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::SystemProxyPortUnresolved);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::SystemProxyRestoreFailed);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::SystemProxyShutDown);
        check(nyanpasu_core::client::effects::status::EffectFailureCode::SystemProxyStopped);
        check(nyanpasu_core::state::profiles::sources::SourceOrigin::ScheduledRefresh);
        check(nyanpasu_core::state::profiles::sources::SourceOrigin::ManualRefresh);
        check(nyanpasu_core::state::profiles::sources::SourceOrigin::ExternalFile);
        check(
            nyanpasu_core::state::profiles::sources::SourceOutcome::Committed {
                operation_id: if present {
                    Some("field-17/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
            },
        );
        check(
            nyanpasu_core::state::profiles::sources::SourceOutcome::Superseded {
                reason: "field-18/路徑\\file.yaml".to_owned(),
            },
        );
        check(
            nyanpasu_core::state::profiles::sources::SourceOutcome::Failed {
                message: "field-19/路徑\\file.yaml".to_owned(),
            },
        );
        check(
            nyanpasu_core::state::profiles::sources::SourceOutcome::Rejected {
                code: "field-20/路徑\\file.yaml".to_owned(),
                message: "field-21/路徑\\file.yaml".to_owned(),
            },
        );
        check(nyanpasu_core::state::profiles::sources::SourceStatus {
            profile: sample(json!("profile-1")),
            name: "field-22/路徑\\file.yaml".to_owned(),
            origin: source_origin(present),
            outcome: source_outcome(present),
            health: convergence_health(present),
            at: 14,
        });
        check(nyanpasu_core::client::debug_http::DebugHttpStatus {
            enabled: present,
            url: if present {
                Some("field-23/路徑\\file.yaml".to_owned())
            } else {
                None
            },
        });
        check(nyanpasu_core::client::clash_info::ClashInfo {
            port: 15,
            server: "field-24/路徑\\file.yaml".to_owned(),
            secret: if present {
                Some("field-25/路徑\\file.yaml".to_owned())
            } else {
                None
            },
        });
        check(
            nyanpasu_core::client::runtime_inspection::RuntimeInspection {
                snapshot_id: "field-26/路徑\\file.yaml".to_owned(),
                applied: present,
                effective_pending: present,
                effective_revision: if present {
                    Some(sample(
                        json!({"epoch": 1, "generation": 2, "source_hash": "source", "effective_hash": "effective"}),
                    ))
                } else {
                    None
                },
                revision: "field-27/路徑\\file.yaml".to_owned(),
                target_core: "field-28/路徑\\file.yaml".to_owned(),
                root_id: 16,
                final_id: 17,
                nodes: if present {
                    vec![runtime_inspection_node(present)]
                } else {
                    vec![]
                },
            },
        );
        check(
            nyanpasu_core::client::runtime_inspection::RuntimeInspectionNode {
                id: 18,
                tag: nyanpasu_config::runtime::snapshot::OperatorTag::BareRoot,
                next: if present { vec![7] } else { vec![] },
                has_logs: present,
                max_log_level: if present {
                    Some(sample(json!("warn")))
                } else {
                    None
                },
                changed_fields: if present {
                    Some(if present {
                        vec!["field-29/路徑\\file.yaml".to_owned()]
                    } else {
                        vec![]
                    })
                } else {
                    None
                },
            },
        );
        check(
            nyanpasu_core::client::runtime_inspection::RuntimeInspectionContent {
                yaml: "field-30/路徑\\file.yaml".to_owned(),
                diff: if present {
                    Some(runtime_inspection_diff(present))
                } else {
                    None
                },
                logs: if present {
                    vec![sample(json!({"level": "warn", "message": "step log"}))]
                } else {
                    vec![]
                },
            },
        );
        check(
            nyanpasu_core::client::runtime_inspection::RuntimeInspectionDiff {
                parent_id: 19,
                hunks: if present {
                    vec![sample(
                        json!({"old_start": 1, "old_lines": 1, "new_start": 1, "new_lines": 2, "lines": [" line", "+added"]}),
                    )]
                } else {
                    vec![]
                },
            },
        );
        check(nyanpasu_core::client::direct_egress::DirectEgress::Disabled);
        check(nyanpasu_core::client::direct_egress::DirectEgress::TunEnabled);
        check(nyanpasu_core::client::direct_egress::DirectEgress::Probed {
            ipv4: if present {
                Some("192.0.2.1".parse().unwrap())
            } else {
                None
            },
            ipv6: if present {
                Some("2001:db8::1".parse().unwrap())
            } else {
                None
            },
        });
        check(nyanpasu_core::actor_v2::EndpointConnectivity::Connected);
        check(nyanpasu_core::actor_v2::EndpointConnectivity::ShutDown);
        check(nyanpasu_core::actor_v2::EndpointConnectivity::HandingOff {
            from: sample(json!(if present { "service" } else { "local" })),
            to: sample(json!(if present { "service" } else { "local" })),
        });
        check(nyanpasu_core::actor_v2::EndpointConnectivity::Degraded {
            desired: sample(json!(if present { "service" } else { "local" })),
            reason: "field-31/路徑\\file.yaml".to_owned(),
        });
        check(nyanpasu_core::actor_v2::CoreStatusInfo {
            controller: if present {
                Some(sample(json!({"Http": "http://127.0.0.1:9090"})))
            } else {
                None
            },
            host: sample(json!(if present { "service" } else { "local" })),
            connectivity: endpoint_connectivity(present),
            generation: 20,
            state: if present {
                Some(sample(json!({"Running": {"epoch": 1, "pid": 42}})))
            } else {
                None
            },
            state_changed_at: 21,
            revision: if present {
                Some(sample(
                    json!({"epoch": 1, "generation": 2, "effective_hash": "effective"}),
                ))
            } else {
                None
            },
            healthy: if present { Some(present) } else { None },
        });
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Probing);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::NotInstalled);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::DaemonStopped);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Installing);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::StartingDaemon);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Ready);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Incompatible);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Restarting);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Exhausted);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Uninstalling);
        check(nyanpasu_core::actor_v2::service_actor::ServicePhase::Unknown);
        check(nyanpasu_core::service::compat::ServiceCompat::Unknown);
        check(nyanpasu_core::service::compat::ServiceCompat::Compatible {
            server_version: "field-32/路徑\\file.yaml".to_owned(),
        });
        check(
            nyanpasu_core::service::compat::ServiceCompat::Incompatible {
                server_version: "field-33/路徑\\file.yaml".to_owned(),
                required_major: 22,
                required_min: "field-34/路徑\\file.yaml".to_owned(),
            },
        );
        check(nyanpasu_core::service::compat::ServiceCompat::Unparsable {
            server_version: "field-35/路徑\\file.yaml".to_owned(),
        });
        check(nyanpasu_core::actor_v2::service_actor::ServiceHostStatus {
            name: "service-name".into(),
            version: "service-version".into(),
            status: nyanpasu_ipc::types::ServiceStatus::Running,
            server: if present {
                Some(sample(
                    json!({"version": "2.0.0", "core_infos": {"type": null, "state": "Running", "state_changed_at": 1, "config_path": null}, "runtime_infos": {"service_data_dir": "service/data", "service_config_dir": "service/config", "nyanpasu_config_dir": "app/config", "nyanpasu_data_dir": "app/data"}}),
                ))
            } else {
                None
            },
            phase: service_phase(present),
            compat: service_compat(present),
            restart_attempts: 23,
            settled_ready: present,
            ready_generation: 9,
        });
        check(nyanpasu_core::clash::api::ClashConfig {
            port: if present { Some(7) } else { None },
            mode: if present {
                Some("field-36/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            ipv6: if present { Some(present) } else { None },
            socket_port: if present { Some(7) } else { None },
            allow_lan: if present { Some(present) } else { None },
            log_level: if present {
                Some("field-37/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            mixed_port: if present { Some(7) } else { None },
            redir_port: if present { Some(7) } else { None },
            socks_port: if present { Some(7) } else { None },
            tproxy_port: if present { Some(7) } else { None },
            external_controller: if present {
                Some("field-38/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            secret: if present {
                Some("field-39/路徑\\file.yaml".to_owned())
            } else {
                None
            },
        });
        check(nyanpasu_core::clash::api::ClashVersion {
            version: "field-40/路徑\\file.yaml".to_owned(),
            premium: if present { Some(present) } else { None },
            meta: if present { Some(present) } else { None },
        });
        check(nyanpasu_core::clash::api::ClashRule {
            r#type: "field-41/路徑\\file.yaml".to_owned(),
            payload: "field-42/路徑\\file.yaml".to_owned(),
            proxy: "field-43/路徑\\file.yaml".to_owned(),
        });
        check(nyanpasu_core::clash::api::RulesRes {
            rules: if present {
                vec![clash_rule(present)]
            } else {
                vec![]
            },
        });
        check(nyanpasu_core::clash::api::RuleProviderItem {
            behavior: if present {
                Some("field-44/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            format: if present {
                Some("field-45/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            name: "field-46/路徑\\file.yaml".to_owned(),
            rule_count: if present { Some(7) } else { None },
            r#type: if present {
                Some("field-47/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            updated_at: if present {
                Some("field-48/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            vehicle_type: if present {
                Some("field-49/路徑\\file.yaml".to_owned())
            } else {
                None
            },
        });
        check(nyanpasu_core::clash::api::ProvidersRulesRes {
            providers: if present {
                [(
                    "field-50/路徑\\file.yaml".to_owned(),
                    rule_provider_item(present),
                )]
                .into()
            } else {
                Default::default()
            },
        });
        check(nyanpasu_core::clash::proxies::ProxyProviderSummary {
            name: sample(json!("Provider A")),
            provider_type: clash_api::ProviderType::Proxy,
            vehicle_type: clash_api::VehicleType::Http,
            updated_at: if present {
                Some("2026-11-07T12:30:00+08:00".parse().unwrap())
            } else {
                None
            },
            subscription_info: if present {
                Some(sample(
                    json!({"upload": 1, "download": 2, "total": 3, "expire": 1730000000}),
                ))
            } else {
                None
            },
            proxy_count: 24,
        });
        check(nyanpasu_core::clash::proxies::ProxyGroupKind::Selector);
        check(nyanpasu_core::clash::proxies::ProxyGroupKind::UrlTest);
        check(nyanpasu_core::clash::proxies::ProxyGroupKind::Fallback);
        check(nyanpasu_core::clash::proxies::ProxyGroupKind::LoadBalance);
        check(nyanpasu_core::clash::proxies::ProxyGroupKind::Relay);
        check(nyanpasu_core::clash::proxies::ProxyGroupKind::Smart);
        check(nyanpasu_core::clash::proxies::ProxyGroupKind::Unknown(
            "field-51/路徑\\file.yaml".to_owned(),
        ));
        check(nyanpasu_core::clash::proxies::ProxyGroupCapabilities {
            select: present,
            clear_fixed: present,
        });
        check(nyanpasu_core::clash::proxies::ProxyGroup {
            name: sample(json!("Proxy A")),
            kind: proxy_group_kind(present),
            all: if present {
                vec![sample(json!("Proxy A"))]
            } else {
                vec![]
            },
            now: if present {
                Some(sample(json!("Proxy A")))
            } else {
                None
            },
            fixed: if present {
                Some(sample(json!("Proxy A")))
            } else {
                None
            },
            test_url: if present {
                Some("field-52/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            expected_status: if present {
                Some("field-53/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            hidden: present,
            icon: if present {
                Some("field-54/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            capabilities: proxy_group_capabilities(present),
        });
        check(nyanpasu_core::clash::proxies::Proxies {
            global: if present {
                Some(proxy_group(present))
            } else {
                None
            },
            groups: if present {
                vec![proxy_group(present)]
            } else {
                vec![]
            },
            nodes: if present {
                [(
                    sample(json!("Proxy A")),
                    sample(json!({"name": "node", "type": "Direct", "udp": true, "history": []})),
                )]
                .into()
            } else {
                Default::default()
            },
        });
        check(nyanpasu_core::clash::connection_rates::TrafficRate {
            download: 25,
            upload: 26,
        });
        check(
            nyanpasu_core::clash::connection_rates::ClashConnectionsSummary {
                download_total: 27,
                upload_total: 28,
                download_speed: 29,
                upload_speed: 30,
                memory: if present { Some(7) } else { None },
                connection_count: 31,
                member_rates: if present {
                    [("field-55/路徑\\file.yaml".to_owned(), traffic_rate(present))].into()
                } else {
                    Default::default()
                },
            },
        );
        check(nyanpasu_core::clash::ws::ClashConnectionsInfo {
            download_total: 32,
            upload_total: 33,
            download_speed: 34,
            upload_speed: 35,
        });
        check(
            nyanpasu_core::clash::ws::ClashConnectionsConnectorEvent::StateChanged(
                clash_connections_connector_state(present),
            ),
        );
        check(
            nyanpasu_core::clash::ws::ClashConnectionsConnectorEvent::Update(
                clash_connections_info(present),
            ),
        );
        check(nyanpasu_core::clash::ws::ClashConnectionsConnectorState::Disconnected);
        check(nyanpasu_core::clash::ws::ClashConnectionsConnectorState::Connecting);
        check(nyanpasu_core::clash::ws::ClashConnectionsConnectorState::Connected);
        check(nyanpasu_core::clash::ws::ClashWsKind::Connections);
        check(nyanpasu_core::clash::ws::ClashWsKind::Logs);
        check(nyanpasu_core::clash::ws::ClashWsKind::Traffic);
        check(nyanpasu_core::clash::ws::ClashWsKind::Memory);
        check(nyanpasu_core::clash::ws::ClashWsRecording {
            connections: present,
            logs: present,
            traffic: present,
            memory: present,
        });
        check(nyanpasu_core::clash::ws::ClashWsMemory {
            inuse: 36,
            oslimit: 37,
        });
        check(nyanpasu_core::clash::ws::ClashWsTraffic { up: 38, down: 7 });
        check(nyanpasu_core::clash::ws::ClashWsSnapshot {
            sequence: 39,
            state: clash_connections_connector_state(present),
            recording: clash_ws_recording(present),
            connections: if present {
                vec![clash_connections_summary(present)]
            } else {
                vec![]
            },
            traffic: if present {
                vec![clash_ws_traffic(present)]
            } else {
                vec![]
            },
            memory: if present {
                vec![clash_ws_memory(present)]
            } else {
                vec![]
            },
        });
        check(nyanpasu_core::clash::ws::ClashWsPacket {
            sequence: 40,
            update: clash_ws_update(present),
        });
        check(nyanpasu_core::clash::ws::ClashWsUpdate::Reset(Box::new(
            clash_ws_snapshot(present),
        )));
        check(nyanpasu_core::clash::ws::ClashWsUpdate::StateChanged(
            clash_connections_connector_state(present),
        ));
        check(nyanpasu_core::clash::ws::ClashWsUpdate::ConnectionsUpdated(
            clash_connections_summary(present),
        ));
        check(nyanpasu_core::clash::ws::ClashWsUpdate::TrafficUpdated(
            clash_ws_traffic(present),
        ));
        check(nyanpasu_core::clash::ws::ClashWsUpdate::MemoryUpdated(
            clash_ws_memory(present),
        ));
        check(nyanpasu_core::clash::ws::ClashWsUpdate::RecordingChanged(
            clash_ws_recording(present),
        ));
        check(nyanpasu_core::clash::ws::ClashWsUpdate::HistoryCleared(
            clash_ws_kind(present),
        ));
        check(nyanpasu_core::client::logs::LogSource::App);
        check(nyanpasu_core::client::logs::LogSource::Service);
        check(nyanpasu_core::client::frontend_events::FrontendEventKind::Console);
        check(nyanpasu_core::client::frontend_events::FrontendEventKind::UncaughtError);
        check(nyanpasu_core::client::frontend_events::FrontendEventKind::UnhandledRejection);
        check(nyanpasu_core::client::frontend_events::FrontendEventKind::ReactUncaught);
        check(nyanpasu_core::client::frontend_events::FrontendEventKind::ReactCaught);
        check(nyanpasu_core::client::frontend_events::FrontendEventKind::ReactRecoverable);
        check(nyanpasu_core::client::frontend_events::FrontendEventLevel::Warning);
        check(nyanpasu_core::client::frontend_events::FrontendEventLevel::Error);
        check(nyanpasu_core::client::frontend_events::FrontendErrorCause {
            name: if present {
                Some("field-56/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            message: "field-57/路徑\\file.yaml".to_owned(),
            stack: if present {
                Some("field-58/路徑\\file.yaml".to_owned())
            } else {
                None
            },
        });
        check(nyanpasu_core::client::frontend_events::FrontendEvent {
            kind: frontend_event_kind(present),
            level: frontend_event_level(present),
            message: "field-59/路徑\\file.yaml".to_owned(),
            error_name: if present {
                Some("field-60/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            stack: if present {
                Some("field-61/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            causes: if present {
                vec![frontend_error_cause(present)]
            } else {
                vec![]
            },
            component_stack: if present {
                Some("field-62/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            fingerprint: "field-63/路徑\\file.yaml".to_owned(),
            count: 41,
            first_seen_ms: if present { Some(1.25) } else { None },
            last_seen_ms: if present { Some(1.25) } else { None },
            route: "field-64/路徑\\file.yaml".to_owned(),
        });
        check(nyanpasu_core::client::frontend_events::FrontendEventBatch {
            events: if present {
                vec![frontend_event(present)]
            } else {
                vec![]
            },
            dropped: 42,
        });
        check(nyanpasu_observers::logs::CoreLogError::InvalidRequest);
        check(nyanpasu_observers::logs::CoreLogError::CursorExpired);
        check(nyanpasu_observers::logs::CoreLogError::RecordGone);
        check(nyanpasu_observers::logs::CoreLogError::TooLarge);
        check(nyanpasu_observers::logs::CoreLogError::Unavailable(
            "field-65/路徑\\file.yaml".to_owned(),
        ));
        check(nyanpasu_observers::logs::CoreLogCursor {
            generation: "field-66/路徑\\file.yaml".to_owned(),
            sequence: 43,
        });
        check(nyanpasu_observers::logs::CoreLogSource {
            capture: "field-67/路徑\\file.yaml".to_owned(),
            instance_id: "field-68/路徑\\file.yaml".to_owned(),
            core_kind: if present {
                Some("field-69/路徑\\file.yaml".to_owned())
            } else {
                None
            },
        });
        check(nyanpasu_observers::logs::CoreLogRecord {
            source: core_log_source(present),
            received_at: 44,
            time: if present {
                Some("field-70/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            log_type: "field-71/路徑\\file.yaml".to_owned(),
            payload: "field-72/路徑\\file.yaml".to_owned(),
        });
        check(nyanpasu_observers::logs::CoreLogRow {
            id: core_log_cursor(present),
            record: core_log_record(present),
            truncated: present,
        });
        check(nyanpasu_observers::logs::CoreLogDirection::Latest);
        check(nyanpasu_observers::logs::CoreLogDirection::Before);
        check(nyanpasu_observers::logs::CoreLogDirection::After);
        check(nyanpasu_observers::logs::CoreLogQuery {
            direction: core_log_direction(present),
            cursor: if present {
                Some(core_log_cursor(present))
            } else {
                None
            },
            level: if present {
                Some("field-73/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            keyword: "field-74/路徑\\file.yaml".to_owned(),
            limit: 45,
        });
        check(nyanpasu_observers::logs::CoreLogStatus {
            generation: "field-75/路徑\\file.yaml".to_owned(),
            version: 46,
            first: if present {
                Some(core_log_cursor(present))
            } else {
                None
            },
            head: if present {
                Some(core_log_cursor(present))
            } else {
                None
            },
            bytes: 47,
            error: if present {
                Some("field-76/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            discarded: 48,
        });
        check(nyanpasu_observers::logs::CoreLogPage {
            rows: if present {
                vec![core_log_row(present)]
            } else {
                vec![]
            },
            cursor: if present {
                Some(core_log_cursor(present))
            } else {
                None
            },
            more: present,
            status: core_log_status(present),
        });
        check(nyanpasu_updater::ManifestVersionLatest {
            mihomo: "field-77/路徑\\file.yaml".to_owned(),
            mihomo_alpha: "field-78/路徑\\file.yaml".to_owned(),
            clash_rs: "field-79/路徑\\file.yaml".to_owned(),
            clash_rs_alpha: "field-80/路徑\\file.yaml".to_owned(),
            clash_premium: "field-81/路徑\\file.yaml".to_owned(),
            meow: "field-82/路徑\\file.yaml".to_owned(),
            meow_alpha: "field-83/路徑\\file.yaml".to_owned(),
        });
        check(nyanpasu_updater::UpdaterSummary {
            id: 49,
            state: updater_state(present),
            downloader: download_status(present),
        });
        check(nyanpasu_updater::UpdaterState::Idle);
        check(nyanpasu_updater::UpdaterState::Downloading);
        check(nyanpasu_updater::UpdaterState::Decompressing);
        check(nyanpasu_updater::UpdaterState::Replacing);
        check(nyanpasu_updater::UpdaterState::Restarting);
        check(nyanpasu_updater::UpdaterState::Done);
        check(nyanpasu_updater::UpdaterState::Failed(
            "field-84/路徑\\file.yaml".to_owned(),
        ));
        check(nyanpasu_updater::DownloadStatus {
            state: downloader_state(present),
            downloaded: 50,
            total: 51,
            speed: 1.25,
        });
        check(nyanpasu_updater::DownloaderState::Idle);
        check(nyanpasu_updater::DownloaderState::Downloading);
        check(nyanpasu_updater::DownloaderState::Failed(
            "field-85/路徑\\file.yaml".to_owned(),
        ));
        check(nyanpasu_updater::DownloaderState::Finished);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Idle);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Checking);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::UpToDate);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Available);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Downloading);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Cancelling);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Cancelled);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Verifying);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Ready);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Installing);
        check(nyanpasu_core::client::app_update::AppUpdatePhase::Failed);
        check(nyanpasu_core::client::app_update::AppUpdateRelease {
            version: "field-86/路徑\\file.yaml".to_owned(),
            date: if present {
                Some("field-87/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            body: if present {
                Some("field-88/路徑\\file.yaml".to_owned())
            } else {
                None
            },
        });
        check(nyanpasu_core::client::app_update::AppUpdateSnapshot {
            revision: 52,
            phase: app_update_phase(present),
            release: if present {
                Some(app_update_release(present))
            } else {
                None
            },
            downloaded: 53,
            total: if present { Some(7) } else { None },
            speed: 1.25,
            source: if present {
                Some(sample(json!("github")))
            } else {
                None
            },
            error: if present {
                Some("field-89/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            last_checked_at: if present {
                Some("field-90/路徑\\file.yaml".to_owned())
            } else {
                None
            },
            supported: present,
            endpoints: if present {
                vec!["field-91/路徑\\file.yaml".to_owned()]
            } else {
                vec![]
            },
        });
        check(nyanpasu_core::client::app_update::changelog::AppUpdateChangelogSource::Changelog);
        check(nyanpasu_core::client::app_update::changelog::AppUpdateChangelogSource::Release);
        check(
            nyanpasu_core::client::app_update::changelog::AppUpdateChangelogEntry {
                version: "field-92/路徑\\file.yaml".to_owned(),
                date: if present {
                    Some("field-93/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
                body: "field-94/路徑\\file.yaml".to_owned(),
                source: app_update_changelog_source(present),
            },
        );
        check(
            nyanpasu_core::client::app_update::changelog::AppUpdateChangelog {
                entries: if present {
                    vec![app_update_changelog_entry(present)]
                } else {
                    vec![]
                },
                compare_url: if present {
                    Some("field-95/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
            },
        );
        check(nyanpasu_core::client::StateChanged::NyanpasuConfig);
        check(nyanpasu_core::client::StateChanged::ClashConfig);
        check(nyanpasu_core::client::StateChanged::Profiles {
            files: if present {
                vec![sample(json!("profile-1"))]
            } else {
                vec![]
            },
        });
        check(nyanpasu_core::client::StateChanged::Proxies);
    }
}

fn import_classification(present: bool) -> nyanpasu_core::profile_import::ImportClassification {
    let _ = present;
    nyanpasu_core::profile_import::ImportClassification {
        suggested: if present {
            Some(profile_import_kind(present))
        } else {
            None
        },
        evidence: if present {
            vec![import_evidence(present)]
        } else {
            vec![]
        },
    }
}

fn profile_import_kind(present: bool) -> nyanpasu_core::profile_import::ProfileImportKind {
    let _ = present;
    nyanpasu_core::profile_import::ProfileImportKind::Config
}

fn import_signal(present: bool) -> nyanpasu_core::profile_import::ImportSignal {
    let _ = present;
    nyanpasu_core::profile_import::ImportSignal::SubscriptionQuota
}

fn import_evidence(present: bool) -> nyanpasu_core::profile_import::ImportEvidence {
    let _ = present;
    nyanpasu_core::profile_import::ImportEvidence {
        kind: profile_import_kind(present),
        signal: import_signal(present),
    }
}

fn source_versions(present: bool) -> nyanpasu_core::client::configuration_status::SourceVersions {
    let _ = present;
    nyanpasu_core::client::configuration_status::SourceVersions {
        application: 54,
        clash: 55,
        session: 56,
        profiles: 57,
    }
}

fn runtime_convergence(
    present: bool,
) -> nyanpasu_core::client::configuration_status::RuntimeConvergence {
    let _ = present;
    nyanpasu_core::client::configuration_status::RuntimeConvergence {
        health: convergence_health(present),
        operation_id: if present {
            Some("field-96/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        attempts: 58,
        automatic_remaining: 59,
        message: if present {
            Some("field-97/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn effect_convergence(
    present: bool,
) -> nyanpasu_core::client::configuration_status::EffectConvergence {
    let _ = present;
    nyanpasu_core::client::configuration_status::EffectConvergence {
        kind: effect_kind(present),
        health: convergence_health(present),
        desired_revision: 60,
        applied_revision: 61,
        attempts: 62,
        automatic_remaining: 63,
        code: if present {
            Some(effect_failure_code(present))
        } else {
            None
        },
        message: if present {
            Some("field-98/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn operation_status(present: bool) -> nyanpasu_core::client::configuration_status::OperationStatus {
    let _ = present;
    nyanpasu_core::client::configuration_status::OperationStatus {
        operation_id: "field-99/路徑\\file.yaml".to_owned(),
        domain: "field-100/路徑\\file.yaml".to_owned(),
        outcome: "field-101/路徑\\file.yaml".to_owned(),
        conclusion: "field-102/路徑\\file.yaml".to_owned(),
        message: if present {
            Some("field-103/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn convergence_health(present: bool) -> nyanpasu_core::client::convergence::ConvergenceHealth {
    let _ = present;
    nyanpasu_core::client::convergence::ConvergenceHealth::Healthy
}

fn effect_kind(present: bool) -> nyanpasu_core::client::effects::plan::EffectKind {
    let _ = present;
    nyanpasu_core::client::effects::plan::EffectKind::Locale
}

fn effect_failure_code(present: bool) -> nyanpasu_core::client::effects::status::EffectFailureCode {
    let _ = present;
    nyanpasu_core::client::effects::status::EffectFailureCode::PresentationUnsupported
}

fn source_origin(present: bool) -> nyanpasu_core::state::profiles::sources::SourceOrigin {
    let _ = present;
    nyanpasu_core::state::profiles::sources::SourceOrigin::ScheduledRefresh
}

fn source_outcome(present: bool) -> nyanpasu_core::state::profiles::sources::SourceOutcome {
    let _ = present;
    nyanpasu_core::state::profiles::sources::SourceOutcome::Committed {
        operation_id: if present {
            Some("field-104/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn source_status(present: bool) -> nyanpasu_core::state::profiles::sources::SourceStatus {
    let _ = present;
    nyanpasu_core::state::profiles::sources::SourceStatus {
        profile: sample(json!("profile-1")),
        name: "field-105/路徑\\file.yaml".to_owned(),
        origin: source_origin(present),
        outcome: source_outcome(present),
        health: convergence_health(present),
        at: 64,
    }
}

fn runtime_inspection_node(
    present: bool,
) -> nyanpasu_core::client::runtime_inspection::RuntimeInspectionNode {
    let _ = present;
    nyanpasu_core::client::runtime_inspection::RuntimeInspectionNode {
        id: 65,
        tag: nyanpasu_config::runtime::snapshot::OperatorTag::BareRoot,
        next: if present { vec![7] } else { vec![] },
        has_logs: present,
        max_log_level: if present {
            Some(sample(json!("warn")))
        } else {
            None
        },
        changed_fields: if present {
            Some(if present {
                vec!["field-106/路徑\\file.yaml".to_owned()]
            } else {
                vec![]
            })
        } else {
            None
        },
    }
}

fn runtime_inspection_diff(
    present: bool,
) -> nyanpasu_core::client::runtime_inspection::RuntimeInspectionDiff {
    let _ = present;
    nyanpasu_core::client::runtime_inspection::RuntimeInspectionDiff {
        parent_id: 66,
        hunks: if present {
            vec![sample(
                json!({"old_start": 1, "old_lines": 1, "new_start": 1, "new_lines": 2, "lines": [" line", "+added"]}),
            )]
        } else {
            vec![]
        },
    }
}

fn endpoint_connectivity(present: bool) -> nyanpasu_core::actor_v2::EndpointConnectivity {
    let _ = present;
    nyanpasu_core::actor_v2::EndpointConnectivity::Connected
}

fn service_phase(present: bool) -> nyanpasu_core::actor_v2::service_actor::ServicePhase {
    let _ = present;
    nyanpasu_core::actor_v2::service_actor::ServicePhase::Probing
}

fn service_compat(present: bool) -> nyanpasu_core::service::compat::ServiceCompat {
    let _ = present;
    nyanpasu_core::service::compat::ServiceCompat::Unknown
}

fn clash_rule(present: bool) -> nyanpasu_core::clash::api::ClashRule {
    let _ = present;
    nyanpasu_core::clash::api::ClashRule {
        r#type: "field-107/路徑\\file.yaml".to_owned(),
        payload: "field-108/路徑\\file.yaml".to_owned(),
        proxy: "field-109/路徑\\file.yaml".to_owned(),
    }
}

fn rule_provider_item(present: bool) -> nyanpasu_core::clash::api::RuleProviderItem {
    let _ = present;
    nyanpasu_core::clash::api::RuleProviderItem {
        behavior: if present {
            Some("field-110/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        format: if present {
            Some("field-111/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        name: "field-112/路徑\\file.yaml".to_owned(),
        rule_count: if present { Some(7) } else { None },
        r#type: if present {
            Some("field-113/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        updated_at: if present {
            Some("field-114/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        vehicle_type: if present {
            Some("field-115/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn proxy_group_kind(present: bool) -> nyanpasu_core::clash::proxies::ProxyGroupKind {
    let _ = present;
    nyanpasu_core::clash::proxies::ProxyGroupKind::Selector
}

fn proxy_group_capabilities(
    present: bool,
) -> nyanpasu_core::clash::proxies::ProxyGroupCapabilities {
    let _ = present;
    nyanpasu_core::clash::proxies::ProxyGroupCapabilities {
        select: present,
        clear_fixed: present,
    }
}

fn proxy_group(present: bool) -> nyanpasu_core::clash::proxies::ProxyGroup {
    let _ = present;
    nyanpasu_core::clash::proxies::ProxyGroup {
        name: sample(json!("Proxy A")),
        kind: proxy_group_kind(present),
        all: if present {
            vec![sample(json!("Proxy A"))]
        } else {
            vec![]
        },
        now: if present {
            Some(sample(json!("Proxy A")))
        } else {
            None
        },
        fixed: if present {
            Some(sample(json!("Proxy A")))
        } else {
            None
        },
        test_url: if present {
            Some("field-116/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        expected_status: if present {
            Some("field-117/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        hidden: present,
        icon: if present {
            Some("field-118/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        capabilities: proxy_group_capabilities(present),
    }
}

fn traffic_rate(present: bool) -> nyanpasu_core::clash::connection_rates::TrafficRate {
    let _ = present;
    nyanpasu_core::clash::connection_rates::TrafficRate {
        download: 67,
        upload: 68,
    }
}

fn clash_connections_summary(
    present: bool,
) -> nyanpasu_core::clash::connection_rates::ClashConnectionsSummary {
    let _ = present;
    nyanpasu_core::clash::connection_rates::ClashConnectionsSummary {
        download_total: 69,
        upload_total: 70,
        download_speed: 71,
        upload_speed: 72,
        memory: if present { Some(7) } else { None },
        connection_count: 73,
        member_rates: if present {
            [(
                "field-119/路徑\\file.yaml".to_owned(),
                traffic_rate(present),
            )]
            .into()
        } else {
            Default::default()
        },
    }
}

fn clash_connections_info(present: bool) -> nyanpasu_core::clash::ws::ClashConnectionsInfo {
    let _ = present;
    nyanpasu_core::clash::ws::ClashConnectionsInfo {
        download_total: 74,
        upload_total: 75,
        download_speed: 76,
        upload_speed: 77,
    }
}

fn clash_connections_connector_state(
    present: bool,
) -> nyanpasu_core::clash::ws::ClashConnectionsConnectorState {
    let _ = present;
    nyanpasu_core::clash::ws::ClashConnectionsConnectorState::Disconnected
}

fn clash_ws_kind(present: bool) -> nyanpasu_core::clash::ws::ClashWsKind {
    let _ = present;
    nyanpasu_core::clash::ws::ClashWsKind::Connections
}

fn clash_ws_recording(present: bool) -> nyanpasu_core::clash::ws::ClashWsRecording {
    let _ = present;
    nyanpasu_core::clash::ws::ClashWsRecording {
        connections: present,
        logs: present,
        traffic: present,
        memory: present,
    }
}

fn clash_ws_memory(present: bool) -> nyanpasu_core::clash::ws::ClashWsMemory {
    let _ = present;
    nyanpasu_core::clash::ws::ClashWsMemory {
        inuse: 78,
        oslimit: 79,
    }
}

fn clash_ws_traffic(present: bool) -> nyanpasu_core::clash::ws::ClashWsTraffic {
    let _ = present;
    nyanpasu_core::clash::ws::ClashWsTraffic { up: 80, down: 7 }
}

fn clash_ws_snapshot(present: bool) -> nyanpasu_core::clash::ws::ClashWsSnapshot {
    let _ = present;
    nyanpasu_core::clash::ws::ClashWsSnapshot {
        sequence: 81,
        state: clash_connections_connector_state(present),
        recording: clash_ws_recording(present),
        connections: if present {
            vec![clash_connections_summary(present)]
        } else {
            vec![]
        },
        traffic: if present {
            vec![clash_ws_traffic(present)]
        } else {
            vec![]
        },
        memory: if present {
            vec![clash_ws_memory(present)]
        } else {
            vec![]
        },
    }
}

fn clash_ws_update(present: bool) -> nyanpasu_core::clash::ws::ClashWsUpdate {
    let _ = present;
    nyanpasu_core::clash::ws::ClashWsUpdate::Reset(Box::new(clash_ws_snapshot(present)))
}

fn frontend_event_kind(present: bool) -> nyanpasu_core::client::frontend_events::FrontendEventKind {
    let _ = present;
    nyanpasu_core::client::frontend_events::FrontendEventKind::Console
}

fn frontend_event_level(
    present: bool,
) -> nyanpasu_core::client::frontend_events::FrontendEventLevel {
    let _ = present;
    nyanpasu_core::client::frontend_events::FrontendEventLevel::Warning
}

fn frontend_error_cause(
    present: bool,
) -> nyanpasu_core::client::frontend_events::FrontendErrorCause {
    let _ = present;
    nyanpasu_core::client::frontend_events::FrontendErrorCause {
        name: if present {
            Some("field-120/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        message: "field-121/路徑\\file.yaml".to_owned(),
        stack: if present {
            Some("field-122/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn frontend_event(present: bool) -> nyanpasu_core::client::frontend_events::FrontendEvent {
    let _ = present;
    nyanpasu_core::client::frontend_events::FrontendEvent {
        kind: frontend_event_kind(present),
        level: frontend_event_level(present),
        message: "field-123/路徑\\file.yaml".to_owned(),
        error_name: if present {
            Some("field-124/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        stack: if present {
            Some("field-125/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        causes: if present {
            vec![frontend_error_cause(present)]
        } else {
            vec![]
        },
        component_stack: if present {
            Some("field-126/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        fingerprint: "field-127/路徑\\file.yaml".to_owned(),
        count: 82,
        first_seen_ms: if present { Some(1.25) } else { None },
        last_seen_ms: if present { Some(1.25) } else { None },
        route: "field-128/路徑\\file.yaml".to_owned(),
    }
}

fn core_log_cursor(present: bool) -> nyanpasu_observers::logs::CoreLogCursor {
    let _ = present;
    nyanpasu_observers::logs::CoreLogCursor {
        generation: "field-129/路徑\\file.yaml".to_owned(),
        sequence: 83,
    }
}

fn core_log_source(present: bool) -> nyanpasu_observers::logs::CoreLogSource {
    let _ = present;
    nyanpasu_observers::logs::CoreLogSource {
        capture: "field-130/路徑\\file.yaml".to_owned(),
        instance_id: "field-131/路徑\\file.yaml".to_owned(),
        core_kind: if present {
            Some("field-132/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn core_log_record(present: bool) -> nyanpasu_observers::logs::CoreLogRecord {
    let _ = present;
    nyanpasu_observers::logs::CoreLogRecord {
        source: core_log_source(present),
        received_at: 84,
        time: if present {
            Some("field-133/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        log_type: "field-134/路徑\\file.yaml".to_owned(),
        payload: "field-135/路徑\\file.yaml".to_owned(),
    }
}

fn core_log_row(present: bool) -> nyanpasu_observers::logs::CoreLogRow {
    let _ = present;
    nyanpasu_observers::logs::CoreLogRow {
        id: core_log_cursor(present),
        record: core_log_record(present),
        truncated: present,
    }
}

fn core_log_direction(present: bool) -> nyanpasu_observers::logs::CoreLogDirection {
    let _ = present;
    nyanpasu_observers::logs::CoreLogDirection::Latest
}

fn core_log_status(present: bool) -> nyanpasu_observers::logs::CoreLogStatus {
    let _ = present;
    nyanpasu_observers::logs::CoreLogStatus {
        generation: "field-136/路徑\\file.yaml".to_owned(),
        version: 85,
        first: if present {
            Some(core_log_cursor(present))
        } else {
            None
        },
        head: if present {
            Some(core_log_cursor(present))
        } else {
            None
        },
        bytes: 86,
        error: if present {
            Some("field-137/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        discarded: 87,
    }
}

fn updater_state(present: bool) -> nyanpasu_updater::UpdaterState {
    let _ = present;
    nyanpasu_updater::UpdaterState::Idle
}

fn download_status(present: bool) -> nyanpasu_updater::DownloadStatus {
    let _ = present;
    nyanpasu_updater::DownloadStatus {
        state: downloader_state(present),
        downloaded: 88,
        total: 89,
        speed: 1.25,
    }
}

fn downloader_state(present: bool) -> nyanpasu_updater::DownloaderState {
    let _ = present;
    nyanpasu_updater::DownloaderState::Idle
}

fn app_update_phase(present: bool) -> nyanpasu_core::client::app_update::AppUpdatePhase {
    let _ = present;
    nyanpasu_core::client::app_update::AppUpdatePhase::Idle
}

fn app_update_release(present: bool) -> nyanpasu_core::client::app_update::AppUpdateRelease {
    let _ = present;
    nyanpasu_core::client::app_update::AppUpdateRelease {
        version: "field-138/路徑\\file.yaml".to_owned(),
        date: if present {
            Some("field-139/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        body: if present {
            Some("field-140/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    }
}

fn app_update_changelog_source(
    present: bool,
) -> nyanpasu_core::client::app_update::changelog::AppUpdateChangelogSource {
    let _ = present;
    nyanpasu_core::client::app_update::changelog::AppUpdateChangelogSource::Changelog
}

fn app_update_changelog_entry(
    present: bool,
) -> nyanpasu_core::client::app_update::changelog::AppUpdateChangelogEntry {
    let _ = present;
    nyanpasu_core::client::app_update::changelog::AppUpdateChangelogEntry {
        version: "field-141/路徑\\file.yaml".to_owned(),
        date: if present {
            Some("field-142/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        body: "field-143/路徑\\file.yaml".to_owned(),
        source: app_update_changelog_source(present),
    }
}

fn receive<T: Serialize + DeserializeOwned>(value: impl Serialize) -> serde_json::Value {
    let bytes = serde_json::to_vec(&value).unwrap();
    let expected = serde_json::to_value(&value).unwrap();
    let received: T = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&received).unwrap(), bytes);
    assert_eq!(serde_json::to_value(received).unwrap(), expected);
    expected
}

fn error_sources() {
    use crate::error::{IpcError, IpcErrorKind, application_error};
    use nyanpasu_contracts::{
        profiles::error::ProfileFileError, system_dns::SystemDnsError, system_proxy::OsProxyError,
    };
    use nyanpasu_core::{
        client::{debug_http::DebugHttpControlError, effects::error::EffectsError},
        state::config_error::ConfigError,
        storage::StorageOperationError,
    };
    use nyanpasu_rpc_protocol::wire::receive;
    use nyanpasu_runtime_values::{
        mutation::NotReady,
        profiles::ProfilesError,
        runtime::{Degradation, DegradationPhase, DegradationReason, MutationOutcome},
        runtime_error::RuntimeError,
    };
    use std::{io, path::PathBuf, sync::Arc};

    let io_source = || io::Error::other("operational source retained only on the host");
    let file = receive::<receive::ProfileFileError>(ProfileFileError::ReadFile {
        path: PathBuf::from("profiles/測試.yaml").into(),
        source: io_source(),
    });
    assert_eq!(
        file,
        json!({"kind": "read_file", "path": "profiles/測試.yaml"})
    );

    let errors = [
        IpcError::unknown("unclassified"),
        ProfilesError::InvalidSubscriptionUrl {
            url: "bad URL".into(),
            source: url::ParseError::RelativeUrlWithoutBase,
        }
        .into(),
        RuntimeError::ShuttingDown.into(),
        ConfigError::WorkflowNotReady { source: NotReady }.into(),
        StorageOperationError::DecodeValue {
            key: "storage-key".into(),
            source: serde_json::from_str::<bool>("invalid").unwrap_err(),
        }
        .into(),
        SystemDnsError::RunFlushCommand {
            command: "ipconfig",
            source: io_source(),
        }
        .into(),
        OsProxyError::ReadOsProxy {
            source: Box::new(io_source()),
        }
        .into(),
        EffectsError::EffectsStopped.into(),
        DebugHttpControlError::ReadStatus {
            source: anyhow::Error::new(io_source()),
        }
        .into(),
    ];
    for error in errors {
        let expected = serde_json::to_value(&error).unwrap();
        receive::<receive::IpcError>(&error);
        let envelope = receive::<receive::RpcError>(application_error(error));
        assert_eq!(envelope["domain_error"], expected);
        assert!(expected["kind"]["error"].get("source").is_none());
    }
    // A structured nested cause must survive even though its operational source does not.
    let runtime = RuntimeError::BuildRuntime {
        source: nyanpasu_profile_transform::RuntimeBuildError::RunPipeline {
            source: nyanpasu_config::runtime::executor::RuntimePipelineError::ExecuteScript {
                source: nyanpasu_config::runtime::executor::ScriptExecutionError::StartEngine {
                    runtime: nyanpasu_config::profile::ScriptRuntime::Lua,
                    operation: nyanpasu_config::runtime::executor::ScriptOperation::Transform,
                },
            },
        },
    };
    let kind = IpcErrorKind::Runtime(Box::new(runtime));
    let value = receive::<receive::IpcErrorKind>(kind);
    assert_eq!(
        value["error"]["source"]["source"]["source"]["kind"],
        "start_engine"
    );
    for cause in [None, Some(Arc::new(RuntimeError::OwnerUnavailable))] {
        let outcome = MutationOutcome::CommittedDegraded {
            value: (),
            commits: vec![],
            notifications_pending: false,
            degradations: vec![Degradation {
                phase: DegradationPhase::RuntimeApply,
                reason: DegradationReason::RuntimeRecoveryRequired {
                    operation_id: None,
                    cause,
                },
                message: "recovery required".into(),
                retryable: true,
            }],
        };
        let value = receive::<receive::MutationOutcome<()>>(outcome);
        assert!(value["value"].is_null());
        assert!(value["degradations"][0]["reason"].get("cause").is_some());
    }
}

fn request_defaults() {
    use super::IntoApplication;
    use nyanpasu_rpc_protocol::wire::{
        profiles::NewProfileRequest, updater::ManifestVersionLatest,
    };
    for folder in [None, Some(json!("folder-1"))] {
        let mut input = json!({"metadata": {"name": "profile"}, "definition": {"type": "config", "config": {"type": "composition"}}});
        if let Some(folder) = folder {
            input["folder"] = folder;
        }
        let expected: nyanpasu_core::state::profiles::actor::NewProfileRequest =
            sample(input.clone());
        let wire: NewProfileRequest = sample(input);
        let actual = wire.into_application();
        assert_eq!(
            serde_json::to_value(actual.metadata).unwrap(),
            serde_json::to_value(expected.metadata).unwrap()
        );
        assert_eq!(
            serde_json::to_value(actual.definition).unwrap(),
            serde_json::to_value(expected.definition).unwrap()
        );
        assert_eq!(actual.folder, expected.folder);
    }
    let old_manifest = json!({"mihomo":"1", "mihomo_alpha":"2", "clash_rs":"3", "clash_rs_alpha":"4", "clash_premium":"5", "meow":"6"});
    let receiving: ManifestVersionLatest = sample(old_manifest.clone());
    let application: nyanpasu_updater::ManifestVersionLatest = sample(old_manifest);
    assert_eq!(
        serde_json::to_value(receiving).unwrap(),
        serde_json::to_value(application).unwrap()
    );
}

fn storage_and_connection_details() {
    use nyanpasu_core::{
        clash::{
            connection_rates::{ConnectionDetails, TrafficRate},
            ws::ClashConnectionDetails,
        },
        storage::StorageItem,
    };
    use std::sync::Arc;
    for value in [None, Some("{\"raw\":true}".to_owned())] {
        check(StorageItem {
            value,
            revision: sample(json!(9007199254740991_u64)),
        });
    }
    for connections in [
        None,
        Some(vec![]),
        Some(vec![sample(json!({
            "id": "00000000-0000-0000-0000-000000000001", "metadata": {"network": "tcp", "futureMetadata": [null, 7]},
            "upload": 4, "download": 9, "start": "2026-11-07T12:30:00+08:00",
            "chains": ["Proxy A"], "rule": "MATCH", "rulePayload": "", "futureConnection": {"value": true}
        }))]),
    ] {
        let rates = vec![
            TrafficRate {
                download: 5,
                upload: 2
            };
            connections.as_ref().map_or(0, Vec::len)
        ];
        let snapshot = clash_api::ConnectionsSnapshot {
            connections,
            download_total: 9,
            upload_total: 4,
            memory: None,
        };
        let frame = ClashConnectionDetails {
            sequence: 90,
            connections: ConnectionDetails::new(Arc::new(snapshot), rates),
        };
        let frame = Arc::new(frame);
        let projected = frame.into_wire();
        let expected = serde_json::to_vec(&frame).unwrap();
        assert_eq!(serde_json::to_vec(&projected).unwrap(), expected);
        let received: nyanpasu_rpc_protocol::wire::streams::ClashConnectionDetails =
            serde_json::from_slice(&expected).unwrap();
        assert_eq!(serde_json::to_vec(&received).unwrap(), expected);
        // Sending retains the original backing frame, including all connections.
        assert_eq!(Arc::strong_count(&frame), 2);
    }
}
