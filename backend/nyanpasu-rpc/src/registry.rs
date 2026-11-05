//! The command registry. `#[nyanpasu_macro::rpc]` submits one
//! [`CommandEntry`] per command; [`UnifiedRpc`] collects them once, keeps
//! only the commands whose shell capabilities the shell provides, and
//! dispatches a call only on a target the command opted into.

use std::{
    collections::HashMap,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{Arc, Weak},
};

use nyanpasu_core::{client::NyanpasuClient, storage::Storage};
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::{RpcError, RpcOwner, RpcTarget, ShellCapability};
use serde_json::Value;
use snafu::Snafu;

use crate::{EventBus, WebTickets};

/// What every transport-neutral command may receive. Shells assemble it in
/// their composition root; it carries no shell handle, so no command can
/// reach a capability through it.
pub struct RpcDependencies {
    pub client: NyanpasuClient,
    pub storage: Storage,
    pub paths: PathResolver,
    pub events: EventBus,
    /// Signs browsers in to the WebUI, when the shell serves it.
    pub web_tickets: WebTickets,
}

pub type RpcFuture = Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'static>>;
pub type RpcHandler = fn(Arc<RpcDependencies>, RpcOwner, Value) -> RpcFuture;

/// One application command, submitted by `#[nyanpasu_macro::rpc]`.
pub struct CommandEntry {
    pub name: &'static str,
    /// The targets the command opted into; the registry refuses the rest.
    pub targets: &'static [RpcTarget],
    /// The shell capabilities the command can reach.
    pub capabilities: &'static [ShellCapability],
    /// The transport-neutral handler. A command that is also a
    /// `#[tauri::command]` reaches the desktop through the shell's own
    /// handler instead.
    pub handler: RpcHandler,
}

inventory::collect!(CommandEntry);

/// A dependency the registry injects into a command, declared like
/// `tauri::State` so a command reads the same on every shell. The generated
/// handler passes a reference; the declaration only types the parameter, and
/// specta leaves it out of the frontend signature.
pub struct State<'a, T>(&'a T);

impl<T> Deref for State<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.0
    }
}

impl<T> specta::function::FunctionArg for State<'_, T> {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

#[derive(Debug, Snafu)]
pub enum RegistryError {
    #[snafu(display("duplicate RPC command: {name}"))]
    DuplicateCommand { name: &'static str },
}

/// The commands a shell serves, with their dependencies. Cloning shares both.
#[derive(Clone)]
pub struct UnifiedRpc {
    dependencies: Arc<RpcDependencies>,
    commands: Arc<HashMap<&'static str, &'static CommandEntry>>,
}

/// A registry handle that does not keep the application alive, for transport
/// infrastructure the application itself owns.
#[derive(Clone)]
pub struct WeakUnifiedRpc {
    dependencies: Weak<RpcDependencies>,
    commands: Arc<HashMap<&'static str, &'static CommandEntry>>,
}

/// The linked commands on `target` that need no shell capability, sorted: what
/// a shell that provides none registers. A command cannot reach the registry
/// that serves it, so listing methods reads the linked commands instead.
pub(crate) fn linked_command_names(target: RpcTarget) -> Vec<&'static str> {
    let mut names = inventory::iter::<CommandEntry>
        .into_iter()
        .filter(|entry| entry.targets.contains(&target) && entry.capabilities.is_empty())
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

impl UnifiedRpc {
    /// Registers every linked command whose capabilities are all in
    /// `provided`; a shell that lacks a capability does not offer the commands
    /// that need it.
    ///
    /// # Errors
    ///
    /// Fails when two linked commands share a name.
    pub fn new(
        dependencies: RpcDependencies,
        provided: &[ShellCapability],
    ) -> Result<Self, RegistryError> {
        let mut commands = HashMap::new();
        for entry in inventory::iter::<CommandEntry> {
            if !entry
                .capabilities
                .iter()
                .all(|capability| provided.contains(capability))
            {
                continue;
            }
            snafu::ensure!(
                commands.insert(entry.name, entry).is_none(),
                DuplicateCommandSnafu { name: entry.name }
            );
        }
        Ok(Self {
            dependencies: Arc::new(dependencies),
            commands: Arc::new(commands),
        })
    }

    pub fn dependencies(&self) -> &RpcDependencies {
        &self.dependencies
    }

    /// The registered commands exposed on `target`, sorted by name.
    pub fn command_names(&self, target: RpcTarget) -> Vec<&'static str> {
        let mut names = self
            .commands
            .values()
            .filter(|entry| entry.targets.contains(&target))
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    /// Runs `method` for a caller on `target` and waits for its real result,
    /// without a deadline. Dropping the future does not cancel work the
    /// command has already handed to its owner.
    ///
    /// # Errors
    ///
    /// `unknown_method` when no registered command has the name,
    /// `unsupported` when the command did not opt into `target`, and the
    /// command's own failures otherwise.
    pub async fn call(
        &self,
        target: RpcTarget,
        method: &str,
        owner: RpcOwner,
        params: Value,
    ) -> Result<Value, RpcError> {
        let entry = self
            .commands
            .get(method)
            .ok_or_else(|| RpcError::unknown_method(method))?;
        if !entry.targets.contains(&target) {
            return Err(RpcError::unsupported(method, target));
        }
        (entry.handler)(self.dependencies.clone(), owner, params).await
    }

    pub fn downgrade(&self) -> WeakUnifiedRpc {
        WeakUnifiedRpc {
            dependencies: Arc::downgrade(&self.dependencies),
            commands: self.commands.clone(),
        }
    }
}

impl WeakUnifiedRpc {
    /// The registry, unless every strong handle is gone.
    pub fn upgrade(&self) -> Option<UnifiedRpc> {
        Some(UnifiedRpc {
            dependencies: self.dependencies.upgrade()?,
            commands: self.commands.clone(),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use nyanpasu_rpc_protocol::RpcErrorKind;

    use super::*;
    use crate::IpcError;

    type Result<T> = std::result::Result<T, IpcError>;

    // Registered only in test binaries: a shared command without HTTP or local exposure.
    #[nyanpasu_macro::rpc]
    async fn desktop_only_probe(client: State<'_, NyanpasuClient>) -> Result<bool> {
        Ok(client.is_portable())
    }

    // Registered only in test binaries: a shared command that needs a shell capability.
    #[nyanpasu_macro::rpc(http, capabilities(clipboard))]
    async fn clipboard_probe() -> Result<bool> {
        Ok(true)
    }

    /// Stands in for a caller's window in a declaration; transports inject
    /// the owner they derive from the connection.
    #[expect(dead_code, reason = "only the command declaration names it")]
    pub(crate) struct Window;

    impl Window {
        #[expect(dead_code, reason = "only the command declaration calls it")]
        fn label(&self) -> &str {
            unreachable!("only the declaration names the window")
        }
    }

    // Registered only in test binaries: a command that panics before it replies.
    #[nyanpasu_macro::rpc(http, local)]
    async fn panic_probe() -> Result<bool> {
        panic!("the probe panics on purpose")
    }

    // Registered only in test binaries: reports the owner a transport derived.
    #[nyanpasu_macro::rpc(http, local, owner)]
    async fn owner_probe(window: Window) -> Result<String> {
        Ok(window.label().to_owned())
    }

    pub(crate) async fn test_rpc(
        directory: &tempfile::TempDir,
        events: EventBus,
        provided: &[ShellCapability],
    ) -> UnifiedRpc {
        test_rpc_serving(directory, events, provided, WebTickets::unavailable()).await
    }

    pub(crate) async fn test_rpc_serving(
        directory: &tempfile::TempDir,
        events: EventBus,
        provided: &[ShellCapability],
        web_tickets: WebTickets,
    ) -> UnifiedRpc {
        let client = nyanpasu_core::client::test_support::facade::test_client(directory).await;
        UnifiedRpc::new(
            RpcDependencies {
                client,
                storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
                paths: nyanpasu_core::utils::test_support::test_paths(
                    directory.path(),
                    directory.path().join("data"),
                ),
                events,
                web_tickets,
            },
            provided,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn each_target_serves_only_the_commands_that_opted_in() {
        let directory = tempfile::tempdir().unwrap();
        let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
        let owner = RpcOwner::desktop("main");

        let desktop = rpc
            .call(
                RpcTarget::Desktop,
                "desktop_only_probe",
                owner.clone(),
                Value::Null,
            )
            .await;
        assert_eq!(desktop, Ok(Value::Bool(false)));
        for target in [RpcTarget::Http, RpcTarget::Local] {
            let refused = rpc
                .call(target, "desktop_only_probe", owner.clone(), Value::Null)
                .await
                .unwrap_err();
            assert_eq!(refused.kind, RpcErrorKind::Unsupported);
            assert_eq!(
                refused.message,
                format!("command `desktop_only_probe` is unavailable over {target}")
            );
        }
        assert!(
            rpc.command_names(RpcTarget::Desktop)
                .contains(&"desktop_only_probe")
        );
        assert!(
            !rpc.command_names(RpcTarget::Http)
                .contains(&"desktop_only_probe")
        );
        assert!(rpc.command_names(RpcTarget::Http).contains(&"get_profiles"));
    }

    #[tokio::test]
    async fn commands_needing_a_missing_capability_are_not_registered() {
        let directory = tempfile::tempdir().unwrap();
        let full = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
        assert!(
            full.command_names(RpcTarget::Http)
                .contains(&"clipboard_probe")
        );

        let directory = tempfile::tempdir().unwrap();
        let headless = test_rpc(&directory, EventBus::new(Vec::new()), &[]).await;
        assert!(
            !headless
                .command_names(RpcTarget::Desktop)
                .contains(&"clipboard_probe")
        );
        let missing = headless
            .call(
                RpcTarget::Http,
                "clipboard_probe",
                RpcOwner::desktop("main"),
                Value::Null,
            )
            .await
            .unwrap_err();
        assert_eq!(missing.kind, RpcErrorKind::UnknownMethod);
        assert!(
            headless
                .command_names(RpcTarget::Http)
                .contains(&"get_profiles")
        );
    }

    #[tokio::test]
    async fn the_local_allowlist_is_the_router_command_set() {
        let directory = tempfile::tempdir().unwrap();
        let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
        let mut expected = vec![
            // Read access.
            "get_core_status",
            "get_profiles",
            "get_runtime_yaml_redacted",
            "inspect_runtime",
            "query_core_logs",
            "get_core_log_status",
            // Write access.
            "create_profile",
            "delete_profile",
            "replace_profile_definition",
            "set_global_transforms",
            "activate_profile",
            "read_profile_file",
            "save_profile_file",
            "reconcile_core",
            "stop_core",
            "get_runtime_yaml",
            // Introspection for the CLI and the WebUI sign-in.
            "list_methods",
            "issue_web_ticket",
        ];
        expected.sort_unstable();
        let local = rpc
            .command_names(RpcTarget::Local)
            .into_iter()
            .filter(|name| !name.ends_with("_probe"))
            .collect::<Vec<_>>();
        assert_eq!(local, expected);
        // A server that provides no capability serves exactly what it lists.
        let directory = tempfile::tempdir().unwrap();
        let headless = test_rpc(&directory, EventBus::new(Vec::new()), &[]).await;
        let listed = headless
            .call(
                RpcTarget::Local,
                "list_methods",
                RpcOwner::local_connection(1),
                Value::Null,
            )
            .await
            .unwrap();
        assert_eq!(
            listed,
            serde_json::json!(headless.command_names(RpcTarget::Local))
        );
        // Core lifecycle and the redacted YAML are new and not reviewed for the WebUI.
        let http = rpc.command_names(RpcTarget::Http);
        for name in ["reconcile_core", "stop_core", "get_runtime_yaml_redacted"] {
            assert!(!http.contains(&name), "{name}");
        }
    }

    #[tokio::test]
    async fn core_lifecycle_results_and_failures_reach_the_local_target() {
        let directory = tempfile::tempdir().unwrap();
        let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
        let owner = RpcOwner::local_connection(1);

        let missing = rpc
            .call(
                RpcTarget::Local,
                "get_runtime_yaml_redacted",
                owner.clone(),
                Value::Null,
            )
            .await
            .unwrap_err();
        assert_eq!(missing.kind, RpcErrorKind::ApplicationError);
        let reconciled = rpc
            .call(
                RpcTarget::Local,
                "reconcile_core",
                owner.clone(),
                Value::Null,
            )
            .await
            .unwrap();
        assert!(reconciled["applied"]["generation"].is_u64(), "{reconciled}");
        assert!(reconciled["status"]["host"].is_string(), "{reconciled}");
        let yaml = rpc
            .call(
                RpcTarget::Local,
                "get_runtime_yaml_redacted",
                owner.clone(),
                Value::Null,
            )
            .await
            .unwrap();
        assert!(yaml.as_str().unwrap().contains("<redacted>"), "{yaml}");
        // The test endpoint answers every operation as a reconcile, so the
        // stop fails; its domain error must still reach the caller intact.
        let stop = rpc
            .call(RpcTarget::Local, "stop_core", owner, Value::Null)
            .await
            .unwrap_err();
        assert_eq!(stop.kind, RpcErrorKind::ApplicationError);
        let domain = &stop.domain_error.as_ref().unwrap().0["kind"];
        assert_eq!(domain["domain"], "runtime", "{domain}");
        assert_eq!(domain["error"]["kind"], "stop_core", "{domain}");
    }

    struct FixedTicket;

    impl crate::WebTicketIssuer for FixedTicket {
        fn issue(&self) -> crate::WebTicket {
            crate::WebTicket {
                path: "/?ticket=fixed".into(),
                port: 7899,
            }
        }
    }

    #[tokio::test]
    async fn web_tickets_are_issued_locally_and_refused_without_a_listener() {
        let directory = tempfile::tempdir().unwrap();
        let owner = RpcOwner::local_connection(1);
        let without = test_rpc(&directory, EventBus::new(Vec::new()), &[]).await;
        let refused = without
            .call(
                RpcTarget::Local,
                "issue_web_ticket",
                owner.clone(),
                Value::Null,
            )
            .await
            .unwrap_err();
        assert_eq!(refused.kind, RpcErrorKind::ApplicationError);
        assert!(refused.message.contains("not being served"), "{refused}");

        let directory = tempfile::tempdir().unwrap();
        let serving = test_rpc_serving(
            &directory,
            EventBus::new(Vec::new()),
            &[],
            WebTickets::new(Arc::new(FixedTicket)),
        )
        .await;
        let ticket = serving
            .call(
                RpcTarget::Local,
                "issue_web_ticket",
                owner.clone(),
                Value::Null,
            )
            .await
            .unwrap();
        assert_eq!(
            ticket,
            serde_json::json!({"path": "/?ticket=fixed", "port": 7899})
        );
        // A browser session cannot mint tickets.
        let http = serving
            .call(RpcTarget::Http, "issue_web_ticket", owner, Value::Null)
            .await
            .unwrap_err();
        assert_eq!(http.kind, RpcErrorKind::Unsupported);
    }
}
