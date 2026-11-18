//! Shared caller authorization, event filtering, and capability discovery policy.
//!
//! All transports evaluate command, direct-route, event, and `rpc_describe`
//! availability through these predicates inside the audited admission seam.

use nyanpasu_rpc_protocol::{
    AuthContext, CommandEffect, CommandKind, CommandMetadata, DescribedCommand, EventName,
    RpcError, RpcTarget, Scope, ShellCapability,
};

use crate::registry::CommandHandlers;

/// The canonical self-inspection query exempt from `Scope::Read`.
pub(crate) const RPC_DESCRIBE_METHOD: &str = "rpc_describe";

/// Return `true` when `command` is exposed on `target`, has all required shell
/// capabilities in the running host, and has a linked executable handler.
pub(crate) fn is_command_runnable(
    metadata: &CommandMetadata,
    capabilities_available: bool,
    has_neutral_handler: bool,
    native_linked: bool,
    target: RpcTarget,
) -> bool {
    if !metadata.targets.contains(&target) || !capabilities_available {
        return false;
    }
    match (target, metadata.kind) {
        (RpcTarget::Desktop, CommandKind::Native) => native_linked,
        _ => has_neutral_handler,
    }
}

/// Return `true` when an authenticated `auth` principal may invoke `command`
/// on `target` in this host.
///
/// Mirrors dispatch admission: target exposure, required shell capabilities,
/// handler linkage, and independent scope ownership (with the documented
/// resolved-identity exception for `rpc_describe`).
pub(crate) fn is_command_allowed(
    metadata: &CommandMetadata,
    capabilities_available: bool,
    has_neutral_handler: bool,
    native_linked: bool,
    target: RpcTarget,
    auth: &AuthContext,
) -> bool {
    is_command_runnable(
        metadata,
        capabilities_available,
        has_neutral_handler,
        native_linked,
        target,
    ) && has_command_scope(metadata, auth)
}

/// Whether `auth` satisfies the authority requirement of `command`.
pub(crate) fn has_command_scope(metadata: &CommandMetadata, auth: &AuthContext) -> bool {
    if metadata.name == RPC_DESCRIBE_METHOD {
        // Approved own-only self-inspection exception: any resolved principal
        // may call `rpc_describe` without holding `Scope::Read`.
        true
    } else {
        auth.has_scope(metadata.scope)
    }
}

/// Admit a command invocation after the registry confirms `command` exists and is
/// runnable on `target`.
pub(crate) fn admit_resolved_command(
    metadata: &CommandMetadata,
    caller: Option<&AuthContext>,
) -> Result<(), RpcError> {
    let auth = caller.ok_or_else(|| RpcError::unauthorized("authentication required"))?;
    if !has_command_scope(metadata, auth) {
        return Err(RpcError::forbidden(format!(
            "missing required scope `{}`",
            metadata.scope.as_str()
        )));
    }
    Ok(())
}

/// Admit a non-`/bridge/rpc` HTTP route before it touches filesystem or stream receivers.
pub(crate) fn admit_route(scope: Scope, caller: Option<&AuthContext>) -> Result<(), RpcError> {
    let auth = caller.ok_or_else(|| RpcError::unauthorized("authentication required"))?;
    if !auth.has_scope(scope) {
        return Err(RpcError::forbidden(format!(
            "missing required scope `{}`",
            scope.as_str()
        )));
    }
    Ok(())
}

/// Admit a named event subscription before acquiring a receiver or snapshot.
pub(crate) fn admit_event(
    event: EventName,
    provided: &[ShellCapability],
    target: RpcTarget,
    caller: Option<&AuthContext>,
) -> Result<(), RpcError> {
    if !event.targets().contains(&target)
        || event
            .capability()
            .is_some_and(|capability| !provided.contains(&capability))
    {
        return Err(RpcError::new(
            "unsupported",
            format!(
                "event `{}` is unavailable on this transport",
                event.as_str()
            ),
        ));
    }
    let auth = caller.ok_or_else(|| RpcError::unauthorized("authentication required"))?;
    if !auth.has_scope(event.scope()) {
        return Err(RpcError::forbidden(format!(
            "missing required scope `{}` for event `{}`",
            event.scope().as_str(),
            event.as_str()
        )));
    }
    Ok(())
}

/// Filter live or unnamed broadcast deliveries using the same target, capability,
/// and scope predicate as subscription admission.
pub(crate) fn event_visible_to(
    event: EventName,
    provided: &[ShellCapability],
    target: RpcTarget,
    caller: &AuthContext,
) -> bool {
    event.targets().contains(&target)
        && event
            .capability()
            .is_none_or(|capability| provided.contains(&capability))
        && caller.has_scope(event.scope())
}

/// Build the sorted discovery command list for `target` and `auth`, including
/// known commands on `target` whose `allowed` flag is `false`.
pub(crate) fn describe_commands(
    commands: &CommandHandlers,
    target: RpcTarget,
    auth: &AuthContext,
) -> Vec<DescribedCommand> {
    let mut described: Vec<DescribedCommand> = commands
        .values()
        .filter(|command| command.metadata().targets.contains(&target))
        .map(|command| {
            let metadata = command.metadata();
            let allowed = is_command_allowed(
                metadata,
                command.capabilities_available(),
                command.has_neutral_handler(),
                command.native_linked(),
                target,
                auth,
            );
            let effect: CommandEffect = metadata.effect;
            DescribedCommand {
                name: metadata.name.to_owned(),
                effect,
                scope: metadata.scope,
                allowed,
            }
        })
        .collect();
    described.sort_by(|left, right| left.name.cmp(&right.name));
    described
}
