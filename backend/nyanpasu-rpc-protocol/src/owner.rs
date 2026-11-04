/// Who owns the caller-scoped resources a command opens, such as log sessions.
///
/// A transport derives it from the connection, never from the request
/// payload, and the domain compares owners by [`RpcOwner::label`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RpcOwner(String);

impl RpcOwner {
    /// A desktop webview window, identified by its window label.
    pub fn desktop(label: &str) -> Self {
        Self(label.to_owned())
    }

    /// A browser session, identified by its session cookie.
    pub fn http_session(session: uuid::Uuid) -> Self {
        Self(format!("http:{session}"))
    }

    /// One accepted local-socket connection. The identity ends with the
    /// connection; a reconnecting client is a new owner.
    pub fn local_connection(connection: u64) -> Self {
        Self(format!("local:{connection}"))
    }

    pub fn label(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_from_different_transports_never_collide() {
        let session = uuid::Uuid::nil();
        assert_eq!(RpcOwner::desktop("main").label(), "main");
        assert_eq!(
            RpcOwner::http_session(session).label(),
            format!("http:{session}")
        );
        assert_eq!(RpcOwner::local_connection(7).label(), "local:7");
        assert_ne!(RpcOwner::local_connection(1), RpcOwner::local_connection(2));
    }
}
