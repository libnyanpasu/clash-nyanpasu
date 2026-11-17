//! The permission layer: whether a caller is admitted to a resource.
//!
//! Every transport admits commands, events, direct routes and streams through [`Authority`],
//! inside the audited admission seam. It reads a resource's requirement, exposure and
//! capabilities from the generated resource table and the caller's grant, never the caller's
//! identity.
//!
//! A call passes these layers, outside in (design section 4.6 of the permission namespace spec):
//! transport, authentication, audit, resolution, admission here, then two reserved layers before
//! the handler. Data sharing will intercept responses that need the user's consent; redaction
//! will decide the response visibility, which today the `rpc` wrapper derives from the caller.
//! Both read the admitted resource from [`crate::registry::RpcCallMeta`].

use nyanpasu_rpc_protocol::{
    AuthContext, DescribedCommand, RpcError, RpcTarget, ScopeSet, ShellCapability,
    permission::{self, ResourceKind, ResourceMetadata},
};

use crate::registry::CommandHandlers;

/// The canonical self-inspection query any resolved caller may invoke.
pub(crate) const RPC_DESCRIBE_METHOD: &str = "rpc_describe";

/// The icon bytes route.
pub(crate) const ICON_FETCH: (ResourceKind, &str) = (ResourceKind::Route, "icon_fetch");
/// The log archive download route.
pub(crate) const LOGS_ARCHIVE: (ResourceKind, &str) = (ResourceKind::Route, "logs_archive");
/// The connection details stream, over HTTP and the local socket.
pub(crate) const CONNECTION_DETAILS: (ResourceKind, &str) =
    (ResourceKind::Stream, "connection_details");

/// The declared row of a route or stream the transports admit by name.
pub(crate) fn declared((kind, name): (ResourceKind, &str)) -> &'static ResourceMetadata {
    permission::find(kind, name).unwrap_or_else(|| panic!("`{name}` has no resource row"))
}

/// Admission against the resource table for the capabilities this host provides.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Authority<'a> {
    provided: &'a [ShellCapability],
}

impl<'a> Authority<'a> {
    pub(crate) fn new(provided: &'a [ShellCapability]) -> Self {
        Self { provided }
    }

    /// Admit `caller` to `resource` on `target`. The order is fixed: an unresolved caller is
    /// `401`; a resource not offered on `target`, or whose capabilities the host lacks, is
    /// `unsupported`; a missing requirement is `403`. Resolving the name, and for commands the
    /// linked handler, happens before this.
    pub(crate) fn admit(
        &self,
        resource: &ResourceMetadata,
        target: RpcTarget,
        caller: Option<&ScopeSet>,
    ) -> Result<(), RpcError> {
        let scopes = caller.ok_or_else(|| RpcError::unauthorized("authentication required"))?;
        if !self.offers(resource, target) {
            return Err(unavailable(resource, target));
        }
        if !permits(resource, scopes) {
            return Err(forbidden(resource));
        }
        Ok(())
    }

    /// Whether `caller` may receive `resource` on `target`: the predicate of [`Self::admit`],
    /// for filtering deliveries and describing commands.
    pub(crate) fn visible(
        &self,
        resource: &ResourceMetadata,
        target: RpcTarget,
        caller: &ScopeSet,
    ) -> bool {
        self.offers(resource, target) && permits(resource, caller)
    }

    fn offers(&self, resource: &ResourceMetadata, target: RpcTarget) -> bool {
        resource.targets.contains(&target)
            && resource
                .capabilities
                .iter()
                .all(|capability| self.provided.contains(capability))
    }
}

/// Whether `scopes` meet the requirement of `resource`. Any resolved caller may describe itself.
fn permits(resource: &ResourceMetadata, scopes: &ScopeSet) -> bool {
    (resource.is_command() && resource.name == RPC_DESCRIBE_METHOD)
        || scopes.contains(resource.scope)
}

/// The `unsupported` refusal of a resource that exists but is not offered here.
pub(crate) fn unavailable(resource: &ResourceMetadata, target: RpcTarget) -> RpcError {
    match resource.kind {
        ResourceKind::Command(_) if target == RpcTarget::Http => {
            RpcError::unsupported(resource.name)
        }
        ResourceKind::Command(_) => RpcError::new(
            "unsupported",
            format!("command `{}` is unavailable in this shell", resource.name),
        ),
        ResourceKind::Event => RpcError::new(
            "unsupported",
            format!("event `{}` is unavailable on this transport", resource.name),
        ),
        ResourceKind::Route | ResourceKind::Stream => RpcError::new(
            "unsupported",
            format!("`{}` is unavailable on this transport", resource.name),
        ),
    }
}

fn forbidden(resource: &ResourceMetadata) -> RpcError {
    let scope = resource.scope.as_str();
    match resource.kind {
        ResourceKind::Event => RpcError::forbidden(format!(
            "missing required scope `{scope}` for event `{}`",
            resource.name
        )),
        _ => RpcError::forbidden(format!("missing required scope `{scope}`")),
    }
}

/// Build the sorted discovery command list for `target` and `auth`, including
/// known commands on `target` whose `allowed` flag is `false`.
pub(crate) fn describe_commands(
    commands: &CommandHandlers,
    authority: Authority<'_>,
    target: RpcTarget,
    auth: &AuthContext,
) -> Vec<DescribedCommand> {
    let mut described: Vec<DescribedCommand> = commands
        .values()
        .filter(|command| command.metadata().targets.contains(&target))
        .map(|command| {
            let metadata = command.metadata();
            DescribedCommand {
                name: metadata.name.to_owned(),
                effect: metadata.effect,
                scope: metadata.scope,
                allowed: command.linked(target)
                    && authority.visible(metadata, target, &auth.scopes),
            }
        })
        .collect();
    described.sort_by(|left, right| left.name.cmp(&right.name));
    described
}

#[cfg(test)]
mod tests;
