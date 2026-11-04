//! The wire shape of each command failure.

use nyanpasu_config::profile::{ManagedProfilePath, ProfileId};
use nyanpasu_contracts::profiles::error::{ProfileFileError, SubscriptionFetchError};
use nyanpasu_core::state::{
    ReplaceIfVersionError,
    mutation::{CommitAborted, RuntimeAftermath, WriteConfigSnafu},
};
use serde_json::json;
use snafu::IntoError;

use super::*;

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
    let failure = nyanpasu_core::client::runtime_error::ApplyRuntimeSnafu.into_error(
        nyanpasu_core_manager::CoreError::new(
            nyanpasu_core_manager::CoreErrorKind::ApplyFailed,
            "the core kept the previous configuration",
            false,
        ),
    );
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
        serde_json::to_value(IpcError::from(RuntimeError::Isolated)).unwrap()["kind"],
        json!({ "domain": "runtime", "error": { "kind": "isolated" } })
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
fn an_aborted_commit_names_what_became_of_the_runtime() {
    let cause = ReplaceIfVersionError::WriteConfig(anyhow::anyhow!("disk full"));
    let aborted = WriteConfigSnafu {
        runtime: RuntimeAftermath::RollbackFailed {
            detail: std::sync::Arc::new(RuntimeError::OwnerUnresponsive {
                operation_id: "op1".into(),
            }),
        },
    }
    .into_error(cause);
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
                "runtime": {
                    "kind": "rollback_failed",
                    "detail": { "kind": "owner_unresponsive", "operation_id": "op1" },
                },
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

#[test]
fn errors_preserve_domain_codes_and_operation_identity() {
    use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
    let id = OperationId::generate();
    let wire = application_error(
        CoreError::new(CoreErrorKind::BackendUnavailable, "pending", false).with_operation(id),
    );
    assert_eq!(wire.operation_id, Some(id.to_string()));
    assert_eq!(wire.retryable, Some(false));
    assert_eq!(
        wire.code,
        Some(CoreErrorKind::BackendUnavailable.to_string())
    );
    let ipc = IpcError::from(std::io::Error::other("request failed"));
    let wire = application_error(ipc);
    let payload = serde_json::to_value(wire).unwrap();
    assert_eq!(payload["domain_error"]["kind"]["domain"], "unknown");
    assert_eq!(payload["domain_error"]["message"], "request failed");
    assert!(payload["domain_error"]["detail"].is_string());
    let log = application_error(nyanpasu_logging::LogError::SessionExpired);
    assert_eq!(
        serde_json::to_value(log).unwrap()["domain_error"],
        "session_expired"
    );
}

#[test]
fn unknown_failures_keep_their_message_and_detail() {
    #[derive(Debug)]
    struct ShellFailure;
    impl fmt::Display for ShellFailure {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("window is gone")
        }
    }
    let wire = serde_json::to_value(IpcError::unknown(ShellFailure)).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "kind": {"domain": "unknown"},
            "message": "window is gone",
            "detail": "ShellFailure",
        })
    );
}
