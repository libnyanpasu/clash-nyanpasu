use serde::de::DeserializeOwned;

use super::{
    CoreLogsChanged, CoreLogsSummaryChanged, IpcError, IpcErrorKind, MutationOutcome, RpcError,
    RuntimeError,
};

#[test]
fn fixed_error_formats_preserve_tags_optional_fields_and_defaults() {
    let envelope: RpcError = serde_json::from_str(
        r#"{"kind":"application_error","message":"missing profile","domain_error":{"kind":{"domain":"profiles","error":{"kind":"profile_not_found","uid":"profile-1"}},"message":"missing profile","detail":"host diagnostic"}}"#,
    )
    .unwrap();
    assert!(envelope.code.is_none());
    assert!(envelope.retryable.is_none());
    assert!(envelope.operation_id.is_none());
    let error = envelope.decode_domain_error::<IpcError>().unwrap().unwrap();
    assert_eq!(error.message, "missing profile");
    assert_eq!(error.detail.as_deref(), Some("host diagnostic"));
    let IpcErrorKind::Profiles(error) = error.kind else {
        panic!("the profile domain tag must be preserved");
    };
    let super::ProfilesError::ProfileNotFound { uid } = *error else {
        panic!("the profile error tag must be preserved");
    };
    assert_eq!(serde_json::to_value(uid).unwrap(), "profile-1");

    for (literal, expected) in [
        (
            r#"{"kind":"serialize_runtime_config"}"#,
            "serialize_runtime_config",
        ),
        (r#"{"kind":"core_rolled_back"}"#, "core_rolled_back"),
        (
            r#"{"kind":"owner_unresponsive","operation_id":"op-1"}"#,
            "owner_unresponsive",
        ),
    ] {
        let error: RuntimeError = serde_json::from_str(literal).unwrap();
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["kind"], expected);
        assert!(value.get("source").is_none());
    }
    let mutation: MutationOutcome<()> = serde_json::from_str(
        r#"{"status":"committed_degraded","value":null,"commits":[],"notifications_pending":true,"degradations":[{"phase":"runtime_apply","reason":{"code":"runtime_deferred","cause":{"kind":"shutting_down"}},"message":"later","retryable":true}]}"#,
    )
    .unwrap();
    let MutationOutcome::CommittedDegraded {
        value: (),
        commits,
        notifications_pending,
        degradations,
    } = mutation
    else {
        panic!("the committed-degraded tag must be preserved");
    };
    assert!(commits.is_empty());
    assert!(notifications_pending);
    assert_eq!(degradations.len(), 1);
    assert!(matches!(
        degradations[0].reason,
        super::DegradationReason::RuntimeDeferred { .. }
    ));
    assert!(degradations[0].retryable);

    let request: nyanpasu_config::profile::NewProfileRequest = serde_json::from_str(
        r#"{"metadata":{"name":"profile"},"definition":{"type":"config","config":{"type":"composition"}}}"#,
    )
    .unwrap();
    assert!(request.folder.is_none());

    let absent: RpcError =
        serde_json::from_str(r#"{"kind":"unsupported","message":"unavailable"}"#).unwrap();
    assert!(absent.decode_domain_error::<IpcError>().unwrap().is_none());
    let invalid: RpcError = serde_json::from_str(r#"{"kind":"application_error","message":"invalid","domain_error":{"kind":"unknown_variant"}}"#).unwrap();
    assert!(invalid.decode_domain_error::<RuntimeError>().is_err());
}

#[test]
fn protocol_types_build_without_application_crates() {
    fn owned_snapshot<T: DeserializeOwned + Send + 'static>() {}

    owned_snapshot::<nyanpasu_runtime_values::configuration::ConfigurationStatus>();
    owned_snapshot::<nyanpasu_runtime_values::core_status::CoreStatusInfo>();
    owned_snapshot::<nyanpasu_runtime_values::core_status::ServiceHostStatus>();
    owned_snapshot::<nyanpasu_runtime_values::app_update::AppUpdateSnapshot>();
    owned_snapshot::<CoreLogsChanged>();
    owned_snapshot::<CoreLogsSummaryChanged>();
    owned_snapshot::<super::ClashConnectionDetails>();
    owned_snapshot::<super::PublicFailure>();
}

#[test]
fn public_error_forms_omit_detail_and_preserve_decision_metadata() {
    use super::{
        PublicCommitAborted, PublicCommitAbortedKind, PublicConfigError, PublicCoreLogReadError,
        PublicFailure, PublicHotkeyErrorKind, PublicOsProxyError, PublicProfilesError,
        PublicRuntimeError, PublicStorageError, PublicSubscriptionFetchKind, PublicSystemDnsError,
    };
    use crate::{
        AuthContext, Principal, ResponseVisibility, RpcError as ServerRpcError, Scope, ScopeSet,
    };

    let public = IpcError {
        kind: IpcErrorKind::Public(Box::new(PublicFailure::Profiles(
            PublicProfilesError::Commit {
                source: PublicCommitAborted {
                    kind: PublicCommitAbortedKind::RuntimeRefused,
                    runtime_causes: vec![PublicRuntimeError::CoreRejectedConfig {
                        core_kind: Some(nyanpasu_core_metadata::CoreErrorKind::InvalidConfig),
                    }],
                },
            },
        ))),
        message: "profile update failed".into(),
        detail: None,
    };
    let wire = serde_json::to_value(&public).unwrap();
    assert!(
        wire.get("detail").is_none(),
        "public IpcError omits detail key"
    );
    let decoded: IpcError = serde_json::from_value(wire).unwrap();
    assert!(decoded.detail.is_none());
    let IpcErrorKind::Public(failure) = decoded.kind else {
        panic!("expected public domain failure");
    };
    assert_eq!(
        *failure,
        PublicFailure::Profiles(PublicProfilesError::Commit {
            source: PublicCommitAborted {
                kind: PublicCommitAbortedKind::RuntimeRefused,
                runtime_causes: vec![PublicRuntimeError::CoreRejectedConfig {
                    core_kind: Some(nyanpasu_core_metadata::CoreErrorKind::InvalidConfig),
                }],
            },
        })
    );

    let fetch = IpcError {
        kind: IpcErrorKind::Public(Box::new(PublicFailure::Profiles(
            PublicProfilesError::FetchSubscription {
                reason: PublicSubscriptionFetchKind::SubscriptionHttpStatus { status: 403 },
            },
        ))),
        message: "subscription download failed".into(),
        detail: None,
    };
    let fetch_json = serde_json::to_string(&fetch).unwrap();
    assert!(!fetch_json.contains("detail"));
    let roundtrip: IpcError = serde_json::from_str(&fetch_json).unwrap();
    assert!(matches!(
        roundtrip.kind,
        IpcErrorKind::Public(f) if matches!(
            *f,
            PublicFailure::Profiles(PublicProfilesError::FetchSubscription {
                reason: PublicSubscriptionFetchKind::SubscriptionHttpStatus { status: 403 }
            })
        )
    ));

    let read_only = AuthContext::new(
        Principal::Desktop {
            window: "test".into(),
        },
        ScopeSet::from_iter([Scope::Read]),
    );
    let full = AuthContext::new(
        Principal::Desktop {
            window: "test".into(),
        },
        ScopeSet::from_iter([Scope::Read, Scope::SensitiveRead]),
    );
    assert_eq!(
        ResponseVisibility::for_auth(Some(&read_only)),
        ResponseVisibility::Public
    );
    assert_eq!(
        ResponseVisibility::for_auth(None),
        ResponseVisibility::Public
    );
    assert_eq!(
        ResponseVisibility::for_auth(Some(&full)),
        ResponseVisibility::Full
    );

    let public_unknown =
        ServerRpcError::unknown_method_for(ResponseVisibility::Public, "secret_method_token");
    assert_eq!(public_unknown.kind, "unknown_method");
    assert!(!public_unknown.message.contains("secret_method_token"));
    let full_unknown =
        ServerRpcError::unknown_method_for(ResponseVisibility::Full, "secret_method_token");
    assert!(full_unknown.message.contains("secret_method_token"));

    let public_params =
        ServerRpcError::invalid_params_for(ResponseVisibility::Public, "bad value `tok-123`");
    assert_eq!(public_params.kind, "invalid_params");
    assert!(!public_params.message.contains("tok-123"));
    let full_params =
        ServerRpcError::invalid_params_for(ResponseVisibility::Full, "bad value `tok-123`");
    assert!(full_params.message.contains("tok-123"));

    for failure in [
        PublicFailure::Runtime(PublicRuntimeError::ApplyRuntime {
            core_kind: Some(nyanpasu_core_metadata::CoreErrorKind::BackendUnavailable),
        }),
        PublicFailure::Config(PublicConfigError::ValidateHotkeys {
            reason: PublicHotkeyErrorKind::DuplicateAccelerator,
        }),
        PublicFailure::CoreLogRead(PublicCoreLogReadError::Unavailable),
        PublicFailure::SystemDns(PublicSystemDnsError::FlushRejected { exit_code: Some(1) }),
        PublicFailure::Storage(PublicStorageError::WriteItem),
        PublicFailure::SystemProxy(PublicOsProxyError::WriteOsProxy {
            enable: true,
            port: 7890,
        }),
        PublicFailure::Core {
            kind: nyanpasu_core_metadata::CoreErrorKind::OperationConflict,
        },
    ] {
        let wrapped = IpcError {
            kind: IpcErrorKind::Public(Box::new(failure.clone())),
            message: "public failure".into(),
            detail: None,
        };
        let encoded = serde_json::to_value(&wrapped).unwrap();
        assert!(encoded.get("detail").is_none());
        let decoded: IpcError = serde_json::from_value(encoded).unwrap();
        let IpcErrorKind::Public(decoded_failure) = decoded.kind else {
            panic!("expected public failure");
        };
        assert_eq!(*decoded_failure, failure);
    }
}

#[test]
fn public_and_received_mutation_outcomes_decode_full_and_public_payloads() {
    use nyanpasu_runtime_values::{
        app_update::{AppUpdatePhase, AppUpdateRelease, AppUpdateSnapshot, AppUpdateSummary},
        runtime::{CommitReceipt, DegradationPhase, RuntimeCommitStatus},
    };

    let full_json = serde_json::json!({
        "status": "committed_degraded",
        "value": "profile-1",
        "commits": [{
            "operation_id": "op-1",
            "domain": "profiles",
            "source_version": 3,
            "runtime": "applied"
        }],
        "notifications_pending": true,
        "degradations": [{
            "phase": "runtime_apply",
            "reason": {"code": "runtime_deferred", "cause": {"kind": "shutting_down"}},
            "message": "secret host path /home/alice/cfg.yaml",
            "retryable": true
        }]
    });
    let full_received: super::ReceivedMutationOutcome<String> =
        serde_json::from_value(full_json).unwrap();
    assert!(matches!(
        full_received,
        super::ReceivedMutationOutcome::Full(_)
    ));

    let public_outcome = super::PublicMutationOutcome::CommittedDegraded {
        value: "profile-1".to_owned(),
        commits: vec![CommitReceipt {
            operation_id: Some("op-1".to_owned()),
            domain: "profiles".to_owned(),
            source_version: 3,
            runtime: RuntimeCommitStatus::Applied,
        }],
        notifications_pending: true,
        degradations: vec![
            super::PublicDegradation {
                phase: DegradationPhase::RuntimeApply,
                reason: super::PublicDegradationReason::RuntimeDeferred {
                    cause: super::PublicRuntimeError::ShuttingDown,
                },
                retryable: true,
            },
            super::PublicDegradation {
                phase: DegradationPhase::SystemEffect,
                reason: super::PublicDegradationReason::ProfileAutoActivationFailed {
                    cause: super::PublicProfilesError::Commit {
                        source: super::PublicCommitAborted {
                            kind: super::PublicCommitAbortedKind::RuntimeFailed,
                            runtime_causes: vec![super::PublicRuntimeError::CoreRejectedConfig {
                                core_kind: Some(
                                    nyanpasu_core_metadata::CoreErrorKind::InvalidConfig,
                                ),
                            }],
                        },
                    },
                },
                retryable: false,
            },
        ],
    };
    let encoded_public = serde_json::to_value(&public_outcome).unwrap();
    assert_eq!(encoded_public["status"], "committed_degraded");
    assert_eq!(encoded_public["value"], "profile-1");
    assert_eq!(encoded_public["notifications_pending"], true);
    assert!(encoded_public["degradations"][0].get("message").is_none());

    let public_received: super::ReceivedMutationOutcome<String> =
        serde_json::from_value(encoded_public).unwrap();
    assert!(matches!(
        public_received,
        super::ReceivedMutationOutcome::Public(_)
    ));

    let snapshot = AppUpdateSnapshot {
        revision: 9,
        phase: AppUpdatePhase::Failed,
        release: Some(AppUpdateRelease {
            version: "2.1.0".into(),
            date: Some("2026-11-14".into()),
            body: Some("secret release notes".into()),
        }),
        downloaded: 512,
        total: Some(1024),
        speed: 128.0,
        source: Some(nyanpasu_config::application::UpdateSource::Github),
        error: Some("https://user:pass@secret.example/update failed".into()),
        last_checked_at: Some("2026-11-14T00:00:00Z".into()),
        supported: true,
        endpoints: vec!["https://user:pass@secret.example/latest.json".into()],
    };
    let summary = AppUpdateSummary::from(&snapshot);
    let summary_json = serde_json::to_value(&summary).unwrap();
    assert_eq!(summary_json["phase"], "failed");
    assert_eq!(summary_json["source"], "github");
    assert_eq!(summary_json["has_error"], true);
    assert!(summary_json.get("endpoints").is_none());
    assert!(summary_json.get("error").is_none());
    assert!(summary_json["release"].get("body").is_none());
    let decoded_summary: super::RpcAppUpdateOutcome = serde_json::from_value(summary_json).unwrap();
    assert!(matches!(
        decoded_summary,
        super::RpcAppUpdateOutcome::Public(_)
    ));
}
