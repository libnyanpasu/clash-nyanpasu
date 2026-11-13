//! Shared command registry, audit kernel and experimental HTTP routes.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use nyanpasu_rpc_protocol::{
    AuthContext, COMMAND_CATALOG, CommandKind, CommandMetadata, Principal, RpcDescribeResponse,
    RpcError, RpcOwner, RpcTarget, ServerInfo, ShellCapability,
};
use serde_json::Value;

use nyanpasu_core::{client::NyanpasuClient, storage::Storage};

use crate::{
    audit::{
        AuditInvocation, AuditPhase, CompletionSummary, RpcAudit, RpcAuditGuard, RpcAuditOutcome,
        RpcTransport,
    },
    events::EventBus,
};

/// Explicit inputs for shared handlers. Declarations use `RpcState` and `RpcCaller`
/// for injected values that must not appear in request JSON or exported schemas.
pub struct RpcDependencies {
    pub icon_files: Option<Arc<dyn crate::ports::HostIconFiles>>,
    pub core_directory: Option<Arc<dyn crate::ports::CoreDirectoryResolver>>,
    pub service_prompt: Option<Arc<dyn crate::ports::ServiceInstallPrompt>>,
    pub accent_color: Option<Arc<dyn crate::ports::AccentColorQuery>>,
    pub host_environment: crate::ports::HostEnvironment,
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

/// Transport-resolved caller identity, scopes, and owner label omitted from wire schemas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcCaller {
    auth: AuthContext,
    owner: RpcOwner,
}

impl RpcCaller {
    /// Construct a caller combining resolved authentication context and owner label.
    #[must_use]
    pub fn new(auth: AuthContext, owner: RpcOwner) -> Self {
        Self { auth, owner }
    }

    /// Construct a full-scope desktop window caller for in-process GUI dispatches.
    #[must_use]
    pub fn desktop(window: impl Into<String>) -> Self {
        let window = window.into();
        let owner = RpcOwner::desktop(&window);
        Self {
            auth: AuthContext::desktop(window),
            owner,
        }
    }

    /// Resolved principal and granted scopes for policy admission and `rpc_describe`.
    #[must_use]
    pub fn auth(&self) -> &AuthContext {
        &self.auth
    }

    /// Keyed resource owner label for log/session ownership correlation.
    #[must_use]
    pub fn owner(&self) -> &RpcOwner {
        &self.owner
    }

    /// Preserve the caller label supplied by the transport without reconstructing it.
    #[must_use]
    pub fn into_owner(self) -> RpcOwner {
        self.owner
    }
}

impl specta::function::FunctionArg for RpcCaller {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

const RPC_DESCRIBE_PROTOCOL_VERSION: u32 = nyanpasu_rpc_protocol::frame::PROTOCOL_VERSION;

/// Host and caller snapshot injected into [`crate::commands::describe::rpc_describe`].
pub struct RpcDescribeContext {
    response: RpcDescribeResponse,
}

impl RpcDescribeContext {
    /// Consume the injected snapshot to return the canonical `rpc_describe` response.
    #[must_use]
    pub fn into_response(self) -> RpcDescribeResponse {
        self.response
    }
}

impl specta::function::FunctionArg for RpcDescribeContext {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

/// Dispatch metadata injected into neutral command wrappers.
#[derive(Clone)]
pub struct RpcCallMeta {
    server: ServerInfo,
    target: RpcTarget,
    commands: CommandHandlers,
}

impl RpcCallMeta {
    /// Build a caller-scoped self-description snapshot for `rpc_describe`.
    #[must_use]
    pub fn describe_context(&self, auth: &AuthContext) -> RpcDescribeContext {
        RpcDescribeContext {
            response: RpcDescribeResponse {
                protocol: RPC_DESCRIBE_PROTOCOL_VERSION,
                server: self.server.clone(),
                transport: self.target,
                principal: auth.principal.clone(),
                scopes: auth.scopes.clone(),
                commands: crate::policy::describe_commands(&self.commands, self.target, auth),
            },
        }
    }
}

/// Observe dispatch without changing its result, polling, cancellation or unwind.
/// The name resolver returns only registry or fixed names. Preparation errors are
/// rejected before admission; prepared futures return their own completion metadata.
/// Aborting the process cannot produce a Drop record.
pub async fn dispatch_audited<T, F>(
    audit: &RpcAudit,
    transport: RpcTransport,
    owner: RpcOwner,
    principal: Option<&Principal>,
    command: impl FnOnce() -> &'static str,
    request_id: Option<nyanpasu_rpc_protocol::frame::RequestId>,
    prepare: impl FnOnce() -> Result<F, RpcError>,
) -> Result<T, RpcError>
where
    F: Future<Output = RpcCompletion<T>>,
{
    let invocation = audit.invocation(transport, &owner, principal, command(), request_id);
    dispatch_with_audit(&invocation, prepare).await
}

/// Use adapter-supplied correlation without giving the invocation transport ownership.
pub(crate) async fn dispatch_with_audit<T, F>(
    invocation: &AuditInvocation,
    prepare: impl FnOnce() -> Result<F, RpcError>,
) -> Result<T, RpcError>
where
    F: Future<Output = RpcCompletion<T>>,
{
    let mut guard = RpcAuditGuard::new(invocation);
    let call = match prepare() {
        Ok(call) => call,
        Err(error) => {
            guard.record(AuditPhase::Rejected {
                kind: error.kind,
                code: error.code.clone(),
            });
            return Err(error);
        }
    };
    guard.admit();
    let RpcCompletion { result, completion } = call.await;
    let outcome = match &result {
        Ok(_) => RpcAuditOutcome::Success,
        Err(error) => RpcAuditOutcome::Failure {
            kind: error.kind,
            code: error.code.clone(),
        },
    };
    guard.record(AuditPhase::Finished {
        outcome,
        completion,
    });
    result
}

/// A handler result with metadata extracted before response encoding, even on encode failure.
pub struct RpcCompletion<T> {
    pub result: Result<T, RpcError>,
    pub completion: CompletionSummary,
}

impl<T> RpcCompletion<T> {
    /// Wrap a result whose declaration does not explicitly report mutation receipts.
    #[must_use]
    pub fn unreported(result: Result<T, RpcError>) -> Self {
        Self {
            result,
            completion: CompletionSummary::NotReported,
        }
    }
}

/// Adapt a direct route without reporting completion metadata or changing its polling.
pub async fn unreported<T>(call: impl Future<Output = Result<T, RpcError>>) -> RpcCompletion<T> {
    RpcCompletion::unreported(call.await)
}

/// A prepared handler, polled inline without cancelling owner-started work on drop.
pub type RpcFuture = Pin<Box<dyn Future<Output = RpcCompletion<Value>> + Send + 'static>>;

/// Decode and bind inputs before returning the future that executes the handler.
pub type RpcHandler =
    fn(Arc<RpcDependencies>, RpcCallMeta, RpcCaller, Value) -> Result<RpcFuture, RpcError>;
/// One declaration registers one neutral handler, including operation-bound commands.
/// The macro checks public requests, success types and method names before JSON erasure.
///
/// A matching declaration compiles (Result aliases use the same success assignment):
///
/// ```no_run
/// use nyanpasu_rpc_protocol::operations::SetStorageItem;
/// use nyanpasu_runtime_values::storage::StorageRevision;
/// type WriteResult = Result<StorageRevision, std::io::Error>;
/// #[nyanpasu_macro::rpc(scope = settings, local, result, operation = SetStorageItem)]
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
///     #[nyanpasu_macro::rpc(scope = settings, local, operation = SetStorageItem)]
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
///     #[nyanpasu_macro::rpc(scope = settings, local, operation = SetStorageItem)]
///     fn set_storage_item(key: String, value: String) -> bool { true }
/// }
/// ```
///
/// A mutation that names no scope does not compile:
///
/// ```compile_fail
/// fn a_mutation_without_a_scope_fails_to_compile() {
///     #[nyanpasu_macro::rpc(http)]
///     fn reorder() -> bool { true }
/// }
/// ```
///
/// Effect and scope are independent: a query may explicitly require `proxy` permission.
///
/// ```no_run
/// #[nyanpasu_macro::rpc(http, query, scope = proxy)]
/// fn proxy_scoped_query() -> bool { true }
/// ```
///
/// The name is checked in a constant, including declarations never called:
///
/// ```compile_fail,E0080
/// use nyanpasu_rpc_protocol::operations::SetStorageItem;
/// use nyanpasu_runtime_values::storage::StorageRevision;
/// fn operation_method_mismatch_fails_to_compile() {
///     #[nyanpasu_macro::rpc(scope = settings, local, operation = SetStorageItem)]
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
    native_linked: bool,
    handler: Option<RpcHandler>,
}

impl RegisteredCommand {
    pub(crate) fn metadata(&self) -> &CommandMetadata {
        &self.metadata
    }

    pub(crate) fn capabilities_available(&self) -> bool {
        self.capabilities_available
    }

    pub(crate) fn has_neutral_handler(&self) -> bool {
        self.handler.is_some()
    }

    pub(crate) fn native_linked(&self) -> bool {
        self.native_linked
    }

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
        if !crate::policy::is_command_runnable(
            &self.metadata,
            self.capabilities_available,
            self.handler.is_some(),
            self.native_linked,
            target,
        ) {
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
                    native_linked: false,
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
                native_linked: false,
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
    pub(crate) server: ServerInfo,
    pub(crate) provided: Vec<ShellCapability>,
    pub(crate) commands: CommandHandlers,
}

impl UnifiedRpc {
    /// Shares the explicitly assembled inputs with special transport adapters.
    pub fn dependencies(&self) -> Arc<RpcDependencies> {
        self.dependencies.clone()
    }

    pub fn new(
        dependencies: RpcDependencies,
        server: ServerInfo,
        provided: &[ShellCapability],
    ) -> Result<Self, SetupError> {
        let commands =
            register_commands(COMMAND_CATALOG, inventory::iter::<CommandEntry>, provided)?;
        Ok(Self {
            dependencies: Arc::new(dependencies),
            server,
            provided: provided.to_vec(),
            commands,
        })
    }

    /// Record native Tauri wrappers linked by the desktop shell so `rpc_describe`
    /// accurately reports desktop availability without introducing a GUI dependency.
    pub fn mark_native_linked(&mut self, names: impl IntoIterator<Item = &'static str>) {
        let commands = Arc::make_mut(&mut self.commands);
        for name in names {
            if let Some(command) = commands.get_mut(name)
                && command.metadata.kind == CommandKind::Native
            {
                command.native_linked = true;
            }
        }
    }

    pub fn command_names(&self) -> Vec<&'static str> {
        let mut names = self.commands.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        names
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

    /// Return the canonical discovery snapshot for `target` and resolved `auth`.
    #[must_use]
    pub fn describe(&self, target: RpcTarget, auth: &AuthContext) -> RpcDescribeResponse {
        RpcDescribeResponse {
            protocol: RPC_DESCRIBE_PROTOCOL_VERSION,
            server: self.server.clone(),
            transport: target,
            principal: auth.principal.clone(),
            scopes: auth.scopes.clone(),
            commands: crate::policy::describe_commands(&self.commands, target, auth),
        }
    }

    #[cfg(test)]
    pub(crate) fn call_meta(&self, target: RpcTarget) -> RpcCallMeta {
        RpcCallMeta {
            server: self.server.clone(),
            target,
            commands: Arc::clone(&self.commands),
        }
    }

    /// Dispatch a shared command with an already-resolved caller context.
    pub async fn call(
        &self,
        transport: RpcTransport,
        method: &str,
        caller: RpcCaller,
        params: Value,
    ) -> Result<Value, RpcError> {
        let invocation = self.audit_invocation(
            transport,
            method,
            caller.owner(),
            Some(&caller.auth().principal),
            None,
        );
        self.call_with_admission(&invocation, transport, method, Some(caller), params, || {
            Ok(())
        })
        .await
    }

    pub(crate) fn admit_command(
        &self,
        transport: RpcTransport,
        method: &str,
        auth: Option<&AuthContext>,
    ) -> Result<(), RpcError> {
        let target = match transport {
            RpcTransport::TauriIpc | RpcTransport::TauriProtocol => RpcTarget::Desktop,
            RpcTransport::Http => RpcTarget::Http,
            RpcTransport::Local => RpcTarget::Local,
        };
        if auth.is_none() {
            return Err(RpcError::unauthorized("authentication required"));
        }
        let command = self
            .commands
            .get(method)
            .ok_or_else(|| RpcError::unknown_method(method))?;
        command.neutral_handler(target)?;
        crate::policy::admit_resolved_command(&command.metadata, auth)
    }

    pub(crate) async fn call_with_admission(
        &self,
        invocation: &AuditInvocation,
        transport: RpcTransport,
        method: &str,
        caller: Option<RpcCaller>,
        params: Value,
        admit_resource: impl FnOnce() -> Result<(), RpcError>,
    ) -> Result<Value, RpcError> {
        let target = match transport {
            RpcTransport::TauriIpc | RpcTransport::TauriProtocol => RpcTarget::Desktop,
            RpcTransport::Http => RpcTarget::Http,
            RpcTransport::Local => RpcTarget::Local,
        };
        dispatch_with_audit(invocation, || {
            let caller = caller.ok_or_else(|| RpcError::unauthorized("authentication required"))?;
            let command = self
                .commands
                .get(method)
                .ok_or_else(|| RpcError::unknown_method(method))?;
            let handler = command.neutral_handler(target)?;
            crate::policy::admit_resolved_command(&command.metadata, Some(caller.auth()))?;
            admit_resource()?;
            let meta = RpcCallMeta {
                server: self.server.clone(),
                target,
                commands: self.commands.clone(),
            };
            handler(self.dependencies.clone(), meta, caller, params)
        })
        .await
    }

    /// Admit and audit a shell-bound invocation without accepting any shell handles.
    /// The callback is invoked only after admission, under the same audit guard as
    /// shared calls, so parameter failures and panics retain their existing semantics.
    pub async fn call_native(
        &self,
        method: &str,
        caller: RpcCaller,
        call: impl FnOnce() -> Result<RpcFuture, RpcError>,
    ) -> Result<Value, RpcError> {
        dispatch_audited(
            &self.dependencies.audit,
            RpcTransport::TauriIpc,
            caller.owner().clone(),
            Some(&caller.auth().principal),
            || self.audit_command_name(method),
            None,
            || {
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
                crate::policy::admit_resolved_command(&command.metadata, Some(caller.auth()))?;
                call()
            },
        )
        .await
    }

    fn audit_command_name(&self, method: &str) -> &'static str {
        self.commands
            .get_key_value(method)
            .map_or("unknown_method", |(name, _)| *name)
    }

    pub(crate) fn audit_invocation(
        &self,
        transport: RpcTransport,
        method: &str,
        owner: &RpcOwner,
        principal: Option<&Principal>,
        request_id: Option<nyanpasu_rpc_protocol::frame::RequestId>,
    ) -> AuditInvocation {
        self.dependencies.audit.invocation(
            transport,
            owner,
            principal,
            self.audit_command_name(method),
            request_id,
        )
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
mod authorization_tests;
#[cfg(all(test, feature = "http"))]
mod delivery_tests;
#[cfg(all(test, feature = "http"))]
mod observation_tests;
#[cfg(all(test, feature = "http"))]
mod settings_tests;
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
