//! Shared command registry, audit kernel and experimental HTTP routes.

use std::{
    collections::HashMap,
    future::Future,
    hash::{BuildHasher as _, RandomState},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use nyanpasu_rpc_protocol::{
    COMMAND_CATALOG, CommandKind, CommandMetadata, RpcError, RpcOwner, RpcTarget, ShellCapability,
};
use serde_json::Value;

use nyanpasu_core::{client::NyanpasuClient, storage::Storage};

use crate::events::EventBus;

/// Explicit inputs for shared handlers. Declarations use `RpcState` and `RpcCaller`
/// for injected values that must not appear in request JSON or exported schemas.
pub struct RpcDependencies {
    pub icon_files: Option<Arc<dyn crate::commands::ports::HostIconFiles>>,
    pub core_directory: Option<Arc<dyn crate::commands::ports::CoreDirectoryResolver>>,
    pub service_prompt: Option<Arc<dyn crate::commands::ports::ServiceInstallPrompt>>,
    pub accent_color: Option<Arc<dyn crate::commands::ports::AccentColorQuery>>,
    pub host_environment: crate::commands::ports::HostEnvironment,
    pub client: NyanpasuClient,
    pub storage: Storage,
    pub paths: nyanpasu_paths::PathResolver,
    pub events: EventBus,
    pub audit: RpcAudit,
}

/// A borrowed dependency in a shared declaration, omitted from its wire schema.
pub struct RpcState<'a, T: ?Sized>(&'a T);

impl<T: ?Sized> std::ops::Deref for RpcState<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.0
    }
}

impl<T: ?Sized> specta::function::FunctionArg for RpcState<'_, T> {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

/// A transport-supplied caller in an owner declaration, omitted from its wire schema.
pub struct RpcCaller(RpcOwner);

impl RpcCaller {
    /// Preserve the caller label supplied by the transport without reconstructing it.
    pub fn into_owner(self) -> RpcOwner {
        self.0
    }
}

impl specta::function::FunctionArg for RpcCaller {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

/// The injected audit sink and the per-process key for HTTP caller identities.
pub struct RpcAudit {
    sink: Arc<dyn RpcAuditSink>,
    owner_key: RandomState,
}

impl RpcAudit {
    pub fn new(sink: Arc<dyn RpcAuditSink>) -> Self {
        Self {
            sink,
            owner_key: RandomState::new(),
        }
    }

    /// IPC callers are window labels. An HTTP owner carries the session cookie, which
    /// authorizes log sessions, so records get a keyed hash that still correlates
    /// one session's calls but never reveals the cookie.
    fn owner(&self, transport: RpcTransport, owner: &RpcOwner) -> RpcAuditOwner {
        match transport {
            RpcTransport::TauriIpc => RpcAuditOwner(owner.label().to_owned()),
            RpcTransport::Http => RpcAuditOwner(format!(
                "http:{:016x}",
                self.owner_key.hash_one(owner.label())
            )),
            RpcTransport::Local => RpcAuditOwner(format!(
                "local:{:016x}",
                self.owner_key.hash_one(owner.label())
            )),
        }
    }
}

/// A caller identity that is safe to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcAuditOwner(String);

impl RpcAuditOwner {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Synchronous observation only: implementations must not panic or queue work.
pub trait RpcAuditSink: Send + Sync + 'static {
    fn record(&self, record: &RpcAuditRecord);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcTransport {
    TauriIpc,
    Http,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcAuditOutcome {
    Success,
    Failure {
        kind: &'static str,
        code: Option<String>,
    },
    Cancelled,
    Panicked,
}

/// Local metadata only; this record is never serialized onto a transport.
#[derive(Debug, Clone)]
pub struct RpcAuditRecord {
    pub transport: RpcTransport,
    pub command: &'static str,
    pub owner: RpcAuditOwner,
    pub duration: Duration,
    pub outcome: RpcAuditOutcome,
}

struct RpcAuditGuard<'a> {
    sink: &'a dyn RpcAuditSink,
    record: RpcAuditRecord,
    started: Instant,
    completed: bool,
}

impl RpcAuditGuard<'_> {
    fn record(&mut self, outcome: RpcAuditOutcome) {
        self.record.duration = self.started.elapsed();
        self.record.outcome = outcome;
        self.completed = true;
        self.sink.record(&self.record);
    }
}

impl Drop for RpcAuditGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.record(if std::thread::panicking() {
                RpcAuditOutcome::Panicked
            } else {
                RpcAuditOutcome::Cancelled
            });
        }
    }
}

/// Observe dispatch without changing its result, polling, cancellation or unwind.
/// The name resolver runs under the guard and returns only registry or fixed names.
/// Aborting the process cannot produce a Drop record.
pub async fn dispatch_audited<T>(
    audit: &RpcAudit,
    transport: RpcTransport,
    owner: RpcOwner,
    command: impl FnOnce() -> &'static str,
    call: impl Future<Output = Result<T, RpcError>>,
) -> Result<T, RpcError> {
    let mut guard = RpcAuditGuard {
        sink: audit.sink.as_ref(),
        record: RpcAuditRecord {
            transport,
            command: "unknown_method",
            owner: audit.owner(transport, &owner),
            duration: Duration::ZERO,
            outcome: RpcAuditOutcome::Cancelled,
        },
        started: Instant::now(),
        completed: false,
    };
    guard.record.command = command();
    let result = call.await;
    guard.record(match &result {
        Ok(_) => RpcAuditOutcome::Success,
        Err(error) => RpcAuditOutcome::Failure {
            kind: error.kind,
            code: error.code.clone(),
        },
    });
    result
}

pub type RpcFuture = Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'static>>;

pub type RpcHandler = fn(Arc<RpcDependencies>, RpcOwner, Value) -> RpcFuture;
/// One declaration registers one neutral handler, including operation-bound commands.
/// The macro checks public requests, success types and method names before JSON erasure.
///
/// A matching declaration compiles (Result aliases use the same success assignment):
///
/// ```no_run
/// use nyanpasu_rpc_protocol::operations::SetStorageItem;
/// use nyanpasu_runtime_values::storage::StorageRevision;
/// type WriteResult = Result<StorageRevision, std::io::Error>;
/// #[nyanpasu_macro::rpc(local, result, operation = SetStorageItem)]
/// fn set_storage_item(key: String, value: String) -> WriteResult {
///     Ok(StorageRevision::INITIAL)
/// }
/// ```
///
/// Request fields must match the handler's parameters:
///
/// ```compile_fail,E0308
/// use nyanpasu_rpc_protocol::operations::SetStorageItem;
/// use nyanpasu_runtime_values::storage::StorageRevision;
/// fn operation_request_mismatch_fails_to_compile() {
///     #[nyanpasu_macro::rpc(local, operation = SetStorageItem)]
///     fn set_storage_item(key: u64, value: String) -> StorageRevision {
///         StorageRevision::INITIAL
///     }
/// }
/// ```
///
/// Successful values must have the contract's sending type:
///
/// ```compile_fail,E0308
/// use nyanpasu_rpc_protocol::operations::SetStorageItem;
/// fn operation_response_mismatch_fails_to_compile() {
///     #[nyanpasu_macro::rpc(local, operation = SetStorageItem)]
///     fn set_storage_item(key: String, value: String) -> bool { true }
/// }
/// ```
///
/// The name is checked in a constant, including declarations never called:
///
/// ```compile_fail,E0080
/// use nyanpasu_rpc_protocol::operations::SetStorageItem;
/// use nyanpasu_runtime_values::storage::StorageRevision;
/// fn operation_method_mismatch_fails_to_compile() {
///     #[nyanpasu_macro::rpc(local, operation = SetStorageItem)]
///     fn different_method(key: String, value: String) -> StorageRevision {
///         StorageRevision::INITIAL
///     }
/// }
/// ```
pub struct CommandEntry {
    pub metadata: CommandMetadata,
    pub handler: RpcHandler,
}

inventory::collect!(CommandEntry);

#[derive(Clone)]
pub(crate) struct RegisteredCommand {
    metadata: CommandMetadata,
    capabilities_available: bool,
    handler: Option<RpcHandler>,
}

impl RegisteredCommand {
    fn unavailable(&self, target: RpcTarget) -> RpcError {
        if target == RpcTarget::Http {
            RpcError::unsupported(self.metadata.name)
        } else {
            RpcError::new(
                "unsupported",
                format!(
                    "command `{}` is unavailable in this shell",
                    self.metadata.name
                ),
            )
        }
    }

    fn neutral_handler(&self, target: RpcTarget) -> Result<RpcHandler, RpcError> {
        if !self.metadata.targets.contains(&target) || !self.capabilities_available {
            return Err(self.unavailable(target));
        }
        if target == RpcTarget::Desktop && self.metadata.kind == CommandKind::Native {
            return Err(self.unavailable(target));
        }
        self.handler.ok_or_else(|| self.unavailable(target))
    }
}

pub(crate) type CommandHandlers = Arc<HashMap<&'static str, RegisteredCommand>>;

pub(crate) fn register_commands(
    catalog: &[CommandMetadata],
    entries: impl IntoIterator<Item = &'static CommandEntry>,
    provided: &[ShellCapability],
) -> Result<CommandHandlers, SetupError> {
    let mut commands = HashMap::new();
    for metadata in catalog {
        if commands
            .insert(
                metadata.name,
                RegisteredCommand {
                    metadata: *metadata,
                    capabilities_available: false,
                    handler: None,
                },
            )
            .is_some()
        {
            return Err(SetupError::RegisterCommand {
                name: metadata.name,
            });
        }
    }
    for entry in entries {
        let command = commands
            .entry(entry.metadata.name)
            .or_insert(RegisteredCommand {
                metadata: entry.metadata,
                capabilities_available: false,
                handler: None,
            });
        if command.handler.replace(entry.handler).is_some() {
            return Err(SetupError::RegisterCommand {
                name: entry.metadata.name,
            });
        }
    }

    // Validate every linked name before checking availability. Missing facilities
    // never erase a known name, and optional effects are not admission requirements.
    for command in commands.values_mut() {
        command.capabilities_available = command
            .metadata
            .capabilities
            .iter()
            .all(|capability| provided.contains(capability));
    }
    Ok(Arc::new(commands))
}

#[derive(Clone)]
pub struct UnifiedRpc {
    pub(crate) dependencies: Arc<RpcDependencies>,
    pub(crate) commands: CommandHandlers,
}

impl UnifiedRpc {
    pub fn new(
        dependencies: RpcDependencies,
        provided: &[ShellCapability],
    ) -> Result<Self, SetupError> {
        let commands =
            register_commands(COMMAND_CATALOG, inventory::iter::<CommandEntry>, provided)?;
        Ok(Self {
            dependencies: Arc::new(dependencies),
            commands,
        })
    }

    pub fn command_names(&self) -> Vec<&'static str> {
        let mut names = self.commands.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    #[cfg(feature = "local-server")]
    pub(crate) fn command_capabilities_available(&self, name: &str) -> bool {
        self.commands
            .get(name)
            .is_some_and(|command| command.capabilities_available)
    }

    #[cfg(all(test, feature = "local-server"))]
    pub(crate) fn enable_local_test_command(&mut self, name: &str, handler: Option<RpcHandler>) {
        let command = Arc::make_mut(&mut self.commands)
            .get_mut(name)
            .expect("registered test command");
        command.metadata.targets = &[RpcTarget::Desktop, RpcTarget::Http, RpcTarget::Local];
        if let Some(handler) = handler {
            command.handler = Some(handler);
        }
    }

    /// Dispatch a shared command, including desktop fallback for unlinked native names.
    pub async fn call(
        &self,
        transport: RpcTransport,
        method: &str,
        owner: RpcOwner,
        params: Value,
    ) -> Result<Value, RpcError> {
        self.call_with_admission(transport, method, owner, params, Ok(()))
            .await
    }

    pub(crate) async fn call_with_admission(
        &self,
        transport: RpcTransport,
        method: &str,
        owner: RpcOwner,
        params: Value,
        admission: Result<(), RpcError>,
    ) -> Result<Value, RpcError> {
        let target = match transport {
            RpcTransport::TauriIpc => RpcTarget::Desktop,
            RpcTransport::Http => RpcTarget::Http,
            RpcTransport::Local => RpcTarget::Local,
        };
        dispatch_audited(
            &self.dependencies.audit,
            transport,
            owner.clone(),
            || self.audit_command_name(method),
            async {
                let handler = self
                    .commands
                    .get(method)
                    .ok_or_else(|| RpcError::unknown_method(method))?;
                let handler = handler.neutral_handler(target)?;
                admission?;
                handler(self.dependencies.clone(), owner, params).await
            },
        )
        .await
    }

    /// Admit and audit a shell-bound invocation without accepting any shell handles.
    /// The callback is invoked only after admission, under the same audit guard as
    /// shared calls, so parameter failures and panics retain their existing semantics.
    pub async fn call_native(
        &self,
        method: &str,
        owner: RpcOwner,
        call: impl FnOnce() -> RpcFuture,
    ) -> Result<Value, RpcError> {
        dispatch_audited(
            &self.dependencies.audit,
            RpcTransport::TauriIpc,
            owner,
            || self.audit_command_name(method),
            async {
                let command = self
                    .commands
                    .get(method)
                    .ok_or_else(|| RpcError::unknown_method(method))?;
                if command.metadata.kind != CommandKind::Native
                    || !command.metadata.targets.contains(&RpcTarget::Desktop)
                    || !command.capabilities_available
                {
                    return Err(command.unavailable(RpcTarget::Desktop));
                }
                call().await
            },
        )
        .await
    }

    fn audit_command_name(&self, method: &str) -> &'static str {
        self.commands
            .get_key_value(method)
            .map_or("unknown_method", |(name, _)| *name)
    }
}

/// Failure to install routes or register a unique command name.
#[derive(Debug, snafu::Snafu)]
pub enum SetupError {
    #[snafu(display("HTTP routes already installed"))]
    InstallRoutes,
    #[snafu(display("duplicate RPC command: {name}"))]
    RegisterCommand { name: &'static str },
}

#[cfg(all(test, feature = "http"))]
mod audit_tests;
#[cfg(all(test, feature = "http"))]
mod tests;

/// Verify linked shared/native metadata and known unlinked names in a host test.
#[cfg(feature = "test-support")]
pub fn assert_command_category_inventory() {
    let commands = register_commands(
        COMMAND_CATALOG,
        inventory::iter::<CommandEntry>,
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    for metadata in COMMAND_CATALOG {
        let command = &commands[metadata.name];
        let entry = inventory::iter::<CommandEntry>
            .into_iter()
            .find(|entry| entry.metadata.name == metadata.name)
            .unwrap();
        assert_eq!(
            entry.metadata, *metadata,
            "{} catalog metadata",
            metadata.name
        );
        assert!(
            command.handler.is_some(),
            "{} neutral handler",
            metadata.name
        );
        if metadata.kind == CommandKind::Shared {
            for target in metadata.targets {
                assert!(command.neutral_handler(*target).is_ok());
            }
        }
    }
    let unlinked = register_commands(COMMAND_CATALOG, [], &[]).unwrap();
    assert_eq!(unlinked.len(), COMMAND_CATALOG.len());
    for metadata in COMMAND_CATALOG
        .iter()
        .filter(|metadata| metadata.kind == CommandKind::Native)
    {
        let command = &unlinked[metadata.name];
        assert_eq!(command.metadata, *metadata);
        assert_eq!(
            command
                .neutral_handler(RpcTarget::Desktop)
                .unwrap_err()
                .kind,
            "unsupported"
        );
    }
}
