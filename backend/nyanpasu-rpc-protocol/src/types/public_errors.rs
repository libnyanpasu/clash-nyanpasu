//! Explicit, bounded public failure wire types for callers lacking `SensitiveRead`.
//!
//! These types preserve stable domain, kind, HTTP status, commit aftermath, and
//! runtime failure discriminants needed by non-privileged RPC callers and UI
//! localization without exposing local filesystem paths, URLs, credentials,
//! host commands, scripts, logs, or raw underlying error strings.

use nyanpasu_config::application::ClashCore;
use nyanpasu_core_metadata::CoreErrorKind;
use nyanpasu_runtime_values::{
    check::EvidenceGap, execution_host::ExecutionHost, ports::PortField,
};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Exhaustive public classification for a failed command when the caller lacks
/// `Scope::SensitiveRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "domain", content = "error", rename_all = "snake_case")]
pub enum PublicFailure {
    Profiles(PublicProfilesError),
    Runtime(PublicRuntimeError),
    Config(PublicConfigError),
    Storage(PublicStorageError),
    SystemDns(PublicSystemDnsError),
    SystemProxy(PublicOsProxyError),
    Effects(PublicEffectsError),
    HttpServer(PublicHttpServerError),
    ClashRead(PublicClashReadError),
    CoreLogRead(PublicCoreLogReadError),
    ProfileSyncRead(PublicProfileSyncReadError),
    Core { kind: CoreErrorKind },
}

/// Safe public profile content validation failure reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublicProfileContentKind {
    NotYamlMapping,
    ReserializeYaml,
    MissingProxies,
    EmptyScript,
}

/// Safe public subscription fetch failure classification without request URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicSubscriptionFetchKind {
    BuildHttpClient,
    RequestSubscription,
    SubscriptionHttpStatus { status: u16 },
    ReadSubscriptionBody,
}

/// Safe public profile error for callers without `SensitiveRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicProfilesError {
    ProfileNotFound,
    ProfileHasNoFile,
    NotARemoteProfile,
    ProfileFileNotWritable,
    ProfileInUse { referrer_count: usize },
    ProfileIdCollision,
    ValidationFailed,
    ReorderListSizeMismatch,
    ReorderListDuplicate,
    RevisionOverflow,
    InvalidSubscriptionUrl,
    RemoteProfileNeedsImport,
    RefreshInProgress,
    FetchSubscription { reason: PublicSubscriptionFetchKind },
    ProfileContentRejected { reason: PublicProfileContentKind },
    ProfileFileNotYaml,
    ProfileDeletedDuringRefresh,
    ProfileChangedDuringRefresh,
    FingerprintDefinition,
    ReadProfileFile { unsafe_path: bool },
    ProfileFileMissing,
    ReadExternalProfile { unsafe_path: bool },
    Materialization { unsafe_path: bool },
    VersionConflict,
    Commit { source: PublicCommitAborted },
    WorkflowNotReady,
    ServiceStopped,
    ShuttingDown,
}

/// Public commit abort phase discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublicCommitAbortedKind {
    WriteConfig,
    RecoverAfterWriteFailure,
    RuntimeRefused,
    RuntimeFailed,
    ValidateState,
}

/// Public summary of what became of the runtime after an aborted commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublicRuntimeAftermathKind {
    Untouched,
    RolledBack,
    RollbackFailed,
    Unknown,
}

/// Safe public commit-aborted decision metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PublicCommitAborted {
    pub kind: PublicCommitAbortedKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PublicRuntimeAftermathKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_causes: Vec<PublicRuntimeError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aftermath_cause: Option<Box<PublicRuntimeError>>,
}

/// Safe public transparent proxy mode discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublicTransparentProxyMode {
    Disabled,
    Redir,
    Tproxy,
}

/// Safe public transparent proxy validation discriminant without interface names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicTransparentProxyValidationKind {
    UnsupportedTarget,
    TunConflict,
    MissingListener { mode: PublicTransparentProxyMode },
    ZeroPort,
    EmptyTarget,
    InvalidInterface,
    DuplicateInterface,
    TooManyInterfaces,
}

/// Safe public runtime pipeline failure stage without script/file diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublicRuntimePipelineKind {
    ExecuteScript,
    SelectedProfileNotFound,
    SelectedProfileNotConfig,
    CompositionMemberInvalid,
    ContentSource,
    ParseProfile,
    Snapshot,
    Internal,
}

/// Safe public runtime build failure discriminant without script output logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicRuntimeBuildKind {
    StartScriptRunner,
    ValidateProfiles,
    ValidateTransparentProxy {
        reason: PublicTransparentProxyValidationKind,
    },
    TransparentProxyRequiresServiceMode,
    RunPipeline {
        stage: PublicRuntimePipelineKind,
    },
    TransformsFailed,
    SerializeFinalConfig,
    ConfigNotMapping,
    UnsupportedConfigValues,
}

/// Safe public port resolution discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicPortResolveKind {
    PortNotAvailable {
        field: PortField,
        port: u16,
    },
    NoOpenPort {
        field: PortField,
    },
    PortConflict {
        first: PortField,
        second: PortField,
        port: u16,
    },
}

/// Safe public core binary installation discriminant without filesystem paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicInstallCoreBinaryKind {
    AuthorizeCoreCopy {
        core: ClashCore,
    },
    StartElevatedCopy {
        core: ClashCore,
    },
    ElevatedCopyFailed {
        core: ClashCore,
        exit_code: Option<i32>,
    },
    PathNotUtf8,
}

/// Safe public runtime check unavailability discriminant without raw messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicRuntimeCheckUnavailableKind {
    NoEndpoint,
    HostUnsupported { host: ExecutionHost },
    CandidateUnavailable,
    Backend { core_kind: Option<CoreErrorKind> },
}

/// Safe public runtime error for callers without `SensitiveRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicRuntimeError {
    ShuttingDown,
    Isolated,
    OwnerUnresponsive,
    OwnerUnavailable,
    SourceSettled,
    ApplyRuntime {
        core_kind: Option<CoreErrorKind>,
    },
    StopCore {
        core_kind: Option<CoreErrorKind>,
    },
    RecoverRuntime {
        core_kind: Option<CoreErrorKind>,
    },
    RecoverServiceEndpoint {
        core_kind: Option<CoreErrorKind>,
    },
    RefreshStatus {
        core_kind: Option<CoreErrorKind>,
    },
    InstallService {
        core_kind: Option<CoreErrorKind>,
    },
    StartService {
        core_kind: Option<CoreErrorKind>,
    },
    StopService {
        core_kind: Option<CoreErrorKind>,
    },
    RestartService {
        core_kind: Option<CoreErrorKind>,
    },
    UninstallService {
        core_kind: Option<CoreErrorKind>,
    },
    MoveHost {
        host: ExecutionHost,
        core_kind: Option<CoreErrorKind>,
    },
    SubmissionUnobserved {
        core_kind: Option<CoreErrorKind>,
    },
    ServiceHostsCore,
    CoreNotStarted,
    RecoveryUnresolved,
    BuildRuntime {
        source: PublicRuntimeBuildKind,
    },
    PublishRuntime,
    ResolvePort {
        source: PublicPortResolveKind,
    },
    ResolveCoreBinary {
        core: ClashCore,
    },
    InstallCoreBinary {
        source: PublicInstallCoreBinaryKind,
    },
    PrepareServiceInstallPrompt,
    ReadCoreVersion {
        core: ClashCore,
    },
    UnsettledBaseline {
        gap: EvidenceGap,
    },
    CoreRejectedConfig {
        core_kind: Option<CoreErrorKind>,
    },
    CheckUnavailable {
        reason: PublicRuntimeCheckUnavailableKind,
    },
    CoreRolledBack,
    HandoffOwnerMismatch,
    HandoffNotRestored,
    RestoreFailed,
    CommittedAfterRefusal,
    NoRuntimeConfig,
    SerializeRuntimeConfig,
    ConvertRuntimeConfig,
    RuntimeSnapshotChanged,
    RuntimeNodeNotFound,
}

/// Safe public remote-access config rejection discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublicRemoteAccessErrorKind {
    PortZero,
    UseDedicatedCommand,
}

/// Safe public hotkey parse rejection discriminant without binding strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PublicHotkeyErrorKind {
    MalformedEntry,
    UnknownFunction,
    EmptyKeySegment,
    UnsupportedAccelerator,
    MissingSuperKey,
    DuplicateAccelerator,
}

/// Safe public configuration error for callers without `SensitiveRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicConfigError {
    LeaveNightlyChannel,
    InvalidUpdateSources,
    InvalidCoreLogs,
    InvalidPublicIpProbeSources,
    InvalidLatencyTimeout,
    InvalidRemoteAccess {
        reason: PublicRemoteAccessErrorKind,
    },
    ValidateHotkeyCapability,
    ValidateHotkeys {
        reason: PublicHotkeyErrorKind,
    },
    WorkflowNotReady,
    ShuttingDown,
    OwnerStopped,
    SessionStateStopped,
    InvalidTransparentProxy {
        reason: PublicTransparentProxyValidationKind,
    },
    TransparentProxyRequiresServiceMode,
    VersionConflict,
    Commit {
        source: PublicCommitAborted,
    },
    PersistSessionState,
}

/// Safe public storage operation discriminant without database paths or keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicStorageError {
    OpenDatabase,
    InspectDatabase,
    PreserveDatabase,
    BeginTransaction,
    OpenTable,
    ReadItem,
    WriteItem,
    RemoveItem,
    ListItems,
    CommitTransaction,
    DecodeValue,
    EncodeValue,
}

/// Safe public system DNS flush discriminant without host command paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicSystemDnsError {
    AuthorizeDnsFlush,
    RunFlushCommand,
    FlushRejected { exit_code: Option<i32> },
    Unsupported,
}

/// Safe public OS proxy error discriminant without proxy host strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicOsProxyError {
    ReadOsProxy,
    WriteOsProxy { enable: bool, port: u16 },
}

/// Safe public side-effects error discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicEffectsError {
    EffectsStopped,
}

/// Safe public HTTP server control error discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicHttpServerError {
    ReadStatusUnsupported,
    ChangeUnsupported,
    ReadStatus,
}

/// Safe public Clash read error discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicClashReadError {
    SettingsUnavailable,
    ProxiesUnavailable,
    UnknownGroup,
    DelayTestFailed,
    VersionUnavailable,
    RulesUnavailable,
    RuleProvidersUnavailable,
    ProxyProvidersUnavailable,
    StreamsUnavailable,
}

/// Safe public core log read/clear error discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicCoreLogReadError {
    InvalidRequest,
    CursorExpired,
    RecordGone,
    TooLarge,
    Unavailable,
}

/// Safe public profile sync read error discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicProfileSyncReadError {
    UnknownProfile,
    NotRemote,
    InvalidCursor,
    Unavailable,
}
