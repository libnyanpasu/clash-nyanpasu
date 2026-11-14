//! Typed, boundary-enforced error privacy projections for callers lacking
//! `Scope::SensitiveRead`.
//!
//! Projections happen on typed domain values before JSON serialization, never by
//! regex scrubbing or key filtering on serialized payloads.

use nyanpasu_config::{
    application::RemoteAccessError,
    clash::config::{
        TransparentProxyMode, TransparentProxyValidationError, clash_strategy::PickPortError,
    },
    runtime::executor::RuntimePipelineError,
};
use nyanpasu_contracts::{
    core_install::InstallCoreBinaryError,
    profiles::error::{ProfileFileError, SubscriptionFetchError},
    system_dns::SystemDnsError,
    system_proxy::OsProxyError,
};
use nyanpasu_core::{
    client::{
        ClashReadError, CoreLogReadError, ProfileSyncReadError, http_server::HttpServerControlError,
    },
    state::config_error::ConfigError,
    storage::StorageOperationError,
};
use nyanpasu_logging::LogError;
use nyanpasu_observers::logs::CoreLogError;
use nyanpasu_profile_transform::RuntimeBuildError;
use nyanpasu_rpc_protocol::{
    ResponseVisibility, RpcError, RpcValue,
    types::{
        PublicClashReadError, PublicCommitAborted, PublicCommitAbortedKind, PublicConfigError,
        PublicCoreLogReadError, PublicEffectsError, PublicFailure, PublicHotkeyErrorKind,
        PublicHttpServerError, PublicInstallCoreBinaryKind, PublicOsProxyError,
        PublicPortResolveKind, PublicProfileContentKind, PublicProfileSyncReadError,
        PublicProfilesError, PublicRemoteAccessErrorKind, PublicRuntimeAftermathKind,
        PublicRuntimeBuildKind, PublicRuntimeCheckUnavailableKind, PublicRuntimeError,
        PublicRuntimePipelineKind, PublicStorageError, PublicSubscriptionFetchKind,
        PublicSystemDnsError, PublicTransparentProxyMode, PublicTransparentProxyValidationKind,
    },
};
use nyanpasu_runtime_values::{
    check::RuntimeCheckUnavailable,
    effects::EffectsError,
    hotkey::HotkeyParseError,
    mutation::{CommitAborted, RuntimeAftermath},
    ports::PortResolveError,
    profiles::{ProfileContentError, ProfilesError},
    runtime_error::RuntimeError,
};

use crate::error::{IpcError, IpcErrorKind};

/// Fixed non-sensitive messages for public RPC errors.
const PUBLIC_INTERNAL_MESSAGE: &str = "internal error";

/// Convert any command failure into an `RpcError` matching `visibility`.
///
/// `ResponseVisibility::Full` preserves full diagnostics and original error text.
/// `ResponseVisibility::Public` walks `.source()` before building any envelope so
/// raw error messages, filesystem paths, URLs, credentials, and host strings never
/// escape, while retaining known domain/variant discriminants, HTTP status codes,
/// `CoreErrorKind`, `retryable`, `operation_id`, and commit aftermath metadata.
pub(crate) fn application_error_for(
    visibility: ResponseVisibility,
    error: &(dyn std::error::Error + 'static),
) -> RpcError {
    match visibility {
        ResponseVisibility::Full => full_application_error(error),
        ResponseVisibility::Public => public_application_error(error),
    }
}

fn full_application_error(error: &(dyn std::error::Error + 'static)) -> RpcError {
    let mut result = RpcError::new("application_error", error.to_string());
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if let Some(log) = current.downcast_ref::<LogError>() {
            result.domain_error = serde_json::to_value(log)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        if let Some(log) = current.downcast_ref::<CoreLogError>() {
            result.domain_error = serde_json::to_value(log)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        if let Some(core) = current.downcast_ref::<nyanpasu_core_manager::CoreError>() {
            result.code = core.kind.map(|kind| kind.to_string());
            result.retryable = Some(core.retryable);
            result.operation_id = core.operation_id.map(|id| id.to_string());
        }
        if let Some(ipc) = current.downcast_ref::<IpcError>() {
            result.domain_error = serde_json::to_value(ipc)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        source = current.source();
    }
    result
}

enum PublicSourceMatch {
    Log(LogError),
    CoreLog(CoreLogError),
    Ipc(IpcError),
    CoreOnly(nyanpasu_core_manager::CoreErrorKind),
}

fn public_application_error(error: &(dyn std::error::Error + 'static)) -> RpcError {
    let mut result = RpcError::new("application_error", PUBLIC_INTERNAL_MESSAGE);
    let mut matched: Option<PublicSourceMatch> = None;

    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if let Some(log) = current.downcast_ref::<LogError>() {
            matched = Some(PublicSourceMatch::Log(log.clone()));
        }
        if let Some(log) = current.downcast_ref::<CoreLogError>() {
            matched = Some(PublicSourceMatch::CoreLog(log.clone()));
        }
        if let Some(core) = current.downcast_ref::<nyanpasu_core_manager::CoreError>() {
            result.code = core.kind.map(|kind| kind.to_string());
            result.retryable = Some(core.retryable);
            result.operation_id = core.operation_id.map(|id| id.to_string());
            if matched.is_none()
                && let Some(kind) = core.kind
            {
                matched = Some(PublicSourceMatch::CoreOnly(kind));
            }
        }
        if let Some(ipc) = current.downcast_ref::<IpcError>() {
            matched = Some(PublicSourceMatch::Ipc(project_public_ipc_error(ipc)));
        }
        source = current.source();
    }

    match matched {
        Some(PublicSourceMatch::Log(log)) => {
            // `LogError` Display arms are fixed static messages on unit variants.
            result.message = log.to_string();
            result.domain_error = serde_json::to_value(&log)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        Some(PublicSourceMatch::CoreLog(log)) => {
            let public_log = project_core_log_error(&log);
            let message = public_core_log_message(public_log).to_owned();
            let ipc = IpcError::public(PublicFailure::CoreLogRead(public_log), message.clone());
            result.message = message;
            result.domain_error = serde_json::to_value(ipc)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        Some(PublicSourceMatch::CoreOnly(kind)) => {
            let message = "core operation failed".to_owned();
            let ipc = IpcError::public(PublicFailure::Core { kind }, message.clone());
            result.message = message;
            result.domain_error = serde_json::to_value(ipc)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        Some(PublicSourceMatch::Ipc(ipc)) => {
            result.message = ipc.message_str().to_owned();
            result.domain_error = serde_json::to_value(ipc)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        None => {
            let ipc = IpcError::public_unknown(PUBLIC_INTERNAL_MESSAGE);
            result.domain_error = serde_json::to_value(ipc)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
    }

    result
}

fn project_public_ipc_error(ipc: &IpcError) -> IpcError {
    match ipc.kind_ref() {
        IpcErrorKind::Unknown => IpcError::public_unknown(PUBLIC_INTERNAL_MESSAGE),
        IpcErrorKind::Public(failure) => {
            let message = public_failure_message(failure).to_owned();
            IpcError::public((**failure).clone(), message)
        }
        IpcErrorKind::Profiles(error) => {
            let projected = PublicFailure::Profiles(project_profiles_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::Runtime(error) => {
            let projected = PublicFailure::Runtime(project_runtime_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::Config(error) => {
            let projected = PublicFailure::Config(project_config_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::Storage(error) => {
            let projected = PublicFailure::Storage(project_storage_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::SystemDns(error) => {
            let projected = PublicFailure::SystemDns(project_system_dns_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::SystemProxy(error) => {
            let projected = PublicFailure::SystemProxy(project_os_proxy_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::Effects(error) => {
            let projected = PublicFailure::Effects(project_effects_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::HttpServer(error) => {
            let projected = PublicFailure::HttpServer(project_http_server_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::ClashRead(error) => {
            let projected = PublicFailure::ClashRead(project_clash_read_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::CoreLogRead(error) => {
            let projected = PublicFailure::CoreLogRead(project_core_log_read_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
        IpcErrorKind::ProfileSyncRead(error) => {
            let projected = PublicFailure::ProfileSyncRead(project_profile_sync_read_error(error));
            let message = public_failure_message(&projected).to_owned();
            IpcError::public(projected, message)
        }
    }
}

fn public_failure_message(failure: &PublicFailure) -> &'static str {
    match failure {
        PublicFailure::Profiles(_) => "profile operation failed",
        PublicFailure::Runtime(_) => "runtime operation failed",
        PublicFailure::Config(_) => "configuration operation failed",
        PublicFailure::Storage(_) => "storage operation failed",
        PublicFailure::SystemDns(_) => "system DNS operation failed",
        PublicFailure::SystemProxy(_) => "system proxy operation failed",
        PublicFailure::Effects(_) => "effect application failed",
        PublicFailure::HttpServer(_) => "HTTP server operation failed",
        PublicFailure::ClashRead(_) => "Clash API read failed",
        PublicFailure::CoreLogRead(error) => public_core_log_message(*error),
        PublicFailure::ProfileSyncRead(_) => "profile sync read failed",
        PublicFailure::Core { .. } => "core operation failed",
    }
}

fn public_core_log_message(error: PublicCoreLogReadError) -> &'static str {
    match error {
        PublicCoreLogReadError::InvalidRequest => "invalid core log request",
        PublicCoreLogReadError::CursorExpired => "core log cursor expired",
        PublicCoreLogReadError::RecordGone => "core log record unavailable",
        PublicCoreLogReadError::TooLarge => "core log record too large",
        PublicCoreLogReadError::Unavailable => "core log storage unavailable",
    }
}

fn is_unsafe_profile_file_path(error: &ProfileFileError) -> bool {
    matches!(
        error,
        ProfileFileError::ReservedPath { .. }
            | ProfileFileError::PathEscapesProfilesDir { .. }
            | ProfileFileError::UnexpectedNode { .. }
            | ProfileFileError::UnexpectedSymlink { .. }
            | ProfileFileError::ExistingFileBlocksSymlink { .. }
            | ProfileFileError::SymlinkTargetNotUtf8 { .. }
    )
}

fn project_profile_content_error(error: &ProfileContentError) -> PublicProfileContentKind {
    match error {
        ProfileContentError::NotYamlMapping { .. } => PublicProfileContentKind::NotYamlMapping,
        ProfileContentError::ReserializeYaml { .. } => PublicProfileContentKind::ReserializeYaml,
        ProfileContentError::MissingProxies => PublicProfileContentKind::MissingProxies,
        ProfileContentError::EmptyScript => PublicProfileContentKind::EmptyScript,
    }
}

fn project_subscription_fetch_error(error: &SubscriptionFetchError) -> PublicSubscriptionFetchKind {
    match error {
        SubscriptionFetchError::BuildHttpClient { .. } => {
            PublicSubscriptionFetchKind::BuildHttpClient
        }
        SubscriptionFetchError::RequestSubscription { .. } => {
            PublicSubscriptionFetchKind::RequestSubscription
        }
        SubscriptionFetchError::SubscriptionHttpStatus { status } => {
            PublicSubscriptionFetchKind::SubscriptionHttpStatus { status: *status }
        }
        SubscriptionFetchError::ReadSubscriptionBody { .. } => {
            PublicSubscriptionFetchKind::ReadSubscriptionBody
        }
    }
}

fn project_profiles_error(error: &ProfilesError) -> PublicProfilesError {
    match error {
        ProfilesError::ProfileNotFound { .. } => PublicProfilesError::ProfileNotFound,
        ProfilesError::ProfileHasNoFile { .. } => PublicProfilesError::ProfileHasNoFile,
        ProfilesError::NotARemoteProfile { .. } => PublicProfilesError::NotARemoteProfile,
        ProfilesError::ProfileFileNotWritable { .. } => PublicProfilesError::ProfileFileNotWritable,
        ProfilesError::ProfileInUse { referrers, .. } => PublicProfilesError::ProfileInUse {
            referrer_count: referrers.len(),
        },
        ProfilesError::ProfileIdCollision { .. } => PublicProfilesError::ProfileIdCollision,
        ProfilesError::ValidationFailed { .. } => PublicProfilesError::ValidationFailed,
        ProfilesError::ReorderListSizeMismatch { .. } => {
            PublicProfilesError::ReorderListSizeMismatch
        }
        ProfilesError::ReorderListDuplicate { .. } => PublicProfilesError::ReorderListDuplicate,
        ProfilesError::RevisionOverflow { .. } => PublicProfilesError::RevisionOverflow,
        ProfilesError::InvalidSubscriptionUrl { .. } => PublicProfilesError::InvalidSubscriptionUrl,
        ProfilesError::RemoteProfileNeedsImport => PublicProfilesError::RemoteProfileNeedsImport,
        ProfilesError::RefreshInProgress { .. } => PublicProfilesError::RefreshInProgress,
        ProfilesError::FetchSubscription { source, .. } => PublicProfilesError::FetchSubscription {
            reason: project_subscription_fetch_error(source),
        },
        ProfilesError::ProfileContentRejected { source } => {
            PublicProfilesError::ProfileContentRejected {
                reason: project_profile_content_error(source),
            }
        }
        ProfilesError::ProfileFileNotYaml { .. } => PublicProfilesError::ProfileFileNotYaml,
        ProfilesError::ProfileDeletedDuringRefresh { .. } => {
            PublicProfilesError::ProfileDeletedDuringRefresh
        }
        ProfilesError::ProfileChangedDuringRefresh { .. } => {
            PublicProfilesError::ProfileChangedDuringRefresh
        }
        ProfilesError::FingerprintDefinition { .. } => PublicProfilesError::FingerprintDefinition,
        ProfilesError::ReadProfileFile { source, .. } => PublicProfilesError::ReadProfileFile {
            unsafe_path: is_unsafe_profile_file_path(source),
        },
        ProfilesError::ProfileFileMissing { .. } => PublicProfilesError::ProfileFileMissing,
        ProfilesError::ReadExternalProfile { source, .. } => {
            PublicProfilesError::ReadExternalProfile {
                unsafe_path: is_unsafe_profile_file_path(source),
            }
        }
        ProfilesError::Materialization { source, .. } => PublicProfilesError::Materialization {
            unsafe_path: is_unsafe_profile_file_path(source),
        },
        ProfilesError::VersionConflict { .. } => PublicProfilesError::VersionConflict,
        ProfilesError::Commit { source, .. } => PublicProfilesError::Commit {
            source: project_commit_aborted(source),
        },
        ProfilesError::WorkflowNotReady { .. } => PublicProfilesError::WorkflowNotReady,
        ProfilesError::ProfilesActorStopped => PublicProfilesError::ServiceStopped,
        ProfilesError::ProfilesReplyDropped => PublicProfilesError::ServiceStopped,
        ProfilesError::BlockingTaskCancelled { .. } => PublicProfilesError::ServiceStopped,
        ProfilesError::ShuttingDown => PublicProfilesError::ShuttingDown,
    }
}

fn project_runtime_aftermath(
    aftermath: &RuntimeAftermath,
) -> (PublicRuntimeAftermathKind, Option<Box<PublicRuntimeError>>) {
    match aftermath {
        RuntimeAftermath::Untouched => (PublicRuntimeAftermathKind::Untouched, None),
        RuntimeAftermath::RolledBack => (PublicRuntimeAftermathKind::RolledBack, None),
        RuntimeAftermath::RollbackFailed { detail } => (
            PublicRuntimeAftermathKind::RollbackFailed,
            Some(Box::new(project_runtime_error(detail))),
        ),
        RuntimeAftermath::Unknown { detail } => (
            PublicRuntimeAftermathKind::Unknown,
            Some(Box::new(project_runtime_error(detail))),
        ),
    }
}

fn project_commit_aborted(commit: &CommitAborted) -> PublicCommitAborted {
    match commit {
        CommitAborted::WriteConfig { runtime, .. } => {
            let (runtime, aftermath_cause) = project_runtime_aftermath(runtime);
            PublicCommitAborted {
                kind: PublicCommitAbortedKind::WriteConfig,
                runtime: Some(runtime),
                runtime_causes: Vec::new(),
                aftermath_cause,
            }
        }
        CommitAborted::RecoverAfterWriteFailure { runtime, .. } => {
            let (runtime, aftermath_cause) = project_runtime_aftermath(runtime);
            PublicCommitAborted {
                kind: PublicCommitAbortedKind::RecoverAfterWriteFailure,
                runtime: Some(runtime),
                runtime_causes: Vec::new(),
                aftermath_cause,
            }
        }
        CommitAborted::RuntimeRefused {
            errors, runtime, ..
        } => {
            let (runtime, aftermath_cause) = project_runtime_aftermath(runtime);
            PublicCommitAborted {
                kind: PublicCommitAbortedKind::RuntimeRefused,
                runtime: Some(runtime),
                runtime_causes: errors.iter().map(|e| project_runtime_error(e)).collect(),
                aftermath_cause,
            }
        }
        CommitAborted::RuntimeFailed {
            errors, runtime, ..
        } => {
            let (runtime, aftermath_cause) = project_runtime_aftermath(runtime);
            PublicCommitAborted {
                kind: PublicCommitAbortedKind::RuntimeFailed,
                runtime: Some(runtime),
                runtime_causes: errors.iter().map(|e| project_runtime_error(e)).collect(),
                aftermath_cause,
            }
        }
        CommitAborted::ValidateState { .. } => PublicCommitAborted {
            kind: PublicCommitAbortedKind::ValidateState,
            runtime: None,
            runtime_causes: Vec::new(),
            aftermath_cause: None,
        },
    }
}

fn project_transparent_proxy_mode(mode: TransparentProxyMode) -> PublicTransparentProxyMode {
    match mode {
        TransparentProxyMode::Disabled => PublicTransparentProxyMode::Disabled,
        TransparentProxyMode::Redir => PublicTransparentProxyMode::Redir,
        TransparentProxyMode::Tproxy => PublicTransparentProxyMode::Tproxy,
    }
}

fn project_transparent_proxy_validation(
    error: &TransparentProxyValidationError,
) -> PublicTransparentProxyValidationKind {
    match error {
        TransparentProxyValidationError::UnsupportedTarget => {
            PublicTransparentProxyValidationKind::UnsupportedTarget
        }
        TransparentProxyValidationError::TunConflict => {
            PublicTransparentProxyValidationKind::TunConflict
        }
        TransparentProxyValidationError::MissingListener { mode } => {
            PublicTransparentProxyValidationKind::MissingListener {
                mode: project_transparent_proxy_mode(*mode),
            }
        }
        TransparentProxyValidationError::ZeroPort => PublicTransparentProxyValidationKind::ZeroPort,
        TransparentProxyValidationError::EmptyTarget => {
            PublicTransparentProxyValidationKind::EmptyTarget
        }
        TransparentProxyValidationError::InvalidInterface { .. } => {
            PublicTransparentProxyValidationKind::InvalidInterface
        }
        TransparentProxyValidationError::DuplicateInterface => {
            PublicTransparentProxyValidationKind::DuplicateInterface
        }
        TransparentProxyValidationError::TooManyInterfaces => {
            PublicTransparentProxyValidationKind::TooManyInterfaces
        }
    }
}

fn project_runtime_pipeline_error(error: &RuntimePipelineError) -> PublicRuntimePipelineKind {
    match error {
        RuntimePipelineError::ExecuteScript { .. } => PublicRuntimePipelineKind::ExecuteScript,
        RuntimePipelineError::SelectedProfileNotFound { .. } => {
            PublicRuntimePipelineKind::SelectedProfileNotFound
        }
        RuntimePipelineError::SelectedProfileNotConfig { .. } => {
            PublicRuntimePipelineKind::SelectedProfileNotConfig
        }
        RuntimePipelineError::CompositionMemberInvalid { .. } => {
            PublicRuntimePipelineKind::CompositionMemberInvalid
        }
        RuntimePipelineError::ContentSource { .. } => PublicRuntimePipelineKind::ContentSource,
        RuntimePipelineError::ParseProfile { .. } => PublicRuntimePipelineKind::ParseProfile,
        RuntimePipelineError::Snapshot { .. } => PublicRuntimePipelineKind::Snapshot,
        RuntimePipelineError::Internal { .. } => PublicRuntimePipelineKind::Internal,
    }
}

fn project_runtime_build_error(error: &RuntimeBuildError) -> PublicRuntimeBuildKind {
    match error {
        RuntimeBuildError::StartScriptRunner { .. } => PublicRuntimeBuildKind::StartScriptRunner,
        RuntimeBuildError::ValidateProfiles { .. } => PublicRuntimeBuildKind::ValidateProfiles,
        RuntimeBuildError::ValidateTransparentProxy { source } => {
            PublicRuntimeBuildKind::ValidateTransparentProxy {
                reason: project_transparent_proxy_validation(source),
            }
        }
        RuntimeBuildError::TransparentProxyRequiresServiceMode => {
            PublicRuntimeBuildKind::TransparentProxyRequiresServiceMode
        }
        RuntimeBuildError::RunPipeline { source } => PublicRuntimeBuildKind::RunPipeline {
            stage: project_runtime_pipeline_error(source),
        },
        RuntimeBuildError::TransformsFailed { .. } => PublicRuntimeBuildKind::TransformsFailed,
        RuntimeBuildError::SerializeFinalConfig { .. } => {
            PublicRuntimeBuildKind::SerializeFinalConfig
        }
        RuntimeBuildError::ConfigNotMapping => PublicRuntimeBuildKind::ConfigNotMapping,
        RuntimeBuildError::UnsupportedConfigValues { .. } => {
            PublicRuntimeBuildKind::UnsupportedConfigValues
        }
    }
}

fn project_port_resolve_error(error: &PortResolveError) -> PublicPortResolveKind {
    match error {
        PortResolveError::ResolvePort { field, source } => match source {
            PickPortError::PortNotAvailable { port } => PublicPortResolveKind::PortNotAvailable {
                field: *field,
                port: *port,
            },
            PickPortError::NoOpenPort => PublicPortResolveKind::NoOpenPort { field: *field },
        },
        PortResolveError::PortConflict {
            first,
            second,
            port,
        } => PublicPortResolveKind::PortConflict {
            first: *first,
            second: *second,
            port: *port,
        },
    }
}

fn project_install_core_binary_error(
    error: &InstallCoreBinaryError,
) -> PublicInstallCoreBinaryKind {
    match error {
        InstallCoreBinaryError::AuthorizeCoreCopy { core, .. } => {
            PublicInstallCoreBinaryKind::AuthorizeCoreCopy { core: *core }
        }
        InstallCoreBinaryError::StartElevatedCopy { core, .. } => {
            PublicInstallCoreBinaryKind::StartElevatedCopy { core: *core }
        }
        InstallCoreBinaryError::ElevatedCopyFailed {
            core, exit_code, ..
        } => PublicInstallCoreBinaryKind::ElevatedCopyFailed {
            core: *core,
            exit_code: *exit_code,
        },
        InstallCoreBinaryError::PathNotUtf8 { .. } => PublicInstallCoreBinaryKind::PathNotUtf8,
    }
}

fn project_check_unavailable(error: &RuntimeCheckUnavailable) -> PublicRuntimeCheckUnavailableKind {
    match error {
        RuntimeCheckUnavailable::NoEndpoint { .. } => PublicRuntimeCheckUnavailableKind::NoEndpoint,
        RuntimeCheckUnavailable::HostUnsupported { host, .. } => {
            PublicRuntimeCheckUnavailableKind::HostUnsupported { host: *host }
        }
        RuntimeCheckUnavailable::CandidateUnavailable { .. } => {
            PublicRuntimeCheckUnavailableKind::CandidateUnavailable
        }
        RuntimeCheckUnavailable::Backend { kind, .. } => {
            PublicRuntimeCheckUnavailableKind::Backend { core_kind: *kind }
        }
    }
}

fn project_runtime_error(error: &RuntimeError) -> PublicRuntimeError {
    match error {
        RuntimeError::ShuttingDown => PublicRuntimeError::ShuttingDown,
        RuntimeError::Isolated => PublicRuntimeError::Isolated,
        RuntimeError::OwnerUnresponsive { .. } => PublicRuntimeError::OwnerUnresponsive,
        RuntimeError::OwnerUnavailable => PublicRuntimeError::OwnerUnavailable,
        RuntimeError::SourceSettled => PublicRuntimeError::SourceSettled,
        RuntimeError::ApplyRuntime { failure } => PublicRuntimeError::ApplyRuntime {
            core_kind: failure.kind,
        },
        RuntimeError::StopCore { failure } => PublicRuntimeError::StopCore {
            core_kind: failure.kind,
        },
        RuntimeError::RecoverRuntime { failure } => PublicRuntimeError::RecoverRuntime {
            core_kind: failure.kind,
        },
        RuntimeError::RecoverServiceEndpoint { failure } => {
            PublicRuntimeError::RecoverServiceEndpoint {
                core_kind: failure.kind,
            }
        }
        RuntimeError::RefreshStatus { failure } => PublicRuntimeError::RefreshStatus {
            core_kind: failure.kind,
        },
        RuntimeError::InstallService { failure } => PublicRuntimeError::InstallService {
            core_kind: failure.kind,
        },
        RuntimeError::StartService { failure } => PublicRuntimeError::StartService {
            core_kind: failure.kind,
        },
        RuntimeError::StopService { failure } => PublicRuntimeError::StopService {
            core_kind: failure.kind,
        },
        RuntimeError::RestartService { failure } => PublicRuntimeError::RestartService {
            core_kind: failure.kind,
        },
        RuntimeError::UninstallService { failure } => PublicRuntimeError::UninstallService {
            core_kind: failure.kind,
        },
        RuntimeError::MoveHost { host, failure } => PublicRuntimeError::MoveHost {
            host: *host,
            core_kind: failure.kind,
        },
        RuntimeError::ServiceHostsCore => PublicRuntimeError::ServiceHostsCore,
        RuntimeError::CoreNotStarted { .. } => PublicRuntimeError::CoreNotStarted,
        RuntimeError::RecoveryUnresolved { .. } => PublicRuntimeError::RecoveryUnresolved,
        RuntimeError::BuildRuntime { source } => PublicRuntimeError::BuildRuntime {
            source: project_runtime_build_error(source),
        },
        RuntimeError::PublishRuntime { .. } => PublicRuntimeError::PublishRuntime,
        RuntimeError::ResolvePort { source } => PublicRuntimeError::ResolvePort {
            source: project_port_resolve_error(source),
        },
        RuntimeError::ResolveCoreBinary { source } => {
            let core = match source {
                nyanpasu_runtime_values::core_spec::CoreSpecError::FindCoreBinary {
                    core, ..
                } => *core,
            };
            PublicRuntimeError::ResolveCoreBinary { core }
        }
        RuntimeError::InstallCoreBinary { source } => PublicRuntimeError::InstallCoreBinary {
            source: project_install_core_binary_error(source),
        },
        RuntimeError::PrepareServiceInstallPrompt { .. } => {
            PublicRuntimeError::PrepareServiceInstallPrompt
        }
        RuntimeError::ReadCoreVersion { source } => {
            let core = match source {
                nyanpasu_contracts::core_version::CoreVersionError::RunCoreVersion {
                    core, ..
                }
                | nyanpasu_contracts::core_version::CoreVersionError::CoreVersionExit {
                    core,
                    ..
                }
                | nyanpasu_contracts::core_version::CoreVersionError::CoreVersionNotReported {
                    core,
                } => *core,
            };
            PublicRuntimeError::ReadCoreVersion { core }
        }
        RuntimeError::UnsettledBaseline { gap } => {
            PublicRuntimeError::UnsettledBaseline { gap: *gap }
        }
        RuntimeError::CoreRejectedConfig { core_kind, .. } => {
            PublicRuntimeError::CoreRejectedConfig {
                core_kind: *core_kind,
            }
        }
        RuntimeError::CheckUnavailable { reason } => PublicRuntimeError::CheckUnavailable {
            reason: project_check_unavailable(reason),
        },
        RuntimeError::CoreRolledBack { .. } => PublicRuntimeError::CoreRolledBack,
        RuntimeError::SubmissionUnobserved { failure } => {
            PublicRuntimeError::SubmissionUnobserved {
                core_kind: failure.kind,
            }
        }
        RuntimeError::HandoffOwnerMismatch { .. } => PublicRuntimeError::HandoffOwnerMismatch,
        RuntimeError::HandoffNotRestored { .. } => PublicRuntimeError::HandoffNotRestored,
        RuntimeError::RestoreFailed { .. } => PublicRuntimeError::RestoreFailed,
        RuntimeError::CommittedAfterRefusal { .. } => PublicRuntimeError::CommittedAfterRefusal,
        RuntimeError::NoRuntimeConfig => PublicRuntimeError::NoRuntimeConfig,
        RuntimeError::SerializeRuntimeConfig { .. } => PublicRuntimeError::SerializeRuntimeConfig,
        RuntimeError::ConvertRuntimeConfig { .. } => PublicRuntimeError::ConvertRuntimeConfig,
        RuntimeError::RuntimeSnapshotChanged => PublicRuntimeError::RuntimeSnapshotChanged,
        RuntimeError::RuntimeNodeNotFound { .. } => PublicRuntimeError::RuntimeNodeNotFound,
    }
}

fn project_hotkey_error(error: &HotkeyParseError) -> PublicHotkeyErrorKind {
    match error {
        HotkeyParseError::MalformedEntry { .. } => PublicHotkeyErrorKind::MalformedEntry,
        HotkeyParseError::UnknownFunction { .. } => PublicHotkeyErrorKind::UnknownFunction,
        HotkeyParseError::EmptyKeySegment { .. } => PublicHotkeyErrorKind::EmptyKeySegment,
        HotkeyParseError::UnsupportedAccelerator { .. } => {
            PublicHotkeyErrorKind::UnsupportedAccelerator
        }
        HotkeyParseError::MissingSuperKey { .. } => PublicHotkeyErrorKind::MissingSuperKey,
        HotkeyParseError::DuplicateAccelerator { .. } => {
            PublicHotkeyErrorKind::DuplicateAccelerator
        }
    }
}

fn project_config_error(error: &ConfigError) -> PublicConfigError {
    match error {
        ConfigError::LeaveNightlyChannel { .. } => PublicConfigError::LeaveNightlyChannel,
        ConfigError::InvalidUpdateSources { .. } => PublicConfigError::InvalidUpdateSources,
        ConfigError::InvalidCoreLogs { .. } => PublicConfigError::InvalidCoreLogs,
        ConfigError::InvalidPublicIpProbeSources { .. } => {
            PublicConfigError::InvalidPublicIpProbeSources
        }
        ConfigError::InvalidLatencyTimeout { .. } => PublicConfigError::InvalidLatencyTimeout,
        ConfigError::InvalidRemoteAccess { reason } => PublicConfigError::InvalidRemoteAccess {
            reason: match reason {
                RemoteAccessError::PortZero => PublicRemoteAccessErrorKind::PortZero,
                RemoteAccessError::UseDedicatedCommand => {
                    PublicRemoteAccessErrorKind::UseDedicatedCommand
                }
            },
        },
        ConfigError::ValidateHotkeyCapability => PublicConfigError::ValidateHotkeyCapability,
        ConfigError::ValidateHotkeys { source } => PublicConfigError::ValidateHotkeys {
            reason: project_hotkey_error(source),
        },
        ConfigError::WorkflowNotReady { .. } => PublicConfigError::WorkflowNotReady,
        ConfigError::ShuttingDown { .. } => PublicConfigError::ShuttingDown,
        ConfigError::OwnerStopped { .. } => PublicConfigError::OwnerStopped,
        ConfigError::SessionStateStopped => PublicConfigError::SessionStateStopped,
        ConfigError::InvalidTransparentProxy { source } => {
            PublicConfigError::InvalidTransparentProxy {
                reason: project_transparent_proxy_validation(source),
            }
        }
        ConfigError::TransparentProxyRequiresServiceMode => {
            PublicConfigError::TransparentProxyRequiresServiceMode
        }
        ConfigError::VersionConflict { .. } => PublicConfigError::VersionConflict,
        ConfigError::Commit { source, .. } => PublicConfigError::Commit {
            source: project_commit_aborted(source),
        },
        ConfigError::PersistSessionState { .. } => PublicConfigError::PersistSessionState,
    }
}

fn project_storage_error(error: &StorageOperationError) -> PublicStorageError {
    match error {
        StorageOperationError::OpenDatabase { .. } => PublicStorageError::OpenDatabase,
        StorageOperationError::InspectDatabase { .. } => PublicStorageError::InspectDatabase,
        StorageOperationError::PreserveDatabase { .. } => PublicStorageError::PreserveDatabase,
        StorageOperationError::BeginTransaction { .. } => PublicStorageError::BeginTransaction,
        StorageOperationError::OpenTable { .. } => PublicStorageError::OpenTable,
        StorageOperationError::ReadItem { .. } => PublicStorageError::ReadItem,
        StorageOperationError::WriteItem { .. } => PublicStorageError::WriteItem,
        StorageOperationError::RemoveItem { .. } => PublicStorageError::RemoveItem,
        StorageOperationError::ListItems { .. } => PublicStorageError::ListItems,
        StorageOperationError::CommitTransaction { .. } => PublicStorageError::CommitTransaction,
        StorageOperationError::DecodeValue { .. } => PublicStorageError::DecodeValue,
        StorageOperationError::EncodeValue { .. } => PublicStorageError::EncodeValue,
    }
}

fn project_system_dns_error(error: &SystemDnsError) -> PublicSystemDnsError {
    match error {
        SystemDnsError::AuthorizeDnsFlush { .. } => PublicSystemDnsError::AuthorizeDnsFlush,
        SystemDnsError::RunFlushCommand { .. } => PublicSystemDnsError::RunFlushCommand,
        SystemDnsError::FlushRejected { code, .. } => {
            PublicSystemDnsError::FlushRejected { exit_code: *code }
        }
        SystemDnsError::Unsupported => PublicSystemDnsError::Unsupported,
    }
}

fn project_os_proxy_error(error: &OsProxyError) -> PublicOsProxyError {
    match error {
        OsProxyError::ReadOsProxy { .. } => PublicOsProxyError::ReadOsProxy,
        OsProxyError::WriteOsProxy { enable, port, .. } => PublicOsProxyError::WriteOsProxy {
            enable: *enable,
            port: *port,
        },
    }
}

fn project_effects_error(error: &EffectsError) -> PublicEffectsError {
    match error {
        EffectsError::EffectsStopped => PublicEffectsError::EffectsStopped,
    }
}

fn project_http_server_error(error: &HttpServerControlError) -> PublicHttpServerError {
    match error {
        HttpServerControlError::ReadStatusUnsupported => {
            PublicHttpServerError::ReadStatusUnsupported
        }
        HttpServerControlError::ChangeUnsupported => PublicHttpServerError::ChangeUnsupported,
        HttpServerControlError::ReadStatus { .. } => PublicHttpServerError::ReadStatus,
    }
}

fn project_clash_read_error(error: &ClashReadError) -> PublicClashReadError {
    match error {
        ClashReadError::SettingsUnavailable => PublicClashReadError::SettingsUnavailable,
        ClashReadError::ProxiesUnavailable => PublicClashReadError::ProxiesUnavailable,
        ClashReadError::UnknownGroup => PublicClashReadError::UnknownGroup,
        ClashReadError::DelayTestFailed => PublicClashReadError::DelayTestFailed,
        ClashReadError::VersionUnavailable => PublicClashReadError::VersionUnavailable,
        ClashReadError::RulesUnavailable => PublicClashReadError::RulesUnavailable,
        ClashReadError::RuleProvidersUnavailable => PublicClashReadError::RuleProvidersUnavailable,
        ClashReadError::ProxyProvidersUnavailable => {
            PublicClashReadError::ProxyProvidersUnavailable
        }
        ClashReadError::StreamsUnavailable => PublicClashReadError::StreamsUnavailable,
    }
}

fn project_core_log_read_error(error: &CoreLogReadError) -> PublicCoreLogReadError {
    match error {
        CoreLogReadError::InvalidRequest => PublicCoreLogReadError::InvalidRequest,
        CoreLogReadError::CursorExpired => PublicCoreLogReadError::CursorExpired,
        CoreLogReadError::RecordGone => PublicCoreLogReadError::RecordGone,
        CoreLogReadError::TooLarge => PublicCoreLogReadError::TooLarge,
        CoreLogReadError::Unavailable => PublicCoreLogReadError::Unavailable,
    }
}

fn project_core_log_error(error: &CoreLogError) -> PublicCoreLogReadError {
    match error {
        CoreLogError::InvalidRequest => PublicCoreLogReadError::InvalidRequest,
        CoreLogError::CursorExpired => PublicCoreLogReadError::CursorExpired,
        CoreLogError::RecordGone => PublicCoreLogReadError::RecordGone,
        CoreLogError::TooLarge => PublicCoreLogReadError::TooLarge,
        CoreLogError::Unavailable(_) => PublicCoreLogReadError::Unavailable,
    }
}

fn project_profile_sync_read_error(error: &ProfileSyncReadError) -> PublicProfileSyncReadError {
    match error {
        ProfileSyncReadError::UnknownProfile => PublicProfileSyncReadError::UnknownProfile,
        ProfileSyncReadError::NotRemote => PublicProfileSyncReadError::NotRemote,
        ProfileSyncReadError::InvalidCursor => PublicProfileSyncReadError::InvalidCursor,
        ProfileSyncReadError::Unavailable => PublicProfileSyncReadError::Unavailable,
    }
}
