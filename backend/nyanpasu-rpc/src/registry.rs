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

use crate::EventBus;

/// What every transport-neutral command may receive. Shells assemble it in
/// their composition root; it carries no shell handle, so no command can
/// reach a capability through it.
pub struct RpcDependencies {
    pub client: NyanpasuClient,
    pub storage: Storage,
    pub paths: PathResolver,
    pub events: EventBus,
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

    pub(crate) async fn test_rpc(
        directory: &tempfile::TempDir,
        events: EventBus,
        provided: &[ShellCapability],
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
}
