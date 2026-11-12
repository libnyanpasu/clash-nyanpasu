//! Receiving-only error and mutation shapes. Skipped operational sources are deliberately absent.
//!
//! These values describe what the remote host reported; they cannot reconstruct its IO,
//! engine or actor failures. They are not registered as additional frontend schema types.

use std::sync::Arc;

use nyanpasu_config::{
    application::{ClashCore, ReleaseChannel},
    clash::{config::TransparentProxyValidationError, features::UnsupportedConfigValue},
    profile::{ExternalProfilePath, ManagedProfilePath, ProfileId, ProfileValidationError},
    runtime::executor::{ScriptExecutionError, TransformFailure},
};
use nyanpasu_contracts::{
    elevation::ElevationConsentError, profiles::error::ExpectedNode, service::ServiceCommand,
};
use nyanpasu_core_metadata::CoreErrorKind;
use nyanpasu_profile_transform::RuntimeBuildLog;
use nyanpasu_runtime_values::{
    check::{EvidenceGap, RuntimeCheckUnavailable},
    effects::EffectsError,
    execution_host::ExecutionHost,
    hotkey::HotkeyAction,
    mutation::ConfigDomain,
    ports::PortResolveError,
    profiles::MaterializationOperation,
    runtime::{CommitReceipt, DegradationPhase, InterruptFailure},
    runtime_error::{CoreFailure, RestoreFailure},
};

/// An owned error envelope; the domain payload is decoded according to its command.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RpcError {
    pub kind: String,
    pub message: String,
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub operation_id: Option<String>,
    pub domain_error: Option<serde_json::Value>,
}

impl RpcError {
    /// Decode a present domain payload using the receiving type for its operation.
    ///
    /// Absence stays `None`; an incompatible payload returns the serde error.
    pub fn decode_domain_error<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<Option<T>, serde_json::Error> {
        self.domain_error
            .clone()
            .map(serde_json::from_value)
            .transpose()
    }
}

/// A failure of an operation on the running core or the runtime workflow.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeError {
    /// Refused at admission: nothing ran.
    ShuttingDown,
    /// Refused at admission: nothing ran.
    Isolated,
    /// The workflow took the command and never answered, so it may have run.
    OwnerUnresponsive {
        operation_id: String,
    },
    /// Refused at admission: the runtime owner is gone, so nothing ran.
    OwnerUnavailable,
    /// Refused at admission: the source transaction had already been decided.
    SourceSettled,
    ApplyRuntime {
        failure: CoreFailure,
    },
    StopCore {
        failure: CoreFailure,
    },
    RecoverRuntime {
        failure: CoreFailure,
    },
    RecoverServiceEndpoint {
        failure: CoreFailure,
    },
    RefreshStatus {
        failure: CoreFailure,
    },
    InstallService {
        failure: CoreFailure,
    },
    StartService {
        failure: CoreFailure,
    },
    StopService {
        failure: CoreFailure,
    },
    RestartService {
        failure: CoreFailure,
    },
    UninstallService {
        failure: CoreFailure,
    },
    MoveHost {
        host: ExecutionHost,

        failure: CoreFailure,
    },
    /// The service hosts the runtime; the local host must take it over first.
    ServiceHostsCore,
    /// An explicit start that could not put the core on its host; `reason` is
    /// the diagnostic text of the convergence that gave up.
    CoreNotStarted {
        reason: String,
        retryable: bool,
    },
    /// An explicit recovery that could not settle the runtime.
    RecoveryUnresolved {
        reason: String,
    },
    BuildRuntime {
        source: RuntimeBuildError,
    },
    PublishRuntime {
        source: PublishRuntimeError,
    },
    ResolvePort {
        source: PortResolveError,
    },
    ResolveCoreBinary {
        source: CoreSpecError,
    },
    InstallCoreBinary {
        source: InstallCoreBinaryError,
    },
    PrepareServiceInstallPrompt {
        source: ServiceCommandError,
    },
    ReadCoreVersion {
        source: CoreVersionError,
    },
    /// A candidate was refused before anything was submitted, for want of a
    /// baseline to apply against.
    UnsettledBaseline {
        gap: EvidenceGap,
    },
    CoreRejectedConfig {
        core_kind: Option<CoreErrorKind>,
        message: String,
    },
    CheckUnavailable {
        reason: RuntimeCheckUnavailable,
    },
    /// The core restored its own previous configuration.
    CoreRolledBack {
        reason: Option<String>,
    },
    /// The submission ended unobserved, so it may have taken effect.
    SubmissionUnobserved {
        failure: CoreFailure,
    },
    HandoffOwnerMismatch {
        expected: ExecutionHost,
    },
    /// A mutation that moved the runtime to the other host and failed there
    /// could not put it back.
    HandoffNotRestored {
        failure: RestoreFailure,
    },
    /// A cancelled mutation could not put the runtime back.
    RestoreFailed {
        failure: RestoreFailure,
    },
    /// The store committed a mutation whose runtime target the workflow had
    /// refused.
    CommittedAfterRefusal {
        operation_id: String,
    },
    /// No runtime configuration has been built yet.
    NoRuntimeConfig,
    SerializeRuntimeConfig {},
    ConvertRuntimeConfig {},
    /// A newer build replaced the snapshot the inspection was opened on.
    RuntimeSnapshotChanged,
    RuntimeNodeNotFound {
        node_id: u32,
    },
}

/// Downloaded or read content that is not a valid profile document.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileContentError {
    NotYamlMapping {},
    ReserializeYaml {},
    MissingProxies,
    EmptyScript,
}

/// What a profile command failed with.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfilesError {
    ProfileNotFound {
        uid: ProfileId,
    },
    ProfileHasNoFile {
        uid: ProfileId,
    },
    NotARemoteProfile {
        uid: ProfileId,
    },
    ProfileFileNotWritable {
        uid: ProfileId,
    },
    ProfileInUse {
        uid: ProfileId,
        referrers: Vec<ProfileId>,
        /// Referenced by the document-level `current` selection.
        current: bool,
        /// Referenced by the document-level `global_transforms` list.
        global_transforms: bool,
    },
    ProfileIdCollision {
        uid: ProfileId,
    },
    ValidationFailed {
        errors: Vec<ProfileValidationError>,
    },
    ReorderListSizeMismatch {
        expected: usize,
        got: usize,
    },
    ReorderListDuplicate {
        uid: ProfileId,
    },
    RevisionOverflow {},
    InvalidSubscriptionUrl {
        url: String,
    },
    RemoteProfileNeedsImport,
    RefreshInProgress {
        uid: ProfileId,
    },
    FetchSubscription {
        url: url::Url,
        source: SubscriptionFetchError,
    },
    ProfileContentRejected {
        source: ProfileContentError,
    },
    ProfileDeletedDuringRefresh {
        uid: ProfileId,
    },
    ProfileChangedDuringRefresh {
        uid: ProfileId,
    },
    FingerprintDefinition {
        uid: ProfileId,
    },
    ReadProfileFile {
        uid: ProfileId,
        source: ProfileFileError,
    },
    ProfileFileNotYaml {
        uid: ProfileId,
        source: ProfileContentError,
    },
    ProfileFileMissing {
        uid: ProfileId,
        path: String,
    },
    ReadExternalProfile {
        target: ExternalProfilePath,
        source: ProfileFileError,
    },
    Materialization {
        operation: MaterializationOperation,
        source: ProfileFileError,
    },
    VersionConflict {
        expected: u64,
        actual: u64,
    },
    Commit {
        /// The profile the change was about, when it was about one.
        uid: Option<ProfileId>,
        /// That profile's file.
        path: Option<ManagedProfilePath>,
        source: CommitAborted,
    },
    WorkflowNotReady {},
    ProfilesActorStopped,
    ProfilesReplyDropped,
    BlockingTaskCancelled {},
    ShuttingDown,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Committed {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
    },
    CommittedDegraded {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
        degradations: Vec<Degradation>,
    },
}

/// Structured committed-degraded detail surfaced over IPC / Specta.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Degradation {
    pub phase: DegradationPhase,
    pub reason: DegradationReason,
    /// The diagnostic text, for logs and the copied details; the frontend
    /// localizes `reason`.
    pub message: String,
    pub retryable: bool,
}

/// Why a committed mutation is degraded. The frontend localizes each variant.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum DegradationReason {
    /// The runtime owner stopped before the mutation settled, or settled it as
    /// needing recovery.
    RuntimeRecoveryRequired {
        operation_id: Option<String>,
        cause: Option<Arc<RuntimeError>>,
    },
    /// The runtime will apply the committed mutation later.
    RuntimeDeferred {
        cause: Arc<RuntimeError>,
    },
    RuntimeProductPublishFailed {
        cause: Arc<RuntimeError>,
    },
    ServiceStopFailed {
        cause: Arc<RuntimeError>,
    },
    ModeInterruptionFailed {
        cause: InterruptFailure,
    },
    ProfileInterruptionFailed {
        cause: InterruptFailure,
    },
    ProxyInterruptionFailed {
        cause: InterruptFailure,
    },
    ProxyCacheRefreshFailed,
    JournalInvalid,
    JobsJournalUnavailable,
    MaterializationDeferred,
    CleanupDeferred,
    ProfileAutoActivationFailed {
        profile: ProfileId,
        cause: Arc<ProfilesError>,
    },
}

/// Why a source transaction did not commit, classified once from the
/// persistence error and, when the Runtime took part, the receipt it settled
/// (U7). A source wraps it in its own domain error.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommitAborted {
    /// The yaml write failed; nothing committed.
    WriteConfig { runtime: RuntimeAftermath },
    /// The write failed and so did the recovery of what the transaction staged.
    RecoverAfterWriteFailure { runtime: RuntimeAftermath },
    /// A required participant refused the candidate.
    RuntimeRefused {
        errors: Vec<Arc<RuntimeError>>,
        runtime: RuntimeAftermath,
    },
    /// A required participant could not decide.
    RuntimeFailed {
        errors: Vec<Arc<RuntimeError>>,
        runtime: RuntimeAftermath,
    },
    /// The coordinator refused before persisting: builder validation, or a CAS
    /// mismatch the caller did not classify as its own version conflict.
    ValidateState {},
}

/// What became of the runtime after an aborted commit, from the receipt's
/// structured fields. `detail` is the receipt's operator diagnostics, never an
/// input to a decision.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeAftermath {
    /// The runtime took no part, or the transaction withdrew before it did.
    Untouched,
    /// The runtime went back to the previous configuration.
    RolledBack,
    /// Rolling the runtime back failed; recovery is required.
    RollbackFailed { detail: Arc<RuntimeError> },
    /// What the runtime is running is unknown and needs recovery.
    Unknown { detail: Arc<RuntimeError> },
}

/// A failure of locating the binary a core is started from.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreSpecError {
    FindCoreBinary { core: ClashCore },
}

/// A failure of publishing the derived runtime config file.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublishRuntimeError {
    CreateRuntimeDirectory { path: String },
    WriteRuntimeConfig { path: String },
}

/// A failure of building a runtime candidate from source config.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeBuildError {
    StartScriptRunner {},
    ValidateProfiles {
        errors: Vec<ProfileValidationError>,
    },
    ValidateTransparentProxy {
        source: TransparentProxyValidationError,
    },
    TransparentProxyRequiresServiceMode,
    RunPipeline {
        source: RuntimePipelineError,
    },
    TransformsFailed {
        failures: Vec<TransformFailure>,
        logs: Vec<RuntimeBuildLog>,
    },
    SerializeFinalConfig {},
    ConfigNotMapping,
    /// The profiles or transforms wrote values the selected core rejects.
    UnsupportedConfigValues {
        values: Vec<UnsupportedConfigValue>,
    },
}

/// Serialized shape for the app's error channel: library sources are skipped and
/// reach the user only through the error's `Debug` detail.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimePipelineError {
    ExecuteScript {
        source: ScriptExecutionError,
    },
    SelectedProfileNotFound {
        profile: ProfileId,
    },
    SelectedProfileNotConfig {
        profile: ProfileId,
    },
    CompositionMemberInvalid {
        composition: ProfileId,
        member: ProfileId,
        reason: String,
    },
    ContentSource {
        profile: ProfileId,
        path: ManagedProfilePath,
    },
    ParseProfile {
        profile: ProfileId,
        message: String,
    },
    Snapshot {},
    /// Theoretically unreachable invariant breaks (e.g. guard serialization).
    Internal {
        message: String,
    },
}

/// A failure of the profile filesystem, its materialization journals or its
/// private storage. One type for both the profile filesystem port and the
/// materialization port, whose implementations share their helpers.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileFileError {
    PreserveDerivedDirectory {},
    InspectPath {
        path: String,
    },
    ReadFile {
        path: String,
    },
    ReadLink {
        path: String,
    },
    WriteFile {
        path: String,
    },
    AtomicWrite {
        path: String,
    },
    CreateDirectory {
        path: String,
    },
    RemoveFile {
        path: String,
    },
    ReplaceFile {
        from: String,
        to: String,
    },
    CreateSymlink {
        link: String,
        target: String,
    },
    ListDirectory {
        path: String,
    },
    // Constructed by the unix-only halves of the adapter.
    SyncDirectory {
        path: String,
    },
    // Constructed by the unix-only halves of the adapter.
    SetPermissions {
        path: String,
    },
    // Constructed by the windows-only private-root check.
    CanonicalizePath {
        path: String,
    },
    ReadExternalTarget {
        target: ExternalProfilePath,
    },
    ParseJournal {
        path: String,
    },
    SerializeJournal {},
    ReservedPath {
        path: String,
    },
    PathEscapesProfilesDir {
        path: String,
        root: String,
    },
    NoParentDirectory {
        path: String,
    },
    UnexpectedNode {
        path: String,
        expected: ExpectedNode,
    },
    UnexpectedSymlink {
        path: String,
    },
    ExistingFileBlocksSymlink {
        path: String,
    },
    SymlinkTargetNotUtf8 {
        path: String,
    },
    InvalidExternalPath {},
    // Constructed by the windows-only tombstone move.
    CleanupTombstoneExists {
        path: String,
    },
    JournalNotFound {
        operation_id: String,
    },
    CompensationFenced {
        path: String,
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
}

/// A failure to download a subscription. The URL is not repeated here: the
/// caller that knows it names it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubscriptionFetchError {
    BuildHttpClient {},
    RequestSubscription {},
    SubscriptionHttpStatus { status: u16 },
    ReadSubscriptionBody {},
}

/// A failure of installing a downloaded core binary over the installed one.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallCoreBinaryError {
    AuthorizeCoreCopy {
        core: ClashCore,
        destination: String,
        source: ElevationConsentError,
    },
    StartElevatedCopy {
        core: ClashCore,
        destination: String,
    },
    ElevatedCopyFailed {
        core: ClashCore,
        destination: String,
        exit_code: Option<i32>,
    },
    // Constructed by the windows-only elevated copy, which needs UTF-8 paths.
    PathNotUtf8 {
        path: String,
    },
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreVersionError {
    RunCoreVersion { core: ClashCore },
    CoreVersionExit { core: ClashCore },
    CoreVersionNotReported { core: ClashCore },
}

/// A failure of running or querying the system service, including the bounds
/// the service actor puts on every call.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceCommandError {
    AuthorizeServiceCommand {
        command: ServiceCommand,
        source: ElevationConsentError,
    },
    SessionEnding,
    ResolveServiceUser {},
    ResolveServiceDirs {},
    RunElevated {
        command: ServiceCommand,
    },
    ServiceCommandExit {
        command: ServiceCommand,
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    RunServiceStatus {},
    ServiceStatusExit {
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    ServiceStatusNotUtf8 {},
    ParseServiceStatus {},
    TimedOut {
        limit_ms: u64,
    },
    StillRunning {
        limit_ms: u64,
    },
    TaskCancelled,
}

/// Why the system DNS cache could not be flushed. No variant is compiled out
/// per platform, so the generated bindings do not depend on the build host.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemDnsError {
    AuthorizeDnsFlush {
        source: ElevationConsentError,
    },
    /// The elevated flush could not be started: the user declined the prompt,
    /// or the platform has no way to ask for elevation.
    RunFlushCommand {
        command: String,
    },
    /// The flush ran and reported failure; declining the macOS authorization
    /// dialog lands here too.
    FlushRejected {
        command: String,
        code: Option<i32>,
    },
    Unsupported,
}

/// Why the platform proxy settings could not be read or written. The platform's
/// own error stays in `source`, boxed because this module does not name the
/// crate that produced it; it reaches the user through the copied detail.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OsProxyError {
    ReadOsProxy {},
    WriteOsProxy {
        enable: bool,
        host: String,
        port: u16,
    },
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigError {
    /// A nightly build keeps its channel.
    LeaveNightlyChannel {
        to: ReleaseChannel,
    },
    InvalidUpdateSources {
        reason: String,
    },
    InvalidCoreLogs {
        reason: String,
    },
    InvalidPublicIpProbeSources {
        reason: String,
    },
    InvalidTransparentProxy {
        source: TransparentProxyValidationError,
    },
    TransparentProxyRequiresServiceMode,
    InvalidLatencyTimeout {
        reason: String,
    },
    ValidateHotkeys {
        source: HotkeyParseError,
    },
    ValidateHotkeyCapability,
    WorkflowNotReady {},
    ShuttingDown {
        domain: ConfigDomain,
    },
    /// The owner's mailbox is closed, or it dropped the reply.
    OwnerStopped {
        domain: ConfigDomain,
    },
    VersionConflict {
        domain: ConfigDomain,
        expected: u64,
        actual: u64,
    },
    Commit {
        domain: ConfigDomain,
        source: CommitAborted,
    },
    PersistSessionState {},
    SessionStateStopped,
}

/// What a storage operation failed with. Library causes stay in `source`
/// (skipped on the wire); they reach the user only through the copied detail.
/// A `key` is the storage key, which the web layer prefixes.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StorageOperationError {
    OpenDatabase { path: String },
    InspectDatabase { path: String },
    PreserveDatabase {},
    BeginTransaction {},
    OpenTable {},
    ReadItem { key: String },
    WriteItem { key: String },
    RemoveItem { key: String },
    ListItems {},
    CommitTransaction {},
    DecodeValue { key: String },
    EncodeValue { key: String },
}

/// Why a hotkey list could not be accepted. Rejected before anything is
/// committed, so every variant names the entry the user has to fix.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HotkeyParseError {
    /// Not the `"<function>,<accelerator>"` shape.
    MalformedEntry {
        entry: String,
    },
    UnknownFunction {
        function: String,
    },
    /// A `+` separated accelerator with an empty segment.
    EmptyKeySegment {
        accelerator: String,
    },
    /// The platform's parser refused it; its text, which names the offending
    /// key, reaches the user only through the copied detail. Boxed because
    /// this module does not name the plugin.
    UnsupportedAccelerator {
        accelerator: String,
    },
    MissingSuperKey {
        accelerator: String,
    },
    /// The same accelerator was bound to two functions.
    DuplicateAccelerator {
        accelerator: String,
        first: HotkeyAction,
        second: HotkeyAction,
    },
}

/// Why a debug HTTP query or setting change could not be performed.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DebugHttpControlError {
    ReadStatusUnsupported,
    ChangeEnabledUnsupported,
    ReadStatus {},
    ChangeEnabled {},
}

/// A failed command as the frontend receives it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct IpcError {
    /// The domain failure; the frontend localizes it.
    pub kind: IpcErrorKind,
    /// The error's own message, shown when `kind` cannot be localized.
    pub message: String,
    /// The original error, copied by the user for diagnosis.
    pub detail: String,
}

/// The domain a command failed in. A domain joins once its errors are typed.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "domain", content = "error", rename_all = "snake_case")]
pub enum IpcErrorKind {
    /// Not classified into a domain; only `message` describes it.
    Unknown,
    Profiles(Box<ProfilesError>),
    Runtime(Box<RuntimeError>),
    Config(Box<ConfigError>),
    Storage(Box<StorageOperationError>),
    SystemDns(Box<SystemDnsError>),
    SystemProxy(Box<OsProxyError>),
    Effects(Box<EffectsError>),
    DebugHttp(Box<DebugHttpControlError>),
}
