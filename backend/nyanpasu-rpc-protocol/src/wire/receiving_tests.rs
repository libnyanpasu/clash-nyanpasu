use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;

use super::receive;

fn sample<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn check<T: Serialize + DeserializeOwned>(
    value: T,
    discriminator: Option<(&str, &str)>,
    fields: &[&str],
) {
    let expected = serde_json::to_value(value).unwrap();
    if let Some((field, variant)) = discriminator {
        assert_eq!(
            if field.is_empty() {
                &expected
            } else {
                &expected[field]
            },
            variant
        );
    }
    if let Some(object) = expected.as_object() {
        let actual: std::collections::BTreeSet<_> = object.keys().map(String::as_str).collect();
        assert_eq!(actual, fields.iter().copied().collect());
    }
    let decoded: T = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        expected,
        "{}",
        std::any::type_name::<T>()
    );
}

#[test]
fn receiving_errors_decode_without_operational_sources() {
    for present in [false, true] {
        check(
            receive::RpcError {
                kind: "field-1/路徑\\file.yaml".to_owned(),
                message: "field-2/路徑\\file.yaml".to_owned(),
                code: if present {
                    Some("field-3/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
                retryable: if present { Some(present) } else { None },
                operation_id: if present {
                    Some("field-4/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
                domain_error: if present {
                    Some(json!({"nested": [null, true, 42]}))
                } else {
                    None
                },
            },
            None,
            &[
                "kind",
                "message",
                "code",
                "retryable",
                "operation_id",
                "domain_error",
            ],
        );
        check(
            receive::RuntimeError::ShuttingDown,
            Some(("kind", "shutting_down")),
            &["kind"],
        );
        check(
            receive::RuntimeError::Isolated,
            Some(("kind", "isolated")),
            &["kind"],
        );
        check(
            receive::RuntimeError::OwnerUnresponsive {
                operation_id: "field-5/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "owner_unresponsive")),
            &["kind", "operation_id"],
        );
        check(
            receive::RuntimeError::OwnerUnavailable,
            Some(("kind", "owner_unavailable")),
            &["kind"],
        );
        check(
            receive::RuntimeError::SourceSettled,
            Some(("kind", "source_settled")),
            &["kind"],
        );
        check(
            receive::RuntimeError::ApplyRuntime {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "apply_runtime")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::StopCore {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "stop_core")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::RecoverRuntime {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "recover_runtime")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::RecoverServiceEndpoint {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "recover_service_endpoint")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::RefreshStatus {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "refresh_status")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::InstallService {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "install_service")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::StartService {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "start_service")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::StopService {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "stop_service")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::RestartService {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "restart_service")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::UninstallService {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "uninstall_service")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::MoveHost {
                host: sample(json!(if present { "service" } else { "local" })),
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "move_host")),
            &["kind", "host", "failure"],
        );
        check(
            receive::RuntimeError::ServiceHostsCore,
            Some(("kind", "service_hosts_core")),
            &["kind"],
        );
        check(
            receive::RuntimeError::CoreNotStarted {
                reason: "field-6/路徑\\file.yaml".to_owned(),
                retryable: present,
            },
            Some(("kind", "core_not_started")),
            &["kind", "reason", "retryable"],
        );
        check(
            receive::RuntimeError::RecoveryUnresolved {
                reason: "field-7/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "recovery_unresolved")),
            &["kind", "reason"],
        );
        check(
            receive::RuntimeError::BuildRuntime {
                source: runtime_build_error(present),
            },
            Some(("kind", "build_runtime")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeError::PublishRuntime {
                source: publish_runtime_error(present),
            },
            Some(("kind", "publish_runtime")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeError::ResolvePort {
                source: port_resolve_error(present),
            },
            Some(("kind", "resolve_port")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeError::ResolveCoreBinary {
                source: core_spec_error(present),
            },
            Some(("kind", "resolve_core_binary")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeError::InstallCoreBinary {
                source: install_core_binary_error(present),
            },
            Some(("kind", "install_core_binary")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeError::PrepareServiceInstallPrompt {
                source: service_command_error(present),
            },
            Some(("kind", "prepare_service_install_prompt")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeError::ReadCoreVersion {
                source: core_version_error(present),
            },
            Some(("kind", "read_core_version")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeError::UnsettledBaseline {
                gap: sample(json!("no_restorable_baseline")),
            },
            Some(("kind", "unsettled_baseline")),
            &["kind", "gap"],
        );
        check(
            receive::RuntimeError::CoreRejectedConfig {
                core_kind: if present {
                    Some(sample(json!("backend_unavailable")))
                } else {
                    None
                },
                message: "field-8/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "core_rejected_config")),
            &["kind", "core_kind", "message"],
        );
        check(
            receive::RuntimeError::CheckUnavailable {
                reason: sample(json!({"cause": "no_endpoint", "reason": "offline"})),
            },
            Some(("kind", "check_unavailable")),
            &["kind", "reason"],
        );
        check(
            receive::RuntimeError::CoreRolledBack {
                reason: if present {
                    Some("field-9/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
            },
            Some(("kind", "core_rolled_back")),
            &["kind", "reason"],
        );
        check(
            receive::RuntimeError::SubmissionUnobserved {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "submission_unobserved")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::HandoffOwnerMismatch {
                expected: sample(json!(if present { "service" } else { "local" })),
            },
            Some(("kind", "handoff_owner_mismatch")),
            &["kind", "expected"],
        );
        check(
            receive::RuntimeError::HandoffNotRestored {
                failure: restore_failure(present),
            },
            Some(("kind", "handoff_not_restored")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::RestoreFailed {
                failure: restore_failure(present),
            },
            Some(("kind", "restore_failed")),
            &["kind", "failure"],
        );
        check(
            receive::RuntimeError::CommittedAfterRefusal {
                operation_id: "field-10/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "committed_after_refusal")),
            &["kind", "operation_id"],
        );
        check(
            receive::RuntimeError::NoRuntimeConfig,
            Some(("kind", "no_runtime_config")),
            &["kind"],
        );
        check(
            receive::RuntimeError::SerializeRuntimeConfig {},
            Some(("kind", "serialize_runtime_config")),
            &["kind"],
        );
        check(
            receive::RuntimeError::ConvertRuntimeConfig {},
            Some(("kind", "convert_runtime_config")),
            &["kind"],
        );
        check(
            receive::RuntimeError::RuntimeSnapshotChanged,
            Some(("kind", "runtime_snapshot_changed")),
            &["kind"],
        );
        check(
            receive::RuntimeError::RuntimeNodeNotFound { node_id: 7 },
            Some(("kind", "runtime_node_not_found")),
            &["kind", "node_id"],
        );
        check(
            receive::RestoreFailure::MoveHostBack {
                host: sample(json!(if present { "service" } else { "local" })),
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "move_host_back")),
            &["kind", "host", "failure"],
        );
        check(
            receive::RestoreFailure::ReadStatus {
                failure: sample(
                    json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                ),
            },
            Some(("kind", "read_status")),
            &["kind", "failure"],
        );
        check(
            receive::RestoreFailure::Unobserved {
                failure: if present {
                    Some(sample(
                        json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                    ))
                } else {
                    None
                },
            },
            Some(("kind", "unobserved")),
            &["kind", "failure"],
        );
        check(
            receive::RestoreFailure::Unverified {
                failure: if present {
                    Some(sample(
                        json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
                    ))
                } else {
                    None
                },
            },
            Some(("kind", "unverified")),
            &["kind", "failure"],
        );
        check(
            receive::RestoreFailure::NotRecorded,
            Some(("kind", "not_recorded")),
            &["kind"],
        );
        check(
            receive::ProfileContentError::NotYamlMapping {},
            Some(("kind", "not_yaml_mapping")),
            &["kind"],
        );
        check(
            receive::ProfileContentError::ReserializeYaml {},
            Some(("kind", "reserialize_yaml")),
            &["kind"],
        );
        check(
            receive::ProfileContentError::MissingProxies,
            Some(("kind", "missing_proxies")),
            &["kind"],
        );
        check(
            receive::ProfileContentError::EmptyScript,
            Some(("kind", "empty_script")),
            &["kind"],
        );
        check(
            receive::ProfilesError::ProfileNotFound {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "profile_not_found")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::ProfileHasNoFile {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "profile_has_no_file")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::NotARemoteProfile {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "not_a_remote_profile")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::ProfileFileNotWritable {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "profile_file_not_writable")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::ProfileInUse {
                uid: sample(json!("profile-1")),
                referrers: if present {
                    vec![sample(json!("profile-1"))]
                } else {
                    vec![]
                },
                current: present,
                global_transforms: present,
            },
            Some(("kind", "profile_in_use")),
            &["kind", "uid", "referrers", "current", "global_transforms"],
        );
        check(
            receive::ProfilesError::ProfileIdCollision {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "profile_id_collision")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::ValidationFailed { errors: vec![] },
            Some(("kind", "validation_failed")),
            &["kind", "errors"],
        );
        check(
            receive::ProfilesError::ReorderListSizeMismatch {
                expected: 2,
                got: 3,
            },
            Some(("kind", "reorder_list_size_mismatch")),
            &["kind", "expected", "got"],
        );
        check(
            receive::ProfilesError::ReorderListDuplicate {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "reorder_list_duplicate")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::RevisionOverflow {},
            Some(("kind", "revision_overflow")),
            &["kind"],
        );
        check(
            receive::ProfilesError::InvalidSubscriptionUrl {
                url: "field-11/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "invalid_subscription_url")),
            &["kind", "url"],
        );
        check(
            receive::ProfilesError::RemoteProfileNeedsImport,
            Some(("kind", "remote_profile_needs_import")),
            &["kind"],
        );
        check(
            receive::ProfilesError::RefreshInProgress {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "refresh_in_progress")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::FetchSubscription {
                url: "https://example.test/release".parse().unwrap(),
                source: subscription_fetch_error(present),
            },
            Some(("kind", "fetch_subscription")),
            &["kind", "url", "source"],
        );
        check(
            receive::ProfilesError::ProfileContentRejected {
                source: profile_content_error(present),
            },
            Some(("kind", "profile_content_rejected")),
            &["kind", "source"],
        );
        check(
            receive::ProfilesError::ProfileDeletedDuringRefresh {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "profile_deleted_during_refresh")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::ProfileChangedDuringRefresh {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "profile_changed_during_refresh")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::FingerprintDefinition {
                uid: sample(json!("profile-1")),
            },
            Some(("kind", "fingerprint_definition")),
            &["kind", "uid"],
        );
        check(
            receive::ProfilesError::ReadProfileFile {
                uid: sample(json!("profile-1")),
                source: profile_file_error(present),
            },
            Some(("kind", "read_profile_file")),
            &["kind", "uid", "source"],
        );
        check(
            receive::ProfilesError::ProfileFileNotYaml {
                uid: sample(json!("profile-1")),
                source: profile_content_error(present),
            },
            Some(("kind", "profile_file_not_yaml")),
            &["kind", "uid", "source"],
        );
        check(
            receive::ProfilesError::ProfileFileMissing {
                uid: sample(json!("profile-1")),
                path: "field-12/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "profile_file_missing")),
            &["kind", "uid", "path"],
        );
        check(
            receive::ProfilesError::ReadExternalProfile {
                target: sample(json!("/profiles/source.yaml")),
                source: profile_file_error(present),
            },
            Some(("kind", "read_external_profile")),
            &["kind", "target", "source"],
        );
        check(
            receive::ProfilesError::Materialization {
                operation: sample(json!("prepare_file_first")),
                source: profile_file_error(present),
            },
            Some(("kind", "materialization")),
            &["kind", "operation", "source"],
        );
        check(
            receive::ProfilesError::VersionConflict {
                expected: 4,
                actual: 5,
            },
            Some(("kind", "version_conflict")),
            &["kind", "expected", "actual"],
        );
        check(
            receive::ProfilesError::Commit {
                uid: if present {
                    Some(sample(json!("profile-1")))
                } else {
                    None
                },
                path: if present {
                    Some(sample(json!("profile.yaml")))
                } else {
                    None
                },
                source: commit_aborted(present),
            },
            Some(("kind", "commit")),
            &["kind", "uid", "path", "source"],
        );
        check(
            receive::ProfilesError::WorkflowNotReady {},
            Some(("kind", "workflow_not_ready")),
            &["kind"],
        );
        check(
            receive::ProfilesError::ProfilesActorStopped,
            Some(("kind", "profiles_actor_stopped")),
            &["kind"],
        );
        check(
            receive::ProfilesError::ProfilesReplyDropped,
            Some(("kind", "profiles_reply_dropped")),
            &["kind"],
        );
        check(
            receive::ProfilesError::BlockingTaskCancelled {},
            Some(("kind", "blocking_task_cancelled")),
            &["kind"],
        );
        check(
            receive::ProfilesError::ShuttingDown,
            Some(("kind", "shutting_down")),
            &["kind"],
        );
        check(
            receive::MutationOutcome::<()>::Committed {
                value: (),
                commits: if present {
                    vec![sample(
                        json!({"operation_id": "op-1", "domain": "profiles", "source_version": 3, "runtime": "applied"}),
                    )]
                } else {
                    vec![]
                },
                notifications_pending: present,
            },
            Some(("status", "committed")),
            &["status", "value", "commits", "notifications_pending"],
        );
        check(
            receive::MutationOutcome::<()>::CommittedDegraded {
                value: (),
                commits: if present {
                    vec![sample(
                        json!({"operation_id": "op-1", "domain": "profiles", "source_version": 3, "runtime": "applied"}),
                    )]
                } else {
                    vec![]
                },
                notifications_pending: present,
                degradations: if present {
                    vec![degradation(present)]
                } else {
                    vec![]
                },
            },
            Some(("status", "committed_degraded")),
            &[
                "status",
                "value",
                "commits",
                "notifications_pending",
                "degradations",
            ],
        );
        check(
            receive::Degradation {
                phase: sample(json!("runtime_apply")),
                reason: degradation_reason(present),
                message: "field-13/路徑\\file.yaml".to_owned(),
                retryable: present,
            },
            None,
            &["phase", "reason", "message", "retryable"],
        );
        check(
            receive::DegradationReason::RuntimeRecoveryRequired {
                operation_id: if present {
                    Some("field-14/路徑\\file.yaml".to_owned())
                } else {
                    None
                },
                cause: if present {
                    Some(std::sync::Arc::new(runtime_error(present)))
                } else {
                    None
                },
            },
            Some(("code", "runtime_recovery_required")),
            &["code", "operation_id", "cause"],
        );
        check(
            receive::DegradationReason::RuntimeDeferred {
                cause: std::sync::Arc::new(runtime_error(present)),
            },
            Some(("code", "runtime_deferred")),
            &["code", "cause"],
        );
        check(
            receive::DegradationReason::RuntimeProductPublishFailed {
                cause: std::sync::Arc::new(runtime_error(present)),
            },
            Some(("code", "runtime_product_publish_failed")),
            &["code", "cause"],
        );
        check(
            receive::DegradationReason::ServiceStopFailed {
                cause: std::sync::Arc::new(runtime_error(present)),
            },
            Some(("code", "service_stop_failed")),
            &["code", "cause"],
        );
        check(
            receive::DegradationReason::ModeInterruptionFailed {
                cause: sample(json!("timeout")),
            },
            Some(("code", "mode_interruption_failed")),
            &["code", "cause"],
        );
        check(
            receive::DegradationReason::ProfileInterruptionFailed {
                cause: sample(json!("timeout")),
            },
            Some(("code", "profile_interruption_failed")),
            &["code", "cause"],
        );
        check(
            receive::DegradationReason::ProxyInterruptionFailed {
                cause: sample(json!("timeout")),
            },
            Some(("code", "proxy_interruption_failed")),
            &["code", "cause"],
        );
        check(
            receive::DegradationReason::ProxyCacheRefreshFailed,
            Some(("code", "proxy_cache_refresh_failed")),
            &["code"],
        );
        check(
            receive::DegradationReason::JournalInvalid,
            Some(("code", "journal_invalid")),
            &["code"],
        );
        check(
            receive::DegradationReason::JobsJournalUnavailable,
            Some(("code", "jobs_journal_unavailable")),
            &["code"],
        );
        check(
            receive::DegradationReason::MaterializationDeferred,
            Some(("code", "materialization_deferred")),
            &["code"],
        );
        check(
            receive::DegradationReason::CleanupDeferred,
            Some(("code", "cleanup_deferred")),
            &["code"],
        );
        check(
            receive::DegradationReason::ProfileAutoActivationFailed {
                profile: sample(json!("profile-1")),
                cause: std::sync::Arc::new(profiles_error(present)),
            },
            Some(("code", "profile_auto_activation_failed")),
            &["code", "profile", "cause"],
        );
        check(
            receive::CommitAborted::WriteConfig {
                runtime: runtime_aftermath(present),
            },
            Some(("kind", "write_config")),
            &["kind", "runtime"],
        );
        check(
            receive::CommitAborted::RecoverAfterWriteFailure {
                runtime: runtime_aftermath(present),
            },
            Some(("kind", "recover_after_write_failure")),
            &["kind", "runtime"],
        );
        check(
            receive::CommitAborted::RuntimeRefused {
                errors: vec![std::sync::Arc::new(runtime_error(present))],
                runtime: runtime_aftermath(present),
            },
            Some(("kind", "runtime_refused")),
            &["kind", "errors", "runtime"],
        );
        check(
            receive::CommitAborted::RuntimeFailed {
                errors: vec![std::sync::Arc::new(runtime_error(present))],
                runtime: runtime_aftermath(present),
            },
            Some(("kind", "runtime_failed")),
            &["kind", "errors", "runtime"],
        );
        check(
            receive::CommitAborted::ValidateState {},
            Some(("kind", "validate_state")),
            &["kind"],
        );
        check(
            receive::RuntimeAftermath::Untouched,
            Some(("kind", "untouched")),
            &["kind"],
        );
        check(
            receive::RuntimeAftermath::RolledBack,
            Some(("kind", "rolled_back")),
            &["kind"],
        );
        check(
            receive::RuntimeAftermath::RollbackFailed {
                detail: std::sync::Arc::new(runtime_error(present)),
            },
            Some(("kind", "rollback_failed")),
            &["kind", "detail"],
        );
        check(
            receive::RuntimeAftermath::Unknown {
                detail: std::sync::Arc::new(runtime_error(present)),
            },
            Some(("kind", "unknown")),
            &["kind", "detail"],
        );
        check(
            receive::PortResolveError::ResolvePort {
                field: sample(json!("mixed")),
                source: pick_port_error(present),
            },
            Some(("kind", "resolve_port")),
            &["kind", "field", "source"],
        );
        check(
            receive::CoreSpecError::FindCoreBinary {
                core: sample(json!("mihomo")),
            },
            Some(("kind", "find_core_binary")),
            &["kind", "core"],
        );
        check(
            receive::PublishRuntimeError::CreateRuntimeDirectory {
                path: "field-15/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "create_runtime_directory")),
            &["kind", "path"],
        );
        check(
            receive::PublishRuntimeError::WriteRuntimeConfig {
                path: "field-16/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "write_runtime_config")),
            &["kind", "path"],
        );
        check(
            receive::RuntimeBuildError::StartScriptRunner {},
            Some(("kind", "start_script_runner")),
            &["kind"],
        );
        check(
            receive::RuntimeBuildError::ValidateProfiles { errors: vec![] },
            Some(("kind", "validate_profiles")),
            &["kind", "errors"],
        );
        check(
            receive::RuntimeBuildError::RunPipeline {
                source: runtime_pipeline_error(present),
            },
            Some(("kind", "run_pipeline")),
            &["kind", "source"],
        );
        check(
            receive::RuntimeBuildError::TransformsFailed {
                failures: vec![
                    sample(json!({"kind":"profile", "id":"script-1"})),
                    sample(json!({"kind":"builtin", "name":"builtin"})),
                ],
                logs: vec![sample(
                    json!({"tag":{"kind":"bare_root"}, "entries":[{"level":"error", "message":"failed"}]}),
                )],
            },
            Some(("kind", "transforms_failed")),
            &["kind", "failures", "logs"],
        );
        check(
            receive::RuntimeBuildError::SerializeFinalConfig {},
            Some(("kind", "serialize_final_config")),
            &["kind"],
        );
        check(
            receive::RuntimeBuildError::ConfigNotMapping,
            Some(("kind", "config_not_mapping")),
            &["kind"],
        );
        check(
            receive::RuntimeBuildError::UnsupportedConfigValues {
                values: if present {
                    vec![sample(
                        json!({"path": "tun.stack", "value": "bad", "supported": ["system"]}),
                    )]
                } else {
                    vec![]
                },
            },
            Some(("kind", "unsupported_config_values")),
            &["kind", "values"],
        );
        check(
            receive::RuntimePipelineError::ExecuteScript {
                source: script_execution_error(present),
            },
            Some(("kind", "execute_script")),
            &["kind", "source"],
        );
        check(
            receive::RuntimePipelineError::SelectedProfileNotFound {
                profile: sample(json!("profile-1")),
            },
            Some(("kind", "selected_profile_not_found")),
            &["kind", "profile"],
        );
        check(
            receive::RuntimePipelineError::SelectedProfileNotConfig {
                profile: sample(json!("profile-1")),
            },
            Some(("kind", "selected_profile_not_config")),
            &["kind", "profile"],
        );
        check(
            receive::RuntimePipelineError::CompositionMemberInvalid {
                composition: sample(json!("profile-1")),
                member: sample(json!("profile-1")),
                reason: "field-17/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "composition_member_invalid")),
            &["kind", "composition", "member", "reason"],
        );
        check(
            receive::RuntimePipelineError::ContentSource {
                profile: sample(json!("profile-1")),
                path: sample(json!("profile.yaml")),
            },
            Some(("kind", "content_source")),
            &["kind", "profile", "path"],
        );
        check(
            receive::RuntimePipelineError::ParseProfile {
                profile: sample(json!("profile-1")),
                message: "field-18/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "parse_profile")),
            &["kind", "profile", "message"],
        );
        check(
            receive::RuntimePipelineError::Snapshot {},
            Some(("kind", "snapshot")),
            &["kind"],
        );
        check(
            receive::RuntimePipelineError::Internal {
                message: "field-19/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "internal")),
            &["kind", "message"],
        );
        check(
            receive::ScriptExecutionError::StartEngine {
                runtime: sample(json!("javascript")),
                operation: sample(json!("transform")),
            },
            Some(("kind", "start_engine")),
            &["kind", "runtime", "operation"],
        );
        check(
            receive::PickPortError::PortNotAvailable { port: 7 },
            Some(("kind", "port_not_available")),
            &["kind", "port"],
        );
        check(
            receive::PickPortError::NoOpenPort,
            Some(("kind", "no_open_port")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::PreserveDerivedDirectory {},
            Some(("kind", "preserve_derived_directory")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::InspectPath {
                path: "field-20/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "inspect_path")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::ReadFile {
                path: "field-21/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "read_file")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::ReadLink {
                path: "field-22/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "read_link")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::WriteFile {
                path: "field-23/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "write_file")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::AtomicWrite {
                path: "field-24/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "atomic_write")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::CreateDirectory {
                path: "field-25/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "create_directory")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::RemoveFile {
                path: "field-26/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "remove_file")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::ReplaceFile {
                from: "field-27/路徑\\file.yaml".to_owned(),
                to: "field-28/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "replace_file")),
            &["kind", "from", "to"],
        );
        check(
            receive::ProfileFileError::CreateSymlink {
                link: "field-29/路徑\\file.yaml".to_owned(),
                target: "field-30/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "create_symlink")),
            &["kind", "link", "target"],
        );
        check(
            receive::ProfileFileError::ListDirectory {
                path: "field-31/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "list_directory")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::SyncDirectory {
                path: "field-32/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "sync_directory")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::SetPermissions {
                path: "field-33/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "set_permissions")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::CanonicalizePath {
                path: "field-34/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "canonicalize_path")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::ReadExternalTarget {
                target: sample(json!("/profiles/source.yaml")),
            },
            Some(("kind", "read_external_target")),
            &["kind", "target"],
        );
        check(
            receive::ProfileFileError::ParseJournal {
                path: "field-35/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "parse_journal")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::SerializeJournal {},
            Some(("kind", "serialize_journal")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::ReservedPath {
                path: "field-36/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "reserved_path")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::PathEscapesProfilesDir {
                path: "field-37/路徑\\file.yaml".to_owned(),
                root: "field-38/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "path_escapes_profiles_dir")),
            &["kind", "path", "root"],
        );
        check(
            receive::ProfileFileError::NoParentDirectory {
                path: "field-39/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "no_parent_directory")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::UnexpectedNode {
                path: "field-40/路徑\\file.yaml".to_owned(),
                expected: sample(json!("regular_file")),
            },
            Some(("kind", "unexpected_node")),
            &["kind", "path", "expected"],
        );
        check(
            receive::ProfileFileError::UnexpectedSymlink {
                path: "field-41/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "unexpected_symlink")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::ExistingFileBlocksSymlink {
                path: "field-42/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "existing_file_blocks_symlink")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::SymlinkTargetNotUtf8 {
                path: "field-43/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "symlink_target_not_utf8")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::InvalidExternalPath {},
            Some(("kind", "invalid_external_path")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::CleanupTombstoneExists {
                path: "field-44/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "cleanup_tombstone_exists")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::JournalNotFound {
                operation_id: "field-45/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "journal_not_found")),
            &["kind", "operation_id"],
        );
        check(
            receive::ProfileFileError::CompensationFenced {
                path: "field-46/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "compensation_fenced")),
            &["kind", "path"],
        );
        check(
            receive::ProfileFileError::OperationIdInvalid,
            Some(("kind", "operation_id_invalid")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::OperationIdExhausted,
            Some(("kind", "operation_id_exhausted")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::JournalOperationIdMismatch,
            Some(("kind", "journal_operation_id_mismatch")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::JournalHashInvalid,
            Some(("kind", "journal_hash_invalid")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::MixedTransactionFamilies,
            Some(("kind", "mixed_transaction_families")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::ConflictingJournalPayloads,
            Some(("kind", "conflicting_journal_payloads")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::JournalDestinationDiffers,
            Some(("kind", "journal_destination_differs")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::ConflictingCleanupPayloads,
            Some(("kind", "conflicting_cleanup_payloads")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::MultipleStagedResources,
            Some(("kind", "multiple_staged_resources")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::MultipleBackups,
            Some(("kind", "multiple_backups")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::StagedHashMismatch,
            Some(("kind", "staged_hash_mismatch")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::ReadySymlinkMismatch,
            Some(("kind", "ready_symlink_mismatch")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::StagedResourceMissing,
            Some(("kind", "staged_resource_missing")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::PromotedHashMismatch,
            Some(("kind", "promoted_hash_mismatch")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::CompensatingCannotPromote,
            Some(("kind", "compensating_cannot_promote")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::TargetHashMismatch,
            Some(("kind", "target_hash_mismatch")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::NotCompletable,
            Some(("kind", "not_completable")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::CleanupAlreadyActivated,
            Some(("kind", "cleanup_already_activated")),
            &["kind"],
        );
        check(
            receive::ProfileFileError::DivergedBeforeRecovery,
            Some(("kind", "diverged_before_recovery")),
            &["kind"],
        );
        check(
            receive::SubscriptionFetchError::BuildHttpClient {},
            Some(("kind", "build_http_client")),
            &["kind"],
        );
        check(
            receive::SubscriptionFetchError::RequestSubscription {},
            Some(("kind", "request_subscription")),
            &["kind"],
        );
        check(
            receive::SubscriptionFetchError::SubscriptionHttpStatus { status: 7 },
            Some(("kind", "subscription_http_status")),
            &["kind", "status"],
        );
        check(
            receive::SubscriptionFetchError::ReadSubscriptionBody {},
            Some(("kind", "read_subscription_body")),
            &["kind"],
        );
        check(
            receive::InstallCoreBinaryError::AuthorizeCoreCopy {
                core: sample(json!("mihomo")),
                destination: "field-47/路徑\\file.yaml".to_owned(),
                source: elevation_consent_error(present),
            },
            Some(("kind", "authorize_core_copy")),
            &["kind", "core", "destination", "source"],
        );
        check(
            receive::InstallCoreBinaryError::StartElevatedCopy {
                core: sample(json!("mihomo")),
                destination: "field-48/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "start_elevated_copy")),
            &["kind", "core", "destination"],
        );
        check(
            receive::InstallCoreBinaryError::ElevatedCopyFailed {
                core: sample(json!("mihomo")),
                destination: "field-49/路徑\\file.yaml".to_owned(),
                exit_code: if present { Some(7) } else { None },
            },
            Some(("kind", "elevated_copy_failed")),
            &["kind", "core", "destination", "exit_code"],
        );
        check(
            receive::InstallCoreBinaryError::PathNotUtf8 {
                path: "field-50/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "path_not_utf8")),
            &["kind", "path"],
        );
        check(
            receive::CoreVersionError::RunCoreVersion {
                core: sample(json!("mihomo")),
            },
            Some(("kind", "run_core_version")),
            &["kind", "core"],
        );
        check(
            receive::CoreVersionError::CoreVersionExit {
                core: sample(json!("mihomo")),
            },
            Some(("kind", "core_version_exit")),
            &["kind", "core"],
        );
        check(
            receive::CoreVersionError::CoreVersionNotReported {
                core: sample(json!("mihomo")),
            },
            Some(("kind", "core_version_not_reported")),
            &["kind", "core"],
        );
        check(
            receive::ServiceCommandError::AuthorizeServiceCommand {
                command: sample(json!("start")),
                source: elevation_consent_error(present),
            },
            Some(("kind", "authorize_service_command")),
            &["kind", "command", "source"],
        );
        check(
            receive::ServiceCommandError::SessionEnding,
            Some(("kind", "session_ending")),
            &["kind"],
        );
        check(
            receive::ServiceCommandError::ResolveServiceUser {},
            Some(("kind", "resolve_service_user")),
            &["kind"],
        );
        check(
            receive::ServiceCommandError::ResolveServiceDirs {},
            Some(("kind", "resolve_service_dirs")),
            &["kind"],
        );
        check(
            receive::ServiceCommandError::RunElevated {
                command: sample(json!("start")),
            },
            Some(("kind", "run_elevated")),
            &["kind", "command"],
        );
        check(
            receive::ServiceCommandError::ServiceCommandExit {
                command: sample(json!("start")),
                exit_code: if present { Some(7) } else { None },
                signal: if present { Some(7) } else { None },
            },
            Some(("kind", "service_command_exit")),
            &["kind", "command", "exit_code", "signal"],
        );
        check(
            receive::ServiceCommandError::RunServiceStatus {},
            Some(("kind", "run_service_status")),
            &["kind"],
        );
        check(
            receive::ServiceCommandError::ServiceStatusExit {
                exit_code: if present { Some(7) } else { None },
                signal: if present { Some(7) } else { None },
            },
            Some(("kind", "service_status_exit")),
            &["kind", "exit_code", "signal"],
        );
        check(
            receive::ServiceCommandError::ServiceStatusNotUtf8 {},
            Some(("kind", "service_status_not_utf8")),
            &["kind"],
        );
        check(
            receive::ServiceCommandError::ParseServiceStatus {},
            Some(("kind", "parse_service_status")),
            &["kind"],
        );
        check(
            receive::ServiceCommandError::TimedOut { limit_ms: 7 },
            Some(("kind", "timed_out")),
            &["kind", "limit_ms"],
        );
        check(
            receive::ServiceCommandError::StillRunning { limit_ms: 7 },
            Some(("kind", "still_running")),
            &["kind", "limit_ms"],
        );
        check(
            receive::ServiceCommandError::TaskCancelled,
            Some(("kind", "task_cancelled")),
            &["kind"],
        );
        check(
            receive::ElevationConsentError::AuthorizeElevation {
                operation: sample(json!({"kind": "dns_flush"})),
            },
            Some(("kind", "authorize_elevation")),
            &["kind", "operation"],
        );
        check(
            receive::SystemDnsError::AuthorizeDnsFlush {
                source: elevation_consent_error(present),
            },
            Some(("kind", "authorize_dns_flush")),
            &["kind", "source"],
        );
        check(
            receive::SystemDnsError::RunFlushCommand {
                command: "field-51/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "run_flush_command")),
            &["kind", "command"],
        );
        check(
            receive::SystemDnsError::FlushRejected {
                command: "field-52/路徑\\file.yaml".to_owned(),
                code: if present { Some(7) } else { None },
            },
            Some(("kind", "flush_rejected")),
            &["kind", "command", "code"],
        );
        check(
            receive::SystemDnsError::Unsupported,
            Some(("kind", "unsupported")),
            &["kind"],
        );
        check(
            receive::OsProxyError::ReadOsProxy {},
            Some(("kind", "read_os_proxy")),
            &["kind"],
        );
        check(
            receive::OsProxyError::WriteOsProxy {
                enable: present,
                host: "field-53/路徑\\file.yaml".to_owned(),
                port: 6,
            },
            Some(("kind", "write_os_proxy")),
            &["kind", "enable", "host", "port"],
        );
        check(
            receive::ConfigError::LeaveNightlyChannel {
                to: sample(json!("stable")),
            },
            Some(("kind", "leave_nightly_channel")),
            &["kind", "to"],
        );
        check(
            receive::ConfigError::InvalidUpdateSources {
                reason: "field-54/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "invalid_update_sources")),
            &["kind", "reason"],
        );
        check(
            receive::ConfigError::InvalidCoreLogs {
                reason: "field-55/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "invalid_core_logs")),
            &["kind", "reason"],
        );
        check(
            receive::ConfigError::InvalidPublicIpProbeSources {
                reason: "field-56/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "invalid_public_ip_probe_sources")),
            &["kind", "reason"],
        );
        check(
            receive::ConfigError::InvalidLatencyTimeout {
                reason: "field-57/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "invalid_latency_timeout")),
            &["kind", "reason"],
        );
        check(
            receive::ConfigError::ValidateHotkeys {
                source: hotkey_parse_error(present),
            },
            Some(("kind", "validate_hotkeys")),
            &["kind", "source"],
        );
        check(
            receive::ConfigError::ValidateHotkeyCapability,
            Some(("kind", "validate_hotkey_capability")),
            &["kind"],
        );
        check(
            receive::ConfigError::WorkflowNotReady {},
            Some(("kind", "workflow_not_ready")),
            &["kind"],
        );
        check(
            receive::ConfigError::ShuttingDown {
                domain: sample(json!("profiles")),
            },
            Some(("kind", "shutting_down")),
            &["kind", "domain"],
        );
        check(
            receive::ConfigError::OwnerStopped {
                domain: sample(json!("profiles")),
            },
            Some(("kind", "owner_stopped")),
            &["kind", "domain"],
        );
        check(
            receive::ConfigError::VersionConflict {
                domain: sample(json!("profiles")),
                expected: 7,
                actual: 8,
            },
            Some(("kind", "version_conflict")),
            &["kind", "domain", "expected", "actual"],
        );
        check(
            receive::ConfigError::Commit {
                domain: sample(json!("profiles")),
                source: commit_aborted(present),
            },
            Some(("kind", "commit")),
            &["kind", "domain", "source"],
        );
        check(
            receive::ConfigError::PersistSessionState {},
            Some(("kind", "persist_session_state")),
            &["kind"],
        );
        check(
            receive::ConfigError::SessionStateStopped,
            Some(("kind", "session_state_stopped")),
            &["kind"],
        );
        check(
            receive::StorageOperationError::OpenDatabase {
                path: "field-58/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "open_database")),
            &["kind", "path"],
        );
        check(
            receive::StorageOperationError::InspectDatabase {
                path: "field-59/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "inspect_database")),
            &["kind", "path"],
        );
        check(
            receive::StorageOperationError::PreserveDatabase {},
            Some(("kind", "preserve_database")),
            &["kind"],
        );
        check(
            receive::StorageOperationError::BeginTransaction {},
            Some(("kind", "begin_transaction")),
            &["kind"],
        );
        check(
            receive::StorageOperationError::OpenTable {},
            Some(("kind", "open_table")),
            &["kind"],
        );
        check(
            receive::StorageOperationError::ReadItem {
                key: "field-60/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "read_item")),
            &["kind", "key"],
        );
        check(
            receive::StorageOperationError::WriteItem {
                key: "field-61/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "write_item")),
            &["kind", "key"],
        );
        check(
            receive::StorageOperationError::RemoveItem {
                key: "field-62/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "remove_item")),
            &["kind", "key"],
        );
        check(
            receive::StorageOperationError::ListItems {},
            Some(("kind", "list_items")),
            &["kind"],
        );
        check(
            receive::StorageOperationError::CommitTransaction {},
            Some(("kind", "commit_transaction")),
            &["kind"],
        );
        check(
            receive::StorageOperationError::DecodeValue {
                key: "field-63/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "decode_value")),
            &["kind", "key"],
        );
        check(
            receive::StorageOperationError::EncodeValue {
                key: "field-64/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "encode_value")),
            &["kind", "key"],
        );
        check(
            receive::HotkeyAction::OpenOrCloseDashboard,
            Some(("", "open_or_close_dashboard")),
            &[],
        );
        check(
            receive::HotkeyAction::ClashModeRule,
            Some(("", "clash_mode_rule")),
            &[],
        );
        check(
            receive::HotkeyAction::ClashModeGlobal,
            Some(("", "clash_mode_global")),
            &[],
        );
        check(
            receive::HotkeyAction::ClashModeDirect,
            Some(("", "clash_mode_direct")),
            &[],
        );
        check(
            receive::HotkeyAction::ClashModeScript,
            Some(("", "clash_mode_script")),
            &[],
        );
        check(
            receive::HotkeyAction::ToggleSystemProxy,
            Some(("", "toggle_system_proxy")),
            &[],
        );
        check(
            receive::HotkeyAction::EnableSystemProxy,
            Some(("", "enable_system_proxy")),
            &[],
        );
        check(
            receive::HotkeyAction::DisableSystemProxy,
            Some(("", "disable_system_proxy")),
            &[],
        );
        check(
            receive::HotkeyAction::ToggleTunMode,
            Some(("", "toggle_tun_mode")),
            &[],
        );
        check(
            receive::HotkeyAction::EnableTunMode,
            Some(("", "enable_tun_mode")),
            &[],
        );
        check(
            receive::HotkeyAction::DisableTunMode,
            Some(("", "disable_tun_mode")),
            &[],
        );
        check(
            receive::HotkeyParseError::MalformedEntry {
                entry: "field-65/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "malformed_entry")),
            &["kind", "entry"],
        );
        check(
            receive::HotkeyParseError::UnknownFunction {
                function: "field-66/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "unknown_function")),
            &["kind", "function"],
        );
        check(
            receive::HotkeyParseError::EmptyKeySegment {
                accelerator: "field-67/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "empty_key_segment")),
            &["kind", "accelerator"],
        );
        check(
            receive::HotkeyParseError::UnsupportedAccelerator {
                accelerator: "field-68/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "unsupported_accelerator")),
            &["kind", "accelerator"],
        );
        check(
            receive::HotkeyParseError::MissingSuperKey {
                accelerator: "field-69/路徑\\file.yaml".to_owned(),
            },
            Some(("kind", "missing_super_key")),
            &["kind", "accelerator"],
        );
        check(
            receive::HotkeyParseError::DuplicateAccelerator {
                accelerator: "field-70/路徑\\file.yaml".to_owned(),
                first: hotkey_action(present),
                second: hotkey_action(present),
            },
            Some(("kind", "duplicate_accelerator")),
            &["kind", "accelerator", "first", "second"],
        );
        check(
            receive::EffectsError::EffectsStopped,
            Some(("kind", "effects_stopped")),
            &["kind"],
        );
        check(
            receive::DebugHttpControlError::ReadStatusUnsupported,
            Some(("kind", "read_status_unsupported")),
            &["kind"],
        );
        check(
            receive::DebugHttpControlError::ChangeEnabledUnsupported,
            Some(("kind", "change_enabled_unsupported")),
            &["kind"],
        );
        check(
            receive::DebugHttpControlError::ReadStatus {},
            Some(("kind", "read_status")),
            &["kind"],
        );
        check(
            receive::DebugHttpControlError::ChangeEnabled {},
            Some(("kind", "change_enabled")),
            &["kind"],
        );
        check(
            receive::IpcError {
                kind: ipc_error_kind(present),
                message: "field-71/路徑\\file.yaml".to_owned(),
                detail: "field-72/路徑\\file.yaml".to_owned(),
            },
            None,
            &["kind", "message", "detail"],
        );
        check(
            receive::IpcErrorKind::Unknown,
            Some(("domain", "unknown")),
            &["domain"],
        );
        check(
            receive::IpcErrorKind::Profiles(Box::new(profiles_error(present))),
            Some(("domain", "profiles")),
            &["domain", "error"],
        );
        check(
            receive::IpcErrorKind::Runtime(Box::new(runtime_error(present))),
            Some(("domain", "runtime")),
            &["domain", "error"],
        );
        check(
            receive::IpcErrorKind::Config(Box::new(config_error(present))),
            Some(("domain", "config")),
            &["domain", "error"],
        );
        check(
            receive::IpcErrorKind::Storage(Box::new(storage_operation_error(present))),
            Some(("domain", "storage")),
            &["domain", "error"],
        );
        check(
            receive::IpcErrorKind::SystemDns(Box::new(system_dns_error(present))),
            Some(("domain", "system_dns")),
            &["domain", "error"],
        );
        check(
            receive::IpcErrorKind::SystemProxy(Box::new(os_proxy_error(present))),
            Some(("domain", "system_proxy")),
            &["domain", "error"],
        );
        check(
            receive::IpcErrorKind::Effects(Box::new(effects_error(present))),
            Some(("domain", "effects")),
            &["domain", "error"],
        );
        check(
            receive::IpcErrorKind::DebugHttp(Box::new(debug_http_control_error(present))),
            Some(("domain", "debug_http")),
            &["domain", "error"],
        );
    }
}

fn runtime_error(present: bool) -> receive::RuntimeError {
    let _ = present;
    receive::RuntimeError::ShuttingDown
}

fn restore_failure(present: bool) -> receive::RestoreFailure {
    let _ = present;
    receive::RestoreFailure::MoveHostBack {
        host: sample(json!(if present { "service" } else { "local" })),
        failure: sample(
            json!({"kind": null, "message": "core failed", "retryable": true, "operation_id": null}),
        ),
    }
}

fn profile_content_error(present: bool) -> receive::ProfileContentError {
    let _ = present;
    receive::ProfileContentError::NotYamlMapping {}
}

fn profiles_error(present: bool) -> receive::ProfilesError {
    let _ = present;
    receive::ProfilesError::ProfileNotFound {
        uid: sample(json!("profile-1")),
    }
}

fn degradation(present: bool) -> receive::Degradation {
    let _ = present;
    receive::Degradation {
        phase: sample(json!("runtime_apply")),
        reason: degradation_reason(present),
        message: "field-73/路徑\\file.yaml".to_owned(),
        retryable: present,
    }
}

fn degradation_reason(present: bool) -> receive::DegradationReason {
    let _ = present;
    receive::DegradationReason::RuntimeRecoveryRequired {
        operation_id: if present {
            Some("field-74/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        cause: if present {
            Some(std::sync::Arc::new(runtime_error(present)))
        } else {
            None
        },
    }
}

fn commit_aborted(present: bool) -> receive::CommitAborted {
    let _ = present;
    receive::CommitAborted::WriteConfig {
        runtime: runtime_aftermath(present),
    }
}

fn runtime_aftermath(present: bool) -> receive::RuntimeAftermath {
    let _ = present;
    receive::RuntimeAftermath::Untouched
}

fn port_resolve_error(present: bool) -> receive::PortResolveError {
    let _ = present;
    receive::PortResolveError::ResolvePort {
        field: sample(json!("mixed")),
        source: pick_port_error(present),
    }
}

fn core_spec_error(present: bool) -> receive::CoreSpecError {
    let _ = present;
    receive::CoreSpecError::FindCoreBinary {
        core: sample(json!("mihomo")),
    }
}

fn publish_runtime_error(present: bool) -> receive::PublishRuntimeError {
    let _ = present;
    receive::PublishRuntimeError::CreateRuntimeDirectory {
        path: "field-75/路徑\\file.yaml".to_owned(),
    }
}

fn runtime_build_error(present: bool) -> receive::RuntimeBuildError {
    let _ = present;
    receive::RuntimeBuildError::StartScriptRunner {}
}

fn runtime_pipeline_error(present: bool) -> receive::RuntimePipelineError {
    let _ = present;
    receive::RuntimePipelineError::ExecuteScript {
        source: script_execution_error(present),
    }
}

fn script_execution_error(present: bool) -> receive::ScriptExecutionError {
    let _ = present;
    receive::ScriptExecutionError::StartEngine {
        runtime: sample(json!("javascript")),
        operation: sample(json!("transform")),
    }
}

fn pick_port_error(present: bool) -> receive::PickPortError {
    let _ = present;
    receive::PickPortError::PortNotAvailable { port: 7 }
}

fn profile_file_error(present: bool) -> receive::ProfileFileError {
    let _ = present;
    receive::ProfileFileError::PreserveDerivedDirectory {}
}

fn subscription_fetch_error(present: bool) -> receive::SubscriptionFetchError {
    let _ = present;
    receive::SubscriptionFetchError::BuildHttpClient {}
}

fn install_core_binary_error(present: bool) -> receive::InstallCoreBinaryError {
    let _ = present;
    receive::InstallCoreBinaryError::AuthorizeCoreCopy {
        core: sample(json!("mihomo")),
        destination: "field-76/路徑\\file.yaml".to_owned(),
        source: elevation_consent_error(present),
    }
}

fn core_version_error(present: bool) -> receive::CoreVersionError {
    let _ = present;
    receive::CoreVersionError::RunCoreVersion {
        core: sample(json!("mihomo")),
    }
}

fn service_command_error(present: bool) -> receive::ServiceCommandError {
    let _ = present;
    receive::ServiceCommandError::AuthorizeServiceCommand {
        command: sample(json!("start")),
        source: elevation_consent_error(present),
    }
}

fn elevation_consent_error(present: bool) -> receive::ElevationConsentError {
    let _ = present;
    receive::ElevationConsentError::AuthorizeElevation {
        operation: sample(json!({"kind": "dns_flush"})),
    }
}

fn system_dns_error(present: bool) -> receive::SystemDnsError {
    let _ = present;
    receive::SystemDnsError::AuthorizeDnsFlush {
        source: elevation_consent_error(present),
    }
}

fn os_proxy_error(present: bool) -> receive::OsProxyError {
    let _ = present;
    receive::OsProxyError::ReadOsProxy {}
}

fn config_error(present: bool) -> receive::ConfigError {
    let _ = present;
    receive::ConfigError::LeaveNightlyChannel {
        to: sample(json!("stable")),
    }
}

fn storage_operation_error(present: bool) -> receive::StorageOperationError {
    let _ = present;
    receive::StorageOperationError::OpenDatabase {
        path: "field-77/路徑\\file.yaml".to_owned(),
    }
}

fn hotkey_action(present: bool) -> receive::HotkeyAction {
    let _ = present;
    receive::HotkeyAction::OpenOrCloseDashboard
}

fn hotkey_parse_error(present: bool) -> receive::HotkeyParseError {
    let _ = present;
    receive::HotkeyParseError::MalformedEntry {
        entry: "field-78/路徑\\file.yaml".to_owned(),
    }
}

fn effects_error(present: bool) -> receive::EffectsError {
    let _ = present;
    receive::EffectsError::EffectsStopped
}

fn debug_http_control_error(present: bool) -> receive::DebugHttpControlError {
    let _ = present;
    receive::DebugHttpControlError::ReadStatusUnsupported
}

fn ipc_error_kind(present: bool) -> receive::IpcErrorKind {
    let _ = present;
    receive::IpcErrorKind::Unknown
}
