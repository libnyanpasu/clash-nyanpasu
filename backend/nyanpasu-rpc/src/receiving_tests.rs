use std::{io, path::PathBuf, sync::Arc};

use nyanpasu_config::{
    clash::config::clash_strategy::port::PickPortError,
    profile::{ExternalProfilePath, ProfileRevisionError, ProfileValidationError},
    runtime::{
        executor::{RuntimePipelineError, ScriptExecutionError, TransformFailure},
        snapshot::SnapshotBuildError,
    },
};
use nyanpasu_contracts::{
    core_install::InstallCoreBinaryError,
    core_version::CoreVersionError,
    elevation::ElevationConsentError,
    profiles::error::{ProfileFileError, SubscriptionFetchError},
    service::ServiceCommandError,
    system_dns::SystemDnsError,
    system_proxy::OsProxyError,
};
use nyanpasu_core::{
    client::debug_http::DebugHttpControlError, state::config_error::ConfigError,
    storage::StorageOperationError,
};
use nyanpasu_logging::LogError;
use nyanpasu_paths::InstallDirError;
use nyanpasu_profile_transform::RuntimeBuildError;
use nyanpasu_rpc_protocol::types as received;
use nyanpasu_runtime_values::{
    check::RuntimeCheckUnavailable,
    core_spec::CoreSpecError,
    effects::EffectsError,
    hotkey::{HotkeyAction, HotkeyParseError},
    mutation::{CommitAborted, NotReady, RuntimeAftermath},
    ports::PortResolveError,
    profiles::{ProfileContentError, ProfilesError},
    publish::PublishRuntimeError,
    runtime::{Degradation, DegradationReason, InterruptFailure, MutationOutcome},
    runtime_error::{CoreFailure, RestoreFailure, RuntimeError},
};
use nyanpasu_state_engine::error::UpsertError;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;

fn sample<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn receive<T: Serialize + DeserializeOwned>(value: impl Serialize) -> serde_json::Value {
    let bytes = serde_json::to_vec(&value).unwrap();
    let expected = serde_json::to_value(&value).unwrap();
    let decoded: T = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    expected
}

// One list constructs every variant and matches the application enum exhaustively.
// Adding a variant or field requires updating its fixture, including skipped sources.
macro_rules! cases {
    ($function:ident, $app:ident $(<$generic:ty>)?, $received:ty, $present:ident;
     $($variant:ident $({$($field:ident: $value:expr),* $(,)?})? $(($tuple_field:ident: $tuple_value:expr))?),* $(,)?) => {
        fn $function($present: bool) -> Vec<$app $(<$generic>)?> {
            let _ = $present;
            let values = vec![$($app::$variant $({$($field: $value),*})? $(($tuple_value))?),*];
            for value in &values {
                match value {
                    $($app::$variant $({$($field: _),*})? $(($tuple_field))? => {}),*
                }
                let payload = receive::<$received>(value);
                assert!(!payload.to_string().contains("operational source retained only on the host"));
            }
            values
        }
    };
}

fn io_source() -> io::Error {
    io::Error::other("operational source retained only on the host")
}
fn json_source() -> serde_json::Error {
    serde_json::from_str::<bool>("invalid").unwrap_err()
}
fn yaml_source() -> yaml_serde::Error {
    yaml_serde::from_str::<bool>("[").unwrap_err()
}

cases!(
    runtime_error_cases,
    RuntimeError,
    received::RuntimeError,
    present;
    ShuttingDown,
    Isolated,
    OwnerUnresponsive {
        operation_id: "field-5/路徑\\file.yaml".to_owned(),
    },
    OwnerUnavailable,
    SourceSettled,
    ApplyRuntime {
        failure: core_failure(present),
    },
    StopCore {
        failure: core_failure(present),
    },
    RecoverRuntime {
        failure: core_failure(present),
    },
    RecoverServiceEndpoint {
        failure: core_failure(present),
    },
    RefreshStatus {
        failure: core_failure(present),
    },
    InstallService {
        failure: core_failure(present),
    },
    StartService {
        failure: core_failure(present),
    },
    StopService {
        failure: core_failure(present),
    },
    RestartService {
        failure: core_failure(present),
    },
    UninstallService {
        failure: core_failure(present),
    },
    MoveHost {
        host: sample(json!(if present { "service" } else { "local" })),
        failure: core_failure(present),
    },
    ServiceHostsCore,
    CoreNotStarted {
        reason: "field-6/路徑\\file.yaml".to_owned(),
        retryable: present,
    },
    RecoveryUnresolved {
        reason: "field-7/路徑\\file.yaml".to_owned(),
    },
    BuildRuntime {
        source: runtime_build_error_cases(present).remove(0),
    },
    PublishRuntime {
        source: publish_runtime_error_cases(present).remove(0),
    },
    ResolvePort {
        source: port_resolve_error_cases(present).remove(0),
    },
    ResolveCoreBinary {
        source: core_spec_error_cases(present).remove(0),
    },
    InstallCoreBinary {
        source: install_core_binary_error_cases(present).remove(0),
    },
    PrepareServiceInstallPrompt {
        source: service_command_error_cases(present).remove(0),
    },
    ReadCoreVersion {
        source: core_version_error_cases(present).remove(0),
    },
    UnsettledBaseline {
        gap: sample(json!("no_restorable_baseline")),
    },
    CoreRejectedConfig {
        core_kind: if present {
            Some(sample(json!("backend_unavailable")))
        } else {
            None
        },
        message: "field-8/路徑\\file.yaml".to_owned(),
    },
    CheckUnavailable {
        reason: sample(json!({"cause": "no_endpoint", "reason": "offline"})),
    },
    CoreRolledBack {
        reason: if present {
            Some("field-9/路徑\\file.yaml".to_owned())
        } else {
            None
        },
    },
    SubmissionUnobserved {
        failure: core_failure(present),
    },
    HandoffOwnerMismatch {
        expected: sample(json!(if present { "service" } else { "local" })),
    },
    HandoffNotRestored {
        failure: restore_failure_cases(present).remove(0),
    },
    RestoreFailed {
        failure: restore_failure_cases(present).remove(0),
    },
    CommittedAfterRefusal {
        operation_id: "field-10/路徑\\file.yaml".to_owned(),
    },
    NoRuntimeConfig,
    SerializeRuntimeConfig {
        source: yaml_source(),
    },
    ConvertRuntimeConfig {
        source: json_source(),
    },
    RuntimeSnapshotChanged,
    RuntimeNodeNotFound { node_id: 7 },
);

cases!(
    restore_failure_cases,
    RestoreFailure,
    RestoreFailure,
    present;
    MoveHostBack {
        host: sample(json!(if present { "service" } else { "local" })),
        failure: core_failure(present),
    },
    ReadStatus {
        failure: core_failure(present),
    },
    Unobserved {
        failure: if present {
            Some(core_failure(present))
        } else {
            None
        },
    },
    Unverified {
        failure: if present {
            Some(core_failure(present))
        } else {
            None
        },
    },
    NotRecorded,
);

cases!(
    profile_content_error_cases,
    ProfileContentError,
    received::ProfileContentError,
    present;
    NotYamlMapping {
        source: yaml_source(),
    },
    ReserializeYaml {
        source: yaml_source(),
    },
    MissingProxies,
    EmptyScript,
);

cases!(
    profiles_error_cases,
    ProfilesError,
    received::ProfilesError,
    present;
    ProfileNotFound {
        uid: sample(json!("profile-1")),
    },
    ProfileHasNoFile {
        uid: sample(json!("profile-1")),
    },
    NotARemoteProfile {
        uid: sample(json!("profile-1")),
    },
    ProfileFileNotWritable {
        uid: sample(json!("profile-1")),
    },
    ProfileInUse {
        uid: sample(json!("profile-1")),
        referrers: if present {
            vec![sample(json!("profile-1"))]
        } else {
            vec![]
        },
        current: present,
        global_transforms: present,
    },
    ProfileIdCollision {
        uid: sample(json!("profile-1")),
    },
    ValidationFailed {
        errors: validation_errors(present),
    },
    ReorderListSizeMismatch {
        expected: 2,
        got: 3,
    },
    ReorderListDuplicate {
        uid: sample(json!("profile-1")),
    },
    RevisionOverflow {
        source: ProfileRevisionError::Overflow,
    },
    InvalidSubscriptionUrl {
        url: "field-11/路徑\\file.yaml".to_owned(),
        source: url::ParseError::RelativeUrlWithoutBase,
    },
    RemoteProfileNeedsImport,
    RefreshInProgress {
        uid: sample(json!("profile-1")),
    },
    FetchSubscription {
        url: "https://example.test/release".parse().unwrap(),
        source: subscription_fetch_error_cases(present).remove(0),
    },
    ProfileContentRejected {
        source: profile_content_error_cases(present).remove(0),
    },
    ProfileDeletedDuringRefresh {
        uid: sample(json!("profile-1")),
    },
    ProfileChangedDuringRefresh {
        uid: sample(json!("profile-1")),
    },
    FingerprintDefinition {
        uid: sample(json!("profile-1")),
        source: yaml_source(),
    },
    ReadProfileFile {
        uid: sample(json!("profile-1")),
        source: profile_file_error_cases(present).remove(0),
    },
    ProfileFileNotYaml {
        uid: sample(json!("profile-1")),
        source: profile_content_error_cases(present).remove(0),
    },
    ProfileFileMissing {
        uid: sample(json!("profile-1")),
        path: PathBuf::from("field-12/路徑\\file.yaml".to_owned()).into(),
    },
    ReadExternalProfile {
        target: sample(json!("/profiles/source.yaml")),
        source: profile_file_error_cases(present).remove(0),
    },
    Materialization {
        operation: sample(json!("prepare_file_first")),
        source: profile_file_error_cases(present).remove(0),
    },
    VersionConflict {
        expected: 4,
        actual: 5,
        cleanup_failures: vec![ProfilesError::ShuttingDown],
    },
    Commit {
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
        source: commit_aborted_cases(present).remove(0),
        cleanup_failures: vec![ProfilesError::ShuttingDown],
    },
    WorkflowNotReady { source: NotReady },
    ProfilesActorStopped,
    ProfilesReplyDropped,
    BlockingTaskCancelled {
        source: cancelled_task(),
    },
    ShuttingDown,
);

cases!(
    mutation_outcome_cases,
    MutationOutcome<serde_json::Value>,
    received::MutationOutcome<serde_json::Value>,
    present;
    Committed {
        value: json!({"nested": [null, true, 42]}),
        commits: if present {
            vec![sample(
                json!({"operation_id": "op-1", "domain": "profiles", "source_version": 3, "runtime": "applied"}),
            )]
        } else {
            vec![]
        },
        notifications_pending: present,
    },
    CommittedDegraded {
        value: json!({"nested": [null, true, 42]}),
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
);

cases!(
    degradation_reason_cases,
    DegradationReason,
    received::DegradationReason,
    present;
    RuntimeRecoveryRequired {
        operation_id: if present {
            Some("field-14/路徑\\file.yaml".to_owned())
        } else {
            None
        },
        cause: if present {
            Some(Arc::new(runtime_error_cases(present).remove(0)))
        } else {
            None
        },
    },
    RuntimeDeferred {
        cause: Arc::new(runtime_error_cases(present).remove(0)),
    },
    RuntimeProductPublishFailed {
        cause: Arc::new(runtime_error_cases(present).remove(0)),
    },
    ServiceStopFailed {
        cause: Arc::new(runtime_error_cases(present).remove(0)),
    },
    ModeInterruptionFailed {
        cause: sample(json!("timeout")),
    },
    ProfileInterruptionFailed {
        cause: sample(json!("timeout")),
    },
    ProxyInterruptionFailed {
        cause: sample(json!("timeout")),
    },
    ProxyCacheRefreshFailed,
    JournalInvalid,
    JobsJournalUnavailable,
    MaterializationDeferred,
    CleanupDeferred,
    ProfileAutoActivationFailed {
        profile: sample(json!("profile-1")),
        cause: Arc::new(profiles_error_cases(present).remove(0)),
    },
);

cases!(
    commit_aborted_cases,
    CommitAborted,
    received::CommitAborted,
    present;
    WriteConfig {
        runtime: runtime_aftermath_cases(present).remove(0),
        source: Box::new(io_source()),
    },
    RecoverAfterWriteFailure {
        runtime: runtime_aftermath_cases(present).remove(0),
        source: Box::new(io_source()),
    },
    RuntimeRefused {
        errors: vec![Arc::new(runtime_error_cases(present).remove(0))],
        runtime: runtime_aftermath_cases(present).remove(0),
        source: Box::new(io_source()),
    },
    RuntimeFailed {
        errors: vec![Arc::new(runtime_error_cases(present).remove(0))],
        runtime: runtime_aftermath_cases(present).remove(0),
        source: Box::new(io_source()),
    },
    ValidateState {
        source: Box::new(io_source()),
    },
);

cases!(
    runtime_aftermath_cases,
    RuntimeAftermath,
    received::RuntimeAftermath,
    present;
    Untouched,
    RolledBack,
    RollbackFailed {
        detail: Arc::new(runtime_error_cases(present).remove(0)),
    },
    Unknown {
        detail: Arc::new(runtime_error_cases(present).remove(0)),
    },
);

cases!(
    port_resolve_error_cases,
    PortResolveError,
    PortResolveError,
    present;
    ResolvePort {
            field: sample(json!("mixed")),
            source: pick_port_error_cases(present).remove(0),
        },
    PortConflict {
            first: sample(json!("mixed")),
            second: sample(json!("tproxy")),
            port: 7890,
        },
);

cases!(
    core_spec_error_cases,
    CoreSpecError,
    received::CoreSpecError,
    present;
    FindCoreBinary {
            core: sample(json!("mihomo")),
            source: io_source(),
        }
);

cases!(
    publish_runtime_error_cases,
    PublishRuntimeError,
    received::PublishRuntimeError,
    present;
    CreateRuntimeDirectory {
        path: PathBuf::from("field-15/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    WriteRuntimeConfig {
        path: PathBuf::from("field-16/路徑\\file.yaml".to_owned()).into(),
        source: atomicwrites::Error::Internal(io_source()),
    },
);

cases!(
    runtime_build_error_cases,
    RuntimeBuildError,
    received::RuntimeBuildError,
    present;
    StartScriptRunner {
        source: Box::new(io_source()),
    },
    ValidateProfiles {
        errors: validation_errors(present),
    },
    ValidateTransparentProxy {
        source: sample(json!({"kind": "invalid_interface", "name": "eth0/路徑"})),
    },
    TransparentProxyRequiresServiceMode,
    RunPipeline {
        source: runtime_pipeline_error_cases(present).remove(0),
    },
    TransformsFailed {
        failures: transform_failure_cases(present),
        logs: vec![sample(
            json!({"tag":{"kind":"bare_root"}, "entries":[{"level":"error", "message":"failed"}]}),
        )],
    },
    SerializeFinalConfig {
        source: yaml_source(),
    },
    ConfigNotMapping,
    UnsupportedConfigValues {
        values: if present {
            vec![sample(
                json!({"path": "tun.stack", "value": "bad", "supported": ["system"]}),
            )]
        } else {
            vec![]
        },
    },
);

cases!(
    runtime_pipeline_error_cases,
    RuntimePipelineError,
    received::RuntimePipelineError,
    present;
    ExecuteScript {
        source: script_execution_error_cases(present).remove(0),
    },
    SelectedProfileNotFound {
        profile: sample(json!("profile-1")),
    },
    SelectedProfileNotConfig {
        profile: sample(json!("profile-1")),
    },
    CompositionMemberInvalid {
        composition: sample(json!("profile-1")),
        member: sample(json!("profile-1")),
        reason: "field-17/路徑\\file.yaml".to_owned(),
    },
    ContentSource {
        profile: sample(json!("profile-1")),
        path: sample(json!("profile.yaml")),
        source: Box::new(io_source()),
    },
    ParseProfile {
        profile: sample(json!("profile-1")),
        message: "field-18/路徑\\file.yaml".to_owned(),
    },
    Snapshot {
        source: SnapshotBuildError::MissingRoot {
            root_id: 17,
        },
    },
    Internal {
        message: "field-19/路徑\\file.yaml".to_owned(),
    },
);

cases!(
    script_execution_error_cases,
    ScriptExecutionError,
    ScriptExecutionError,
    present;
    StartEngine {
            runtime: sample(json!("javascript")),
            operation: sample(json!("transform")),
        }
);

cases!(
    pick_port_error_cases,
    PickPortError,
    PickPortError,
    present;
    PortNotAvailable { port: 7 }, NoOpenPort
);

cases!(
    profile_file_error_cases,
    ProfileFileError,
    received::ProfileFileError,
    present;
    PreserveDerivedDirectory {
        source: Box::new(io_source()),
    },
    InspectPath {
        path: PathBuf::from("field-20/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    ReadFile {
        path: PathBuf::from("field-21/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    ReadLink {
        path: PathBuf::from("field-22/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    WriteFile {
        path: PathBuf::from("field-23/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    AtomicWrite {
        path: PathBuf::from("field-24/路徑\\file.yaml".to_owned()).into(),
        source: Box::new(io_source()),
    },
    CreateDirectory {
        path: PathBuf::from("field-25/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    RemoveFile {
        path: PathBuf::from("field-26/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    ReplaceFile {
        from: PathBuf::from("field-27/路徑\\file.yaml".to_owned()).into(),
        to: PathBuf::from("field-28/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    CreateSymlink {
        link: PathBuf::from("field-29/路徑\\file.yaml".to_owned()).into(),
        target: PathBuf::from("field-30/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    ListDirectory {
        path: PathBuf::from("field-31/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    SyncDirectory {
        path: PathBuf::from("field-32/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    SetPermissions {
        path: PathBuf::from("field-33/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    CanonicalizePath {
        path: PathBuf::from("field-34/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    ReadExternalTarget {
        target: sample(json!("/profiles/source.yaml")),
        source: io_source(),
    },
    ParseJournal {
        path: PathBuf::from("field-35/路徑\\file.yaml".to_owned()).into(),
        source: Box::new(io_source()),
    },
    SerializeJournal {
        source: Box::new(io_source()),
    },
    ReservedPath {
        path: PathBuf::from("field-36/路徑\\file.yaml".to_owned()).into(),
    },
    PathEscapesProfilesDir {
        path: PathBuf::from("field-37/路徑\\file.yaml".to_owned()).into(),
        root: PathBuf::from("field-38/路徑\\file.yaml".to_owned()).into(),
    },
    NoParentDirectory {
        path: PathBuf::from("field-39/路徑\\file.yaml".to_owned()).into(),
    },
    UnexpectedNode {
        path: PathBuf::from("field-40/路徑\\file.yaml".to_owned()).into(),
        expected: sample(json!("regular_file")),
    },
    UnexpectedSymlink {
        path: PathBuf::from("field-41/路徑\\file.yaml".to_owned()).into(),
    },
    ExistingFileBlocksSymlink {
        path: PathBuf::from("field-42/路徑\\file.yaml".to_owned()).into(),
    },
    SymlinkTargetNotUtf8 {
        path: PathBuf::from("field-43/路徑\\file.yaml".to_owned()).into(),
    },
    InvalidExternalPath {
        source: ExternalProfilePath::new("relative").unwrap_err(),
    },
    CleanupTombstoneExists {
        path: PathBuf::from("field-44/路徑\\file.yaml".to_owned()).into(),
    },
    JournalNotFound {
        operation_id: "field-45/路徑\\file.yaml".to_owned(),
    },
    CompensationFenced {
        path: PathBuf::from("field-46/路徑\\file.yaml".to_owned()).into(),
    },
    OperationIdInvalid,
    OperationIdExhausted,
    JournalOperationIdMismatch,
    JournalHashInvalid,
    MixedTransactionFamilies,
    ConflictingJournalPayloads,
    JournalDestinationDiffers,
    ConflictingCleanupPayloads,
    MultipleStagedResources,
    MultipleBackups,
    StagedHashMismatch,
    ReadySymlinkMismatch,
    StagedResourceMissing,
    PromotedHashMismatch,
    CompensatingCannotPromote,
    TargetHashMismatch,
    NotCompletable,
    CleanupAlreadyActivated,
    DivergedBeforeRecovery,
);

cases!(
    subscription_fetch_error_cases,
    SubscriptionFetchError,
    received::SubscriptionFetchError,
    present;
    BuildHttpClient {
        source: Box::new(io_source()),
    },
    RequestSubscription {
        source: Box::new(io_source()),
    },
    SubscriptionHttpStatus { status: 7 },
    ReadSubscriptionBody {
        source: Box::new(io_source()),
    },
);

cases!(
    install_core_binary_error_cases,
    InstallCoreBinaryError,
    received::InstallCoreBinaryError,
    present;
    AuthorizeCoreCopy {
        core: sample(json!("mihomo")),
        destination: PathBuf::from("field-47/路徑\\file.yaml".to_owned()).into(),
        source: elevation_consent_error_cases(present).remove(0),
    },
    StartElevatedCopy {
        core: sample(json!("mihomo")),
        destination: PathBuf::from("field-48/路徑\\file.yaml".to_owned()).into(),
        source: io_source(),
    },
    ElevatedCopyFailed {
        core: sample(json!("mihomo")),
        destination: PathBuf::from("field-49/路徑\\file.yaml".to_owned()).into(),
        exit_code: if present { Some(7) } else { None },
    },
    PathNotUtf8 {
        path: PathBuf::from("field-50/路徑\\file.yaml".to_owned()).into(),
    },
);

cases!(
    core_version_error_cases,
    CoreVersionError,
    received::CoreVersionError,
    present;
    RunCoreVersion {
        core: sample(json!("mihomo")),
        source: anyhow::Error::new(io_source()),
    },
    CoreVersionExit {
        core: sample(json!("mihomo")),
    },
    CoreVersionNotReported {
        core: sample(json!("mihomo")),
    },
);

cases!(
    service_command_error_cases,
    ServiceCommandError,
    received::ServiceCommandError,
    present;
    AuthorizeServiceCommand {
        command: sample(json!("start")),
        source: elevation_consent_error_cases(present).remove(0),
    },
    SessionEnding,
    ResolveServiceUser {
        source: service_user_source(),
    },
    ResolveServiceDirs {
        source: Box::new(InstallDirError::Unknown),
    },
    RunElevated {
        command: sample(json!("start")),
        source: io_source(),
    },
    ServiceCommandExit {
        command: sample(json!("start")),
        exit_code: if present { Some(7) } else { None },
        signal: if present { Some(7) } else { None },
    },
    RunServiceStatus {
        source: io_source(),
    },
    ServiceStatusExit {
        exit_code: if present { Some(7) } else { None },
        signal: if present { Some(7) } else { None },
    },
    ServiceStatusNotUtf8 {
        source: String::from_utf8(vec![0xff]).unwrap_err(),
    },
    ParseServiceStatus {
        source: json_source(),
    },
    TimedOut { limit_ms: 7 },
    StillRunning { limit_ms: 7 },
    TaskCancelled,
);

cases!(
    elevation_consent_error_cases,
    ElevationConsentError,
    ElevationConsentError,
    present;
    AuthorizeElevation {
            operation: sample(json!({"kind": "dns_flush"})),
        }
);

cases!(
    system_dns_error_cases,
    SystemDnsError,
    received::SystemDnsError,
    present;
    AuthorizeDnsFlush {
        source: elevation_consent_error_cases(present).remove(0),
    },
    RunFlushCommand {
        command: "field-51/路徑\\file.yaml",
        source: io_source(),
    },
    FlushRejected {
        command: "field-52/路徑\\file.yaml",
        code: if present { Some(7) } else { None },
    },
    Unsupported,
);

cases!(
    os_proxy_error_cases,
    OsProxyError,
    received::OsProxyError,
    present;
    ReadOsProxy {
        source: Box::new(io_source()),
    },
    WriteOsProxy {
        enable: present,
        host: "field-53/路徑\\file.yaml".to_owned(),
        port: 6,
        source: Box::new(io_source()),
    },
);

cases!(
    config_error_cases,
    ConfigError,
    received::ConfigError,
    present;
    LeaveNightlyChannel {
        to: sample(json!("stable")),
    },
    InvalidUpdateSources {
        reason: "field-54/路徑\\file.yaml".to_owned(),
    },
    InvalidCoreLogs {
        reason: "field-55/路徑\\file.yaml".to_owned(),
    },
    InvalidPublicIpProbeSources {
        reason: "field-56/路徑\\file.yaml".to_owned(),
    },
    InvalidTransparentProxy {
        source: sample(json!({"kind": "missing_listener", "mode": "tproxy"})),
    },
    TransparentProxyRequiresServiceMode,
    InvalidLatencyTimeout {
        reason: "field-57/路徑\\file.yaml".to_owned(),
    },
    ValidateHotkeys {
        source: hotkey_parse_error_cases(present).remove(0),
    },
    ValidateHotkeyCapability,
    WorkflowNotReady { source: NotReady },
    ShuttingDown {
        domain: sample(json!("profiles")),
    },
    OwnerStopped {
        domain: sample(json!("profiles")),
    },
    VersionConflict {
        domain: sample(json!("profiles")),
        expected: 7,
        actual: 8,
    },
    Commit {
        domain: sample(json!("profiles")),
        source: commit_aborted_cases(present).remove(0),
    },
    PersistSessionState {
        source: UpsertError::WriteConfig(anyhow::Error::new(
            io_source(),
        )),
    },
    SessionStateStopped,
);

cases!(
    storage_operation_error_cases,
    StorageOperationError,
    received::StorageOperationError,
    present;
    OpenDatabase {
        path: "field-58/路徑\\file.yaml".to_owned(),
        source: redb::DatabaseError::Storage(redb::StorageError::Io(io_source())),
    },
    InspectDatabase {
        path: "field-59/路徑\\file.yaml".to_owned(),
        source: io_source(),
    },
    PreserveDatabase {
        source: nyanpasu_persistence::Error::PreserveEvidence {
            path: PathBuf::from("broken"),
            evidence: PathBuf::from("evidence"),
            source: io_source(),
        },
    },
    BeginTransaction {
        source: redb::TransactionError::Storage(redb::StorageError::Io(io_source())),
    },
    OpenTable {
        source: redb::TableError::Storage(redb::StorageError::Io(io_source())),
    },
    ReadItem {
        key: "field-60/路徑\\file.yaml".to_owned(),
        source: redb::StorageError::Io(io_source()),
    },
    WriteItem {
        key: "field-61/路徑\\file.yaml".to_owned(),
        source: redb::StorageError::Io(io_source()),
    },
    RemoveItem {
        key: "field-62/路徑\\file.yaml".to_owned(),
        source: redb::StorageError::Io(io_source()),
    },
    ListItems {
        source: redb::StorageError::Io(io_source()),
    },
    CommitTransaction {
        source: redb::CommitError::Storage(redb::StorageError::Io(io_source())),
    },
    DecodeValue {
        key: "field-63/路徑\\file.yaml".to_owned(),
        source: json_source(),
    },
    EncodeValue {
        key: "field-64/路徑\\file.yaml".to_owned(),
        source: json_source(),
    },
);

cases!(
    hotkey_action_cases,
    HotkeyAction,
    HotkeyAction,
    present;
    OpenOrCloseDashboard,
    ClashModeRule,
    ClashModeGlobal,
    ClashModeDirect,
    ToggleSystemProxy,
    EnableSystemProxy,
    DisableSystemProxy,
    ToggleTunMode,
    EnableTunMode,
    DisableTunMode,
    ToggleTransparentProxy,
    EnableTransparentProxy,
    DisableTransparentProxy,
);

cases!(
    hotkey_parse_error_cases,
    HotkeyParseError,
    received::HotkeyParseError,
    present;
    MalformedEntry {
        entry: "field-65/路徑\\file.yaml".to_owned(),
    },
    UnknownFunction {
        function: "field-66/路徑\\file.yaml".to_owned(),
    },
    EmptyKeySegment {
        accelerator: "field-67/路徑\\file.yaml".to_owned(),
    },
    UnsupportedAccelerator {
        accelerator: "field-68/路徑\\file.yaml".to_owned(),
        source: Box::new(io_source()),
    },
    MissingSuperKey {
        accelerator: "field-69/路徑\\file.yaml".to_owned(),
    },
    DuplicateAccelerator {
        accelerator: "field-70/路徑\\file.yaml".to_owned(),
        first: hotkey_action_cases(present).remove(0),
        second: hotkey_action_cases(present).remove(0),
    },
);

cases!(
    effects_error_cases,
    EffectsError,
    EffectsError,
    present;
    EffectsStopped
);

cases!(
    debug_http_control_error_cases,
    DebugHttpControlError,
    received::DebugHttpControlError,
    present;
    ReadStatusUnsupported,
    ChangeEnabledUnsupported,
    ReadStatus {
        source: anyhow::Error::new(io_source()),
    },
    ChangeEnabled {
        source: anyhow::Error::new(io_source()),
    },
);

fn degradation(present: bool) -> Degradation {
    let _ = present;
    Degradation {
        phase: sample(json!("runtime_apply")),
        reason: degradation_reason_cases(present).remove(0),
        message: "field-73/路徑\\file.yaml".to_owned(),
        retryable: present,
    }
}

// The service source is io::Error on Windows and whoami::Error elsewhere.
fn service_user_source<T: From<whoami::Error>>() -> T {
    whoami::CpuArchitecture::Unknown("oracle".into())
        .width()
        .unwrap_err()
        .into()
}

fn cancelled_task() -> tokio::task::JoinError {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let task = tokio::spawn(std::future::pending::<()>());
            task.abort();
            task.await.unwrap_err()
        })
}

cases!(
    log_error_cases,
    LogError,
    LogError,
    present;
    Unavailable,
    Unsupported,
    SessionExpired,
    CursorReset,
    FileGone,
    Limit,
    InvalidRequest,
);

cases!(
    check_unavailable_cases, RuntimeCheckUnavailable, RuntimeCheckUnavailable, present;
    NoEndpoint { reason: "offline".into() },
    HostUnsupported { host: sample(json!("local")), reason: "not supported".into() },
    CandidateUnavailable { reason: "missing candidate".into() },
    Backend { kind: present.then(|| sample(json!("backend_unavailable"))), message: "unavailable".into(), retryable: present },
);

cases!(interrupt_failure_cases, InterruptFailure, InterruptFailure, present;
    Stale, Unavailable, Timeout, Protocol,
);

cases!(transform_failure_cases, TransformFailure, TransformFailure, present;
    Profile { id: "profile-1".into() },
    Builtin { name: "builtin".into() },
);

cases!(profile_validation_error_cases, ProfileValidationError, ProfileValidationError, present;
    ItemKeyMismatch { key: sample(json!("key")), item_uid: sample(json!("item")) },
    CurrentNotFound(_profile: sample(json!("profile"))),
    CurrentNotConfig(_profile: sample(json!("profile"))),
    TransformTargetNotFound { owner: sample(json!({"type":"global"})), target: sample(json!("profile")) },
    TransformTargetNotTransform { owner: sample(json!({"type":"global"})), target: sample(json!("profile")) },
    EmptyCompositionConfig { composition: sample(json!("profile")) },
    CompositionSelfReference { composition: sample(json!("profile")), role: sample(json!("base")) },
    CompositionMemberNotFound { composition: sample(json!("profile")), role: sample(json!("base")), profile: sample(json!("member")) },
    CompositionMemberNotDirectFileConfig { composition: sample(json!("profile")), role: sample(json!("base")), profile: sample(json!("member")) },
    CompositionBaseAlsoContributor { composition: sample(json!("profile")), profile: sample(json!("member")) },
    CompositionDuplicateContributor { composition: sample(json!("profile")), profile: sample(json!("member")) },
    DuplicateMaterializedFile { file: sample(json!("profile.yaml")), first: sample(json!("first")), second: sample(json!("second")) },
    UnsupportedRemoteUrlScheme { profile: sample(json!("profile")), scheme: "invalid".into() },
    FolderDuplicateId { id: sample(json!("folder")) },
    FolderNameEmpty { id: sample(json!("folder")) },
    FolderMemberNotFound { folder: sample(json!("folder")), profile: sample(json!("profile")) },
    FolderMemberNotConfig { folder: sample(json!("folder")), profile: sample(json!("profile")) },
    FolderMemberDuplicate { profile: sample(json!("profile")) },
    FolderNotFound { id: sample(json!("folder")) },
    LayoutMissingProfile { profile: sample(json!("profile")) },
    LayoutUnknownProfile { profile: sample(json!("profile")) },
    ProxyGroupLayoutProfileNotConfig { profile: sample(json!("profile")) },
    ProxyGroupFolderDuplicateId { profile: sample(json!("profile")), id: sample(json!("folder")) },
    ProxyGroupFolderNameEmpty { profile: sample(json!("profile")), id: sample(json!("folder")) },
    ProxyGroupNameEmpty { profile: sample(json!("profile")) },
    ProxyGroupDuplicate { profile: sample(json!("profile")), name: "group".into() },
);

#[test]
fn receiving_types_preserve_all_error_and_mutation_variants() {
    for present in [false, true] {
        check_unavailable_cases(present);
        interrupt_failure_cases(present);
        transform_failure_cases(present);
        profile_validation_error_cases(present);
        runtime_error_cases(present);
        restore_failure_cases(present);
        profile_content_error_cases(present);
        profiles_error_cases(present);
        mutation_outcome_cases(present);
        degradation_reason_cases(present);
        commit_aborted_cases(present);
        runtime_aftermath_cases(present);
        port_resolve_error_cases(present);
        core_spec_error_cases(present);
        publish_runtime_error_cases(present);
        runtime_build_error_cases(present);
        runtime_pipeline_error_cases(present);
        script_execution_error_cases(present);
        pick_port_error_cases(present);
        profile_file_error_cases(present);
        subscription_fetch_error_cases(present);
        install_core_binary_error_cases(present);
        core_version_error_cases(present);
        service_command_error_cases(present);
        elevation_consent_error_cases(present);
        system_dns_error_cases(present);
        os_proxy_error_cases(present);
        config_error_cases(present);
        storage_operation_error_cases(present);
        hotkey_action_cases(present);
        hotkey_parse_error_cases(present);
        effects_error_cases(present);
        debug_http_control_error_cases(present);
        ipc_domains_and_envelope(present);
        for error in log_error_cases(present) {
            let expected = serde_json::to_value(&error).unwrap();
            let envelope = receive::<received::RpcError>(crate::error::application_error(error));
            let envelope: received::RpcError = sample(envelope);
            let decoded = envelope.decode_domain_error::<LogError>().unwrap().unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
        }
    }
}

fn core_failure(present: bool) -> CoreFailure {
    CoreFailure {
        kind: present.then(|| sample(json!("backend_unavailable"))),
        message: "core failed".into(),
        retryable: present,
        operation_id: present.then(|| "core-operation".into()),
    }
}

fn validation_errors(present: bool) -> Vec<ProfileValidationError> {
    if present {
        profile_validation_error_cases(present)
    } else {
        vec![]
    }
}

fn ipc_domains_and_envelope(present: bool) {
    use crate::error::{IpcError, IpcErrorKind, application_error};
    macro_rules! domains {
        ($($variant:ident => $fixtures:ident),* $(,)?) => {
            let mut kinds = vec![IpcErrorKind::Unknown];
            $(
                for error in $fixtures(present) {
                    kinds.push(IpcErrorKind::$variant(Box::new(error)));
                }
                for error in $fixtures(present) {
                    let domain = serde_json::to_value(&error).unwrap();
                    let ipc: IpcError = error.into();
                    let payload = receive::<received::IpcError>(&ipc);
                    assert_eq!(payload["kind"]["error"], domain);
                    assert_eq!(payload["message"], ipc.to_string());
                    let envelope = receive::<received::RpcError>(application_error(ipc));
                    assert_eq!(envelope["domain_error"], payload);
                    let envelope: received::RpcError = sample(envelope);
                    let decoded = envelope.decode_domain_error::<received::IpcError>().unwrap().unwrap();
                    assert_eq!(serde_json::to_value(decoded).unwrap(), payload);
                }
            )*
            for kind in kinds {
                // The same list builds fixtures and covers the application's domain enum.
                match &kind {
                    IpcErrorKind::Unknown $(| IpcErrorKind::$variant(_))* => {}
                }
                receive::<received::IpcErrorKind>(kind);
            }
        };
    }
    domains!(
        Profiles => profiles_error_cases,
        Runtime => runtime_error_cases,
        Config => config_error_cases,
        Storage => storage_operation_error_cases,
        SystemDns => system_dns_error_cases,
        SystemProxy => os_proxy_error_cases,
        Effects => effects_error_cases,
        DebugHttp => debug_http_control_error_cases,
    );
    receive::<received::Degradation>(degradation(present));
    receive::<received::RpcError>(nyanpasu_rpc_protocol::RpcError {
        kind: "application_error",
        message: "envelope".into(),
        code: present.then(|| "code".into()),
        retryable: present.then_some(false),
        operation_id: present.then(|| "operation".into()),
        domain_error: present.then(|| {
            Box::new(nyanpasu_rpc_protocol::RpcValue(
                json!({"nested":[null, true, 42]}),
            ))
        }),
    });
}
