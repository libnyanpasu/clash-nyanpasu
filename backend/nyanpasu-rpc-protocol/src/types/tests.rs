use serde::de::DeserializeOwned;

use super::{CoreLogsChanged, IpcError, IpcErrorKind, MutationOutcome, RpcError, RuntimeError};

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
    assert_eq!(error.detail, "host diagnostic");
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
    owned_snapshot::<super::ClashConnectionDetails>();
}
